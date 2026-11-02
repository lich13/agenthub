use anyhow::Result;
use rusqlite::{Connection, OpenFlags, OptionalExtension};

use super::{thread_rows::table_columns, CodexPaths};

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
    if id.trim().is_empty() || !paths.state_db().is_file() {
        return Ok(CodexTaskIdentity::Unknown);
    }
    let conn = Connection::open_with_flags(paths.state_db(), OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let columns = table_columns(&conn, "threads")?;
    if !columns.iter().any(|column| column == "id") {
        return Ok(CodexTaskIdentity::Unknown);
    }
    let fields = [
        "source",
        "thread_source",
        "source_kind",
        "parent_thread_id",
        "parentThreadId",
        "agent_path",
        "agentPath",
        "agent_nickname",
        "agentNickname",
        "agent_role",
        "agentRole",
    ];
    let expressions = fields.map(|field| {
        if columns.iter().any(|column| column == field) {
            field
        } else {
            "NULL"
        }
    });
    let metadata = conn
        .query_row(
            &format!(
                "SELECT {} FROM threads WHERE id = ?1",
                expressions.join(", ")
            ),
            [id],
            |row| {
                (0..fields.len())
                    .map(|index| row.get::<_, Option<String>>(index))
                    .collect::<rusqlite::Result<Vec<_>>>()
            },
        )
        .optional()?;
    let Some(metadata) = metadata else {
        return Ok(CodexTaskIdentity::Unknown);
    };
    let source = metadata[0].as_deref().unwrap_or_default().trim();
    let origin = metadata[1]
        .as_deref()
        .or(metadata[2].as_deref())
        .unwrap_or_default()
        .trim();
    if metadata[3..]
        .iter()
        .flatten()
        .any(|value| !value.trim().is_empty())
    {
        return Ok(CodexTaskIdentity::Internal);
    }
    // Structured sources describe internal/subagent work; do not infer user identity from them.
    if (!origin.is_empty() && origin != "user")
        || source.starts_with('{')
        || source.starts_with('[')
    {
        return Ok(CodexTaskIdentity::Internal);
    }
    if origin == "user"
        || matches!(
            source,
            "cli" | "vscode" | "exec" | "appServer" | "app-server"
        )
    {
        Ok(CodexTaskIdentity::Main)
    } else {
        Ok(CodexTaskIdentity::Unknown)
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
}
