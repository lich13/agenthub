use anyhow::{Context, Result};
use rusqlite::{Connection, OpenFlags, OptionalExtension};

use super::{
    thread_rows::{coalesce_non_empty, first_non_empty, table_columns},
    CodexPaths,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexTaskIdentity {
    Main,
    Internal,
    Unknown,
}

impl CodexTaskIdentity {
    pub fn suppression_reason(self) -> Option<&'static str> {
        match self {
            Self::Main => None,
            Self::Internal => Some("internal_task"),
            Self::Unknown => Some("unconfirmed_task_identity"),
        }
    }
}

/// Identity comes from native task metadata, never title/body keywords or a Hook claim.
pub fn codex_task_identity(paths: &CodexPaths, id: &str) -> Result<CodexTaskIdentity> {
    if id.trim().is_empty() {
        return Ok(CodexTaskIdentity::Unknown);
    }
    let db = paths.state_db();
    match std::fs::metadata(&db) {
        Ok(meta) if meta.is_file() => {}
        Ok(_) => return Ok(CodexTaskIdentity::Unknown),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(CodexTaskIdentity::Unknown);
        }
        Err(err) => return Err(err).with_context(|| format!("stat {}", db.display())),
    }
    let conn = Connection::open_with_flags(&db, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let columns = table_columns(&conn, "threads")?;
    codex_task_identity_from_connection(&conn, &columns, id)
}

pub(crate) fn codex_task_identity_from_connection(
    conn: &Connection,
    columns: &std::collections::HashSet<String>,
    id: &str,
) -> Result<CodexTaskIdentity> {
    if !columns.iter().any(|column| column == "id") {
        return Ok(CodexTaskIdentity::Unknown);
    }
    let source_expr = coalesce_non_empty(columns, &["source"]);
    let origin_expr = coalesce_non_empty(columns, &["thread_source", "source_kind"]);
    let internal_fields = [
        "parent_thread_id",
        "parentThreadId",
        "agent_path",
        "agentPath",
        "agent_nickname",
        "agentNickname",
        "agent_role",
        "agentRole",
    ];
    let internal_exprs = internal_fields.map(|field| {
        if columns.contains(field) {
            field
        } else {
            "NULL"
        }
    });
    let sql = format!(
        "SELECT {source_expr}, {origin_expr}, {} FROM threads WHERE id = ?1",
        internal_exprs.join(", ")
    );
    let total_cols = 2 + internal_fields.len();
    let metadata = conn
        .query_row(&sql, [id], |row| {
            (0..total_cols)
                .map(|index| row.get::<_, Option<String>>(index))
                .collect::<rusqlite::Result<Vec<_>>>()
        })
        .optional()?;
    let Some(metadata) = metadata else {
        return Ok(CodexTaskIdentity::Unknown);
    };
    Ok(classify_codex_task_identity(
        first_non_empty([metadata[0].as_deref()]),
        first_non_empty([metadata[1].as_deref()]),
        metadata[2..].iter().map(Option::as_deref),
    ))
}

pub(super) fn classify_codex_task_identity<'a>(
    source: Option<&str>,
    origin: Option<&str>,
    internal_fields: impl IntoIterator<Item = Option<&'a str>>,
) -> CodexTaskIdentity {
    let source = source.unwrap_or_default().trim();
    let origin = origin.unwrap_or_default().trim();
    if internal_fields
        .into_iter()
        .flatten()
        .any(|value| !value.trim().is_empty())
    {
        return CodexTaskIdentity::Internal;
    }
    // Structured sources describe internal/subagent work; do not infer user identity from them.
    if (!origin.is_empty() && origin != "user")
        || source.starts_with('{')
        || source.starts_with('[')
    {
        return CodexTaskIdentity::Internal;
    }
    if origin == "user"
        || matches!(
            source,
            "cli" | "vscode" | "exec" | "appServer" | "app-server"
        )
    {
        CodexTaskIdentity::Main
    } else {
        CodexTaskIdentity::Unknown
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn probe_task_identity_requires_native_main_task_without_body_heuristics() {
        let home = std::env::temp_dir().join(format!("agenthub-identity-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&home).unwrap();
        let paths = CodexPaths { home: home.clone() };
        let conn = Connection::open(paths.state_db()).unwrap();
        conn.execute_batch("CREATE TABLE threads (id TEXT PRIMARY KEY, source TEXT, thread_source TEXT, agent_path TEXT, title TEXT, has_user_event INTEGER);
            INSERT INTO threads VALUES ('main', 'vscode', 'user', NULL, 'FINAL_ANSWER memory_summary.md', 0);
            INSERT INTO threads VALUES ('child', 'vscode', 'user', '/root/child', 'Normal title', 1);
            INSERT INTO threads VALUES ('internal', 'exec', 'internal', NULL, 'Normal title', 1);
            INSERT INTO threads VALUES ('unknown', 'future', NULL, NULL, 'Normal title', 1);
            INSERT INTO threads VALUES ('legacy', 'cli', NULL, NULL, 'Legacy', 0);").unwrap();
        for (id, expected) in [
            ("main", CodexTaskIdentity::Main),
            ("legacy", CodexTaskIdentity::Main),
            ("child", CodexTaskIdentity::Internal),
            ("internal", CodexTaskIdentity::Internal),
            ("unknown", CodexTaskIdentity::Unknown),
            ("missing", CodexTaskIdentity::Unknown),
        ] {
            assert_eq!(codex_task_identity(&paths, id).unwrap(), expected, "{id}");
        }
        drop(conn);
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn dual_origin_columns_coalesce_non_empty_identically_across_batch_and_single_lookup() {
        let home =
            std::env::temp_dir().join(format!("agenthub-dual-origin-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&home).unwrap();
        let paths = CodexPaths { home: home.clone() };
        let conn = Connection::open(paths.state_db()).unwrap();
        conn.execute_batch(
            "CREATE TABLE threads (
                id TEXT PRIMARY KEY,
                source TEXT,
                thread_source TEXT,
                source_kind TEXT,
                title TEXT
            );
            INSERT INTO threads VALUES ('null-ts-user-sk', NULL, NULL, 'user', 'Main via source_kind');
            INSERT INTO threads VALUES ('blank-ts-user-sk', '   ', '   ', 'user', 'Main via trimmed source_kind');
            INSERT INTO threads VALUES ('ctrl-ws-ts-user-sk', '\n\t', '\r\n\t ', 'user', 'Main via control-whitespace trim');
            INSERT INTO threads VALUES ('null-ts-internal-sk', 'cli', '\n\t', 'subagent', 'Internal via source_kind');
            INSERT INTO threads VALUES ('both-empty-no-source', NULL, '', ' \t\n ', 'Unconfirmed');",
        )
        .unwrap();
        drop(conn);

        for (id, expected) in [
            ("null-ts-user-sk", CodexTaskIdentity::Main),
            ("blank-ts-user-sk", CodexTaskIdentity::Main),
            ("ctrl-ws-ts-user-sk", CodexTaskIdentity::Main),
            ("null-ts-internal-sk", CodexTaskIdentity::Internal),
            ("both-empty-no-source", CodexTaskIdentity::Unknown),
        ] {
            assert_eq!(
                codex_task_identity(&paths, id).unwrap(),
                expected,
                "single {id}"
            );
        }

        let batch: std::collections::HashMap<_, _> =
            super::super::thread_rows::read_notification_thread_headers_with_identities(&paths)
                .unwrap()
                .into_iter()
                .map(|(summary, ident)| (summary.id, ident))
                .collect();
        assert_eq!(batch.get("null-ts-user-sk"), Some(&CodexTaskIdentity::Main));
        assert_eq!(
            batch.get("blank-ts-user-sk"),
            Some(&CodexTaskIdentity::Main)
        );
        assert_eq!(
            batch.get("ctrl-ws-ts-user-sk"),
            Some(&CodexTaskIdentity::Main)
        );
        assert_eq!(
            batch.get("both-empty-no-source"),
            Some(&CodexTaskIdentity::Unknown)
        );
        assert!(!batch.contains_key("null-ts-internal-sk"));

        let hidden_plan = crate::archive::hidden_delete_plan_with_work(
            &paths,
            &crate::scan_work::ScanWork::default(),
        )
        .unwrap();
        assert!(
            hidden_plan
                .hidden_ids
                .iter()
                .any(|id| id == "null-ts-internal-sk"),
            "archive visibility scan must coalesce non-empty source_kind across control whitespace"
        );

        fs::remove_dir_all(home).unwrap();
    }
}
