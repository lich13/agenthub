use super::annotations::{
    extract_db_error_text, extract_db_planner_texts, extract_db_user_input_text,
    parse_executor_metadata_blob, parse_raw_summary, parse_trajectory_metadata_blob,
    ParsedRawSummary,
};
use super::*;
use crate::native_probe::{self, NativeProvider, NativeStreamSnapshot, NativeTurnEvent};

/// Rejects Go zero time (`0001-01-01...`), epoch 0, and malformed timestamps.
pub(super) fn valid_epoch_ms(ms: i64) -> Option<i64> {
    (ms > 86_400_000).then_some(ms)
}

pub(super) fn parse_valid_timestamp_ms(raw: &str) -> Option<i64> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.starts_with("0001-") || trimmed.starts_with("0000-") {
        return None;
    }
    if let Ok(nanos) = trimmed.parse::<i128>() {
        let ms = if nanos > 1_000_000_000_000_000 {
            (nanos / 1_000_000) as i64
        } else if nanos > 1_000_000_000_000 {
            nanos as i64
        } else {
            (nanos * 1000) as i64
        };
        return valid_epoch_ms(ms);
    }
    trimmed
        .parse::<jiff::Timestamp>()
        .or_else(|_| jiff::Timestamp::strptime("%Y-%m-%d %H:%M:%S%.f%:z", trimmed))
        .ok()
        .map(|ts| ts.as_millisecond())
        .and_then(valid_epoch_ms)
}

pub(super) fn format_ms_utc(ms: i64) -> Option<String> {
    valid_epoch_ms(ms)
        .and_then(|v| jiff::Timestamp::from_millisecond(v).ok())
        .map(|ts| crate::timestamps::format_utc(ts, true))
}

#[derive(Debug, Default, Clone)]
pub(crate) struct FtsMetaRow {
    pub(crate) archived: Option<bool>,
    pub(crate) title: Option<String>,
    pub(crate) workspace: Option<String>,
    pub(crate) step_count: Option<usize>,
    pub(crate) last_modified_ms: Option<i64>,
}

#[derive(Debug, Default, Clone)]
pub(crate) struct ConvMetaEntry {
    pub(crate) is_internal: bool,
    pub(crate) title: Option<String>,
    pub(crate) cwd: Option<String>,
    pub(crate) parent_id: Option<String>,
    pub(crate) root_id: Option<String>,
}

#[derive(Debug, Default, Clone)]
pub(crate) struct SubagentSidecarMeta {
    pub(crate) killed: bool,
    pub(crate) title: Option<String>,
}

#[derive(Debug, Default)]
pub(crate) struct StoreAuxiliaryIndex {
    pub(crate) fts_rows: BTreeMap<String, FtsMetaRow>,
    pub(crate) conv_meta: BTreeMap<String, ConvMetaEntry>,
    pub(crate) last_conv_cwd: BTreeMap<String, String>,
    pub(crate) sidecar_children: BTreeMap<String, Vec<String>>,
    pub(crate) sidecar_meta: BTreeMap<String, SubagentSidecarMeta>,
}

fn sql_int_or_text_ms(row: &rusqlite::Row<'_>, idx: usize) -> Option<i64> {
    use rusqlite::types::ValueRef;
    match row.get_ref(idx).ok()? {
        ValueRef::Integer(v) => {
            let ms = if v > 1_000_000_000_000_000 {
                v / 1_000_000
            } else if v > 1_000_000_000_000 {
                v
            } else {
                v.saturating_mul(1000)
            };
            valid_epoch_ms(ms)
        }
        ValueRef::Text(bytes) => std::str::from_utf8(bytes)
            .ok()
            .and_then(parse_valid_timestamp_ms),
        _ => None,
    }
}

pub(crate) fn load_fts_meta_rows(conn: &Connection) -> BTreeMap<String, FtsMetaRow> {
    let Ok(mut col_stmt) = conn.prepare("PRAGMA table_info(conversations_fts)") else {
        return BTreeMap::new();
    };
    let cols: BTreeSet<String> = col_stmt
        .query_map([], |r| r.get::<_, String>(1))
        .ok()
        .map(|it| it.flatten().collect())
        .unwrap_or_default();
    if !cols.contains("conversation_id") || !cols.contains("chunk_index") {
        return BTreeMap::new();
    }
    let arch_expr = if cols.contains("archived") {
        "archived"
    } else {
        "NULL"
    };
    let title_expr = if cols.contains("title") {
        "COALESCE(title, '')"
    } else {
        "''"
    };
    let ws_expr = if cols.contains("workspace") {
        "COALESCE(workspace, '')"
    } else {
        "''"
    };
    let sc_expr = if cols.contains("step_count") {
        "COALESCE(step_count, 0)"
    } else {
        "0"
    };
    let lm_expr = if cols.contains("last_modified_time") {
        "last_modified_time"
    } else {
        "NULL"
    };
    let sql = format!(
        "SELECT conversation_id, {arch_expr}, {title_expr}, {ws_expr}, {sc_expr}, {lm_expr} \
         FROM conversations_fts WHERE chunk_index = -1"
    );
    let Ok(mut stmt) = conn.prepare(&sql) else {
        return BTreeMap::new();
    };
    let mut out = BTreeMap::new();
    if let Ok(rows) = stmt.query_map([], |row| {
        let cid = row.get::<_, String>(0)?;
        let archived = row.get::<_, ValueOrString>(1).ok().map(|v| v.0);
        let title = row
            .get::<_, String>(2)
            .ok()
            .and_then(|s| super::prompt::clean_antigravity_auto_title(&s));
        let workspace = row
            .get::<_, String>(3)
            .ok()
            .map(|s| decode_file_uri(s.trim()))
            .filter(|s| !s.is_empty());
        let step_count = row
            .get::<_, i64>(4)
            .ok()
            .filter(|&c| c > 0)
            .map(|c| c as usize);
        let last_modified_ms = sql_int_or_text_ms(row, 5);
        Ok((
            cid,
            FtsMetaRow {
                archived,
                title,
                workspace,
                step_count,
                last_modified_ms,
            },
        ))
    }) {
        for (cid, meta) in rows.flatten() {
            if uuid::Uuid::parse_str(&cid).is_ok() {
                out.insert(cid, meta);
            }
        }
    }
    out
}

pub(crate) fn load_store_auxiliary_index(
    paths: &AntigravityPaths,
    conn: Option<&Connection>,
) -> StoreAuxiliaryIndex {
    let mut idx = StoreAuxiliaryIndex::default();
    if let Some(conn) = conn {
        idx.fts_rows = load_fts_meta_rows(conn);
    }

    let conv_meta_path = paths.home.join("cache/conversation_metadata.json");
    if conv_meta_path.is_file() && session_delete::within(&paths.home, &conv_meta_path) {
        if let Ok(bytes) = fs::read(&conv_meta_path) {
            if let Ok(doc) = serde_json::from_slice::<Value>(&bytes) {
                if let Some(convs) = doc["conversations"].as_object() {
                    for (cid, val) in convs {
                        if uuid::Uuid::parse_str(cid).is_err() {
                            continue;
                        }
                        let mut entry = ConvMetaEntry {
                            is_internal: val["is_internal"].as_bool() == Some(true)
                                || val["isInternal"].as_bool() == Some(true)
                                || val["summary"]["Internal"].as_bool() == Some(true)
                                || val["summary"]["internal"].as_bool() == Some(true),
                            title: None,
                            cwd: None,
                            parent_id: val["parent_conversation_id"]
                                .as_str()
                                .or_else(|| val["parentConversationId"].as_str())
                                .or_else(|| val["summary"]["ParentConversationID"].as_str())
                                .filter(|s| !s.is_empty() && *s != cid)
                                .map(str::to_owned),
                            root_id: val["root_conversation_id"]
                                .as_str()
                                .or_else(|| val["rootConversationId"].as_str())
                                .or_else(|| val["summary"]["RootConversationID"].as_str())
                                .filter(|s| !s.is_empty() && *s != cid)
                                .map(str::to_owned),
                        };
                        if let Some(raw_title) = val["summary"]
                            .as_str()
                            .or_else(|| val["summary"]["Title"].as_str())
                            .or_else(|| val["summary"]["title"].as_str())
                            .or_else(|| val["title"].as_str())
                        {
                            entry.title = if entry.is_internal
                                || entry.parent_id.is_some()
                                || entry.root_id.is_some()
                            {
                                super::prompt::clean_antigravity_subagent_title(raw_title)
                            } else {
                                super::prompt::clean_antigravity_auto_title(raw_title)
                            };
                        }
                        if let Some(uri) = val["summary"]["WorkspaceURIs"]
                            .as_array()
                            .or_else(|| val["summary"]["workspaceUris"].as_array())
                            .or_else(|| val["workspace_uris"].as_array())
                            .and_then(|a| a.first())
                            .and_then(Value::as_str)
                            .or_else(|| val["workspace"].as_str())
                            .map(decode_file_uri)
                            .filter(|s| !s.is_empty())
                        {
                            entry.cwd = Some(uri);
                        }
                        idx.conv_meta.insert(cid.clone(), entry);
                    }
                }
            }
        }
    }

    let last_conv_path = paths.home.join("cache/last_conversations.json");
    if last_conv_path.is_file() && session_delete::within(&paths.home, &last_conv_path) {
        if let Ok(bytes) = fs::read(&last_conv_path) {
            if let Ok(Value::Object(map)) = serde_json::from_slice::<Value>(&bytes) {
                for (ws, val) in map {
                    if let Some(cid) = val.as_str().filter(|s| uuid::Uuid::parse_str(s).is_ok()) {
                        let decoded = decode_file_uri(&ws);
                        if !decoded.is_empty() {
                            idx.last_conv_cwd.entry(cid.to_owned()).or_insert(decoded);
                        }
                    }
                }
            }
        }
    }

    if paths.brain_dir().is_dir() {
        if let Ok(parents) = fs::read_dir(paths.brain_dir()) {
            for p_entry in parents.flatten() {
                let pid = p_entry.file_name().to_string_lossy().into_owned();
                if uuid::Uuid::parse_str(&pid).is_err() {
                    continue;
                }
                let sub_dir = p_entry.path().join(".system_generated/subagents");
                if !sub_dir.is_dir() || !session_delete::within(&paths.home, &sub_dir) {
                    continue;
                }
                let Ok(kids) = fs::read_dir(&sub_dir) else {
                    continue;
                };
                for k_entry in kids.flatten() {
                    let k_path = k_entry.path();
                    if k_path.extension().and_then(|s| s.to_str()) != Some("json") {
                        continue;
                    }
                    let Some(cid) = k_path.file_stem().and_then(|s| s.to_str()) else {
                        continue;
                    };
                    if uuid::Uuid::parse_str(cid).is_err() || cid == pid {
                        continue;
                    }
                    let list = idx.sidecar_children.entry(pid.clone()).or_default();
                    if !list.contains(&cid.to_owned()) {
                        list.push(cid.to_owned());
                    }
                    if let Ok(raw) = fs::read(&k_path) {
                        if let Ok(doc) = serde_json::from_slice::<Value>(&raw) {
                            let state = doc["state"]
                                .as_str()
                                .or_else(|| doc["status"].as_str())
                                .unwrap_or("");
                            let killed = state.contains("KILLED") || state.contains("CANCELED");
                            let title = doc["subagentDescriptor"]["role"]
                                .as_str()
                                .or_else(|| doc["subagentDescriptor"]["typeName"].as_str())
                                .or_else(|| doc["title"].as_str())
                                .or_else(|| doc["name"].as_str())
                                .or_else(|| doc["type_name"].as_str())
                                .or_else(|| doc["typeName"].as_str())
                                .map(str::trim)
                                .filter(|s| !s.is_empty())
                                .map(str::to_owned);
                            idx.sidecar_meta
                                .insert(cid.to_owned(), SubagentSidecarMeta { killed, title });
                        }
                    }
                }
            }
        }
    }

    idx
}

#[derive(Debug, Default, Clone)]
pub(crate) struct ConvDbMetadata {
    pub(crate) parent_ids: Vec<String>,
    pub(crate) root_id: Option<String>,
    pub(crate) is_subagent: bool,
    pub(crate) cwd: Option<String>,
    pub(crate) step_count: usize,
    pub(crate) latest_termination_reason: u64,
    pub(crate) has_execution_error: bool,
}

pub(crate) fn inspect_conv_db_metadata(
    paths: &AntigravityPaths,
    id: &str,
) -> Option<ConvDbMetadata> {
    let db_path = paths.conversations_dir().join(format!("{id}.db"));
    let mut f = fs::File::open(&db_path).ok()?;
    let mut header = [0u8; 16];
    if f.read_exact(&mut header).is_err() || &header != b"SQLite format 3\0" {
        return None;
    }
    drop(f);
    let conn = Connection::open_with_flags(
        &db_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .ok()?;
    let mut out = ConvDbMetadata::default();

    let has_table = |name: &str| -> bool {
        conn.query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1 LIMIT 1",
            params![name],
            |_| Ok(()),
        )
        .is_ok()
    };

    if has_table("trajectory_metadata_blob") {
        let blob: Option<Vec<u8>> = conn
            .query_row(
                "SELECT COALESCE(data, blob) FROM trajectory_metadata_blob WHERE id = 'main' LIMIT 1",
                [],
                |r| r.get(0),
            )
            .ok()
            .or_else(|| {
                let cols: Vec<String> = conn
                    .prepare("PRAGMA table_info(trajectory_metadata_blob)")
                    .ok()?
                    .query_map([], |r| r.get::<_, String>(1))
                    .ok()?
                    .flatten()
                    .filter(|c| c != "id")
                    .collect();
                let col = cols.first()?;
                conn.query_row(
                    &format!("SELECT \"{col}\" FROM trajectory_metadata_blob ORDER BY CASE WHEN id = 'main' THEN 0 ELSE 1 END LIMIT 1"),
                    [],
                    |r| r.get::<_, Vec<u8>>(0),
                )
                .ok()
            });
        if let Some(raw) = blob {
            let parsed = parse_trajectory_metadata_blob(&raw);
            out.is_subagent |= parsed.is_subagent(id);
            if let Some(pid) = parsed
                .parent_conversation_id
                .as_deref()
                .filter(|p| !p.is_empty() && *p != id && uuid::Uuid::parse_str(p).is_ok())
            {
                out.parent_ids.push(pid.to_owned());
            }
            if let Some(rid) = parsed
                .root_conversation_id
                .filter(|r| !r.is_empty() && r != id && uuid::Uuid::parse_str(r).is_ok())
            {
                out.root_id = Some(rid);
            }
            if let Some(uri) = parsed.workspace_uris.first() {
                let decoded = decode_file_uri(uri);
                if !decoded.is_empty() {
                    out.cwd = Some(decoded);
                }
            }
        }
    }

    if has_table("parent_references") {
        let cols: Vec<String> = conn
            .prepare("PRAGMA table_info(parent_references)")
            .ok()
            .and_then(|mut s| {
                s.query_map([], |r| r.get::<_, String>(1))
                    .ok()
                    .map(|it| it.flatten().collect())
            })
            .unwrap_or_default();
        for candidate in ["parent_conversation_id", "parent_id", "conversation_id"] {
            if cols.iter().any(|c| c == candidate) {
                if let Ok(mut stmt) =
                    conn.prepare(&format!("SELECT \"{candidate}\" FROM parent_references"))
                {
                    if let Ok(rows) = stmt.query_map([], |r| r.get::<_, String>(0)) {
                        for pid in rows.flatten() {
                            let pid = pid.trim();
                            if !pid.is_empty()
                                && pid != id
                                && uuid::Uuid::parse_str(pid).is_ok()
                                && !out.parent_ids.iter().any(|x| x == pid)
                            {
                                out.is_subagent = true;
                                out.parent_ids.push(pid.to_owned());
                            }
                        }
                    }
                }
            }
        }
    }

    if has_table("steps") {
        if let Ok(cnt) = conn.query_row("SELECT count(*) FROM steps", [], |r| r.get::<_, i64>(0)) {
            out.step_count = cnt.max(0) as usize;
        }
    }

    if has_table("executor_metadata") {
        let cols: Vec<String> = conn
            .prepare("PRAGMA table_info(executor_metadata)")
            .ok()
            .and_then(|mut s| {
                s.query_map([], |r| r.get::<_, String>(1))
                    .ok()
                    .map(|it| it.flatten().collect())
            })
            .unwrap_or_default();
        if let Some(blob_col) = cols
            .iter()
            .find(|c| matches!(c.as_str(), "metadata" | "data" | "blob" | "payload"))
        {
            let order_col = if cols.iter().any(|c| c == "idx") {
                "idx"
            } else {
                "rowid"
            };
            if let Ok(raw) = conn.query_row(
                &format!(
                    "SELECT \"{blob_col}\" FROM executor_metadata ORDER BY {order_col} DESC LIMIT 1"
                ),
                [],
                |r| r.get::<_, Vec<u8>>(0),
            ) {
                if let Some(exec_meta) = parse_executor_metadata_blob(&raw) {
                    out.latest_termination_reason = exec_meta.termination_reason;
                    out.has_execution_error = exec_meta.has_execution_error;
                }
            }
        }
    }

    Some(out)
}

pub(crate) fn read_conv_meta_file(
    paths: &AntigravityPaths,
    id: &str,
) -> (Option<String>, Option<String>) {
    let meta_path = paths.conversations_dir().join(format!("{id}.meta"));
    if !meta_path.is_file() || !session_delete::within(&paths.home, &meta_path) {
        return (None, None);
    }
    let Ok(bytes) = fs::read(&meta_path) else {
        return (None, None);
    };
    let Ok(doc) = serde_json::from_slice::<Value>(&bytes) else {
        return (None, None);
    };
    let title = doc["title"]
        .as_str()
        .or_else(|| doc["summary"].as_str())
        .and_then(super::prompt::clean_antigravity_auto_title);
    let cwd = doc["cwd"]
        .as_str()
        .or_else(|| doc["workspace"].as_str())
        .or_else(|| {
            doc["workspace_uris"]
                .as_array()
                .and_then(|a| a.first())
                .and_then(Value::as_str)
        })
        .map(decode_file_uri)
        .filter(|s| !s.is_empty());
    (title, cwd)
}

pub(crate) fn load_history_jsonl_fallbacks(
    paths: &AntigravityPaths,
) -> BTreeMap<String, (Option<String>, Option<String>)> {
    let hist_path = paths.home.join("history.jsonl");
    if !hist_path.is_file() || !session_delete::within(&paths.home, &hist_path) {
        return BTreeMap::new();
    }
    let Ok(file) = fs::File::open(&hist_path) else {
        return BTreeMap::new();
    };
    let mut reader = BufReader::new(file);
    let mut out: BTreeMap<String, (Option<String>, Option<String>)> = BTreeMap::new();
    let _ = super::prompt::for_each_bounded_jsonl_row(&mut reader, |row| {
        let Some(cid) = row["conversationId"]
            .as_str()
            .or_else(|| row["conversation_id"].as_str())
            .filter(|s| uuid::Uuid::parse_str(s).is_ok())
        else {
            return std::ops::ControlFlow::Continue(());
        };
        let ws = row["workspace"]
            .as_str()
            .or_else(|| row["cwd"].as_str())
            .map(decode_file_uri)
            .filter(|s| !s.is_empty());
        let disp = row["display"]
            .as_str()
            .map(super::prompt::extract_antigravity_user_request)
            .and_then(|s| {
                s.lines()
                    .map(str::trim)
                    .find(|l| !l.is_empty() && !super::prompt::is_slash_meta_command_only(l))
                    .map(|l| l.chars().take(80).collect::<String>())
            });
        let entry = out.entry(cid.to_owned()).or_insert((None, None));
        if ws.is_some() {
            entry.0 = ws;
        }
        if entry.1.is_none() && disp.is_some() {
            entry.1 = disp;
        }
        std::ops::ControlFlow::Continue(())
    });
    out
}

pub(crate) fn transcript_has_subagent_reminder(paths: &AntigravityPaths, id: &str) -> bool {
    let logs = paths.brain_dir().join(id).join(".system_generated/logs");
    let chosen = [
        logs.join("transcript_full.jsonl"),
        logs.join("transcript.jsonl"),
    ]
    .into_iter()
    .find(|p| p.is_file());
    let Some(path) = chosen else {
        return false;
    };
    let Ok(file) = fs::File::open(&path) else {
        return false;
    };
    let mut buf = Vec::with_capacity(16 * 1024);
    if file.take(16 * 1024).read_to_end(&mut buf).is_err() {
        return false;
    }
    buf.windows(b"<subagent_reminder>".len())
        .any(|w| w == b"<subagent_reminder>")
}

fn safe_probe_text(raw: &str) -> String {
    crate::security::redact_output(raw)
        .chars()
        .take(1200)
        .collect()
}

fn read_all_transcript_rows(store: &AntigravityPaths, id: &str) -> Option<(PathBuf, Vec<Value>)> {
    let logs = store.brain_dir().join(id).join(".system_generated/logs");
    let chosen = [
        logs.join("transcript_full.jsonl"),
        logs.join("transcript.jsonl"),
    ]
    .into_iter()
    .find(|p| p.is_file())?;
    let file = fs::File::open(&chosen).ok()?;
    if file.metadata().ok()?.len() > 128 * 1024 * 1024 {
        return None;
    }
    let mut reader = BufReader::new(file);
    let mut rows = Vec::new();
    let _ = super::prompt::for_each_bounded_jsonl_row(&mut reader, |row| {
        rows.push(row);
        if rows.len() >= 200_000 {
            std::ops::ControlFlow::Break(())
        } else {
            std::ops::ControlFlow::Continue(())
        }
    });
    Some((chosen, rows))
}

fn extract_ask_question_from_row(row: &Value, row_idx: usize) -> Option<(String, String)> {
    let calls = row["tool_calls"].as_array()?;
    for (ci, call) in calls.iter().enumerate() {
        let name = call["name"]
            .as_str()
            .or_else(|| call["tool_name"].as_str())
            .unwrap_or("");
        if name != "ask_question" && name != "default_api:ask_question" {
            continue;
        }
        let has_inline_res = call
            .get("output")
            .or_else(|| call.get("result"))
            .and_then(Value::as_str)
            .is_some_and(|s| !s.trim().is_empty());
        if has_inline_res {
            continue;
        }
        let args = call
            .get("arguments")
            .or_else(|| call.get("args"))
            .map(super::super::parse_json_or_value)
            .unwrap_or(Value::Null);
        let texts: Vec<&str> = args["questions"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|q| q["question"].as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect();
        let body = if texts.is_empty() {
            "需要回复，请打开 Antigravity 处理。".into()
        } else {
            texts.join("\n")
        };
        let call_id = call["id"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| format!("row-{row_idx}-call-{ci}"));
        return Some((call_id, body));
    }
    None
}

fn extract_artifact_feedback_summary_from_rows(rows: &[Value]) -> Option<String> {
    for row in rows.iter().rev() {
        if super::super::step_requests_artifact_feedback(row) {
            if let Some(s) = row["codeAction"]["artifactMetadata"]["summary"]
                .as_str()
                .or_else(|| row["codeAction"]["artifactMetadata"]["Summary"].as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                return Some(s.to_owned());
            }
            return Some(String::new());
        }
        if let Some(calls) = row["tool_calls"].as_array() {
            for call in calls.iter().rev() {
                let args = call
                    .get("arguments")
                    .or_else(|| call.get("args"))
                    .map(super::super::parse_json_or_value)
                    .unwrap_or(Value::Null);
                let meta = args
                    .get("ArtifactMetadata")
                    .or_else(|| args.get("artifactMetadata"));
                if let Some(meta) = meta {
                    if meta["RequestFeedback"].as_bool() == Some(true)
                        || meta["requestFeedback"].as_bool() == Some(true)
                    {
                        let summary = meta["Summary"]
                            .as_str()
                            .or_else(|| meta["summary"].as_str())
                            .map(str::trim)
                            .unwrap_or("");
                        return Some(summary.to_owned());
                    }
                }
            }
        }
    }
    None
}

fn read_raw_summary_for_id(store: &AntigravityPaths, id: &str) -> Option<ParsedRawSummary> {
    let db_path = store.summaries_db();
    if !db_path.is_file() {
        return None;
    }
    let conn = Connection::open_with_flags(
        &db_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .ok()?;
    let blob: Option<Vec<u8>> = conn
        .query_row(
            "SELECT raw_summary FROM conversation_summaries WHERE conversation_id = ?1",
            params![id],
            |r| r.get(0),
        )
        .ok()
        .flatten();
    blob.map(|b| parse_raw_summary(&b))
}

fn offline_origin_anchor(
    store: &AntigravityPaths,
    id: &str,
    transcript_rows: Option<&[Value]>,
    raw_summary: Option<&ParsedRawSummary>,
) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        for (prefix, candidate) in [
            ("db", store.conversations_dir().join(format!("{id}.db"))),
            ("pb", store.conversations_dir().join(format!("{id}.pb"))),
        ] {
            if let Ok(meta) = fs::symlink_metadata(&candidate) {
                if meta.is_file() && !meta.file_type().is_symlink() {
                    return format!("{prefix}:{}:{}", meta.dev(), meta.ino());
                }
            }
        }
    }
    if let Some(raw) = raw_summary {
        if let Some(tid) = raw.trajectory_id.as_deref() {
            return format!("traj:{tid}:{}", raw.created_time_ms.unwrap_or(0));
        }
    }
    if let Some(rows) = transcript_rows {
        if let Some(first_ts) = rows
            .first()
            .and_then(|r| r["created_at"].as_str())
            .filter(|s| !s.is_empty())
        {
            return format!("ts:{first_ts}");
        }
    }
    format!("id:{id}")
}

fn turn_rows_have_unsettled_tools(turn_rows: &[Value]) -> bool {
    let mut open_tool_calls = 0usize;
    for row in turn_rows {
        let rtype = row["type"].as_str().unwrap_or("");
        let rstatus = row["status"].as_str().unwrap_or("DONE");
        if matches!(rstatus, "CANCELED" | "INTERRUPTED" | "INVALID")
            || matches!(rtype, "FINISH" | "ERROR_MESSAGE")
        {
            open_tool_calls = 0;
            continue;
        }
        if rtype == "SYSTEM_MESSAGE" || super::prompt::is_transcript_user_row(row) {
            continue;
        }
        if rtype == "PLANNER_RESPONSE" {
            if let Some(calls) = row["tool_calls"].as_array() {
                for call in calls {
                    let has_inline_result = call
                        .get("output")
                        .or_else(|| call.get("result"))
                        .and_then(Value::as_str)
                        .is_some_and(|s| !s.trim().is_empty());
                    if !has_inline_result {
                        let args_val = call
                            .get("arguments")
                            .or_else(|| call.get("args"))
                            .map(super::super::parse_json_or_value)
                            .unwrap_or(Value::Null);
                        let is_feedback_req =
                            args_val["ArtifactMetadata"]["RequestFeedback"].as_bool() == Some(true)
                                || args_val["artifactMetadata"]["requestFeedback"].as_bool()
                                    == Some(true);
                        if !is_feedback_req {
                            open_tool_calls = open_tool_calls.saturating_add(1);
                        }
                    }
                }
            }
        } else if !matches!(rtype, "FINISH" | "CHECKPOINT" | "EPHEMERAL_MESSAGE" | "") {
            open_tool_calls = open_tool_calls.saturating_sub(1);
        }
    }
    open_tool_calls > 0
}

pub(crate) fn build_offline_stream_snapshot(
    store: &AntigravityPaths,
    summary: &ProviderSessionSummary,
) -> Option<NativeStreamSnapshot> {
    let id = summary.id.as_str();
    let raw_summary = read_raw_summary_for_id(store, id);
    if raw_summary
        .as_ref()
        .is_some_and(|r| r.trajectory_meta.is_subagent(id))
        || transcript_has_subagent_reminder(store, id)
    {
        return None;
    }

    let transcript = read_all_transcript_rows(store, id);
    let rows_ref = transcript.as_ref().map(|(_, r)| r.as_slice());
    let store_canon = store
        .home
        .canonicalize()
        .unwrap_or_else(|_| store.home.clone());
    let origin = offline_origin_anchor(store, id, rows_ref, raw_summary.as_ref());
    let identity = format!(
        "cli:{}",
        hex::encode(Sha256::digest(format!(
            "{}\0{id}\0{origin}",
            store_canon.display()
        )))
    );
    let title = safe_probe_text(&native_probe::notification_title(
        &summary.title,
        id,
        &summary.cwd,
    ));
    let fallback_ts_ms = summary
        .updated_at
        .as_deref()
        .and_then(parse_valid_timestamp_ms)
        .or_else(|| {
            transcript
                .as_ref()
                .and_then(|(p, _)| file_mtime_ms(p))
                .and_then(valid_epoch_ms)
        })
        .or_else(|| file_mtime_ms(&store.canonical_path(id)).and_then(valid_epoch_ms));

    if let Some((_, rows)) = &transcript {
        if !rows.is_empty() {
            let record_count = rows.len() as u64;
            if summary.archived {
                return Some(NativeStreamSnapshot {
                    provider: NativeProvider::Antigravity,
                    session_key: id.into(),
                    id: id.into(),
                    title,
                    identity,
                    record_count,
                    settled_count: record_count,
                    events: Vec::new().into(),
                });
            }
            let turn_start = rows
                .iter()
                .rposition(|row| {
                    let st = row["type"].as_str().unwrap_or("");
                    let src = row["source"].as_str().unwrap_or("");
                    (super::prompt::is_transcript_user_row(row)
                        && super::prompt::transcript_row_has_user_payload(row))
                        || (st == "SYSTEM_MESSAGE" && src == "SYSTEM")
                })
                .unwrap_or(0);
            let turn_rows = &rows[turn_start..];
            let last_substantive = turn_rows.iter().enumerate().rev().find(|(_, row)| {
                let st = row["type"].as_str().unwrap_or("");
                let src = row["source"].as_str().unwrap_or("");
                !matches!(st, "CHECKPOINT" | "EPHEMERAL_MESSAGE" | "")
                    && !(st == "SYSTEM_MESSAGE" && src == "SYSTEM_SDK")
                    && !(super::prompt::is_transcript_user_row(row)
                        && !super::prompt::transcript_row_has_user_payload(row))
            });
            let canceled = raw_summary
                .as_ref()
                .is_some_and(ParsedRawSummary::is_user_canceled_or_killed)
                || last_substantive.is_some_and(|(_, r)| {
                    matches!(
                        r["status"].as_str().unwrap_or(""),
                        "CANCELED" | "INTERRUPTED"
                    )
                });

            let mut events = Vec::new();
            let settled_count = match summary.status.as_str() {
                "running" => turn_start as u64,
                "waiting" => {
                    let ts_ms = last_substantive
                        .and_then(|(_, r)| r["created_at"].as_str())
                        .and_then(parse_valid_timestamp_ms)
                        .or(fallback_ts_ms);
                    if let Some(timestamp_ms) = ts_ms {
                        if let Some(ws) = raw_summary.as_ref().and_then(|r| r.waiting_steps.last())
                        {
                            events.push(NativeTurnEvent {
                                prompt_id: None,
                                position: record_count,
                                turn_id: format!(
                                    "cli-wait:{turn_start}:{}:{}",
                                    ws.step_index, ws.proof_hash
                                ),
                                kind: ws.kind.into(),
                                body: safe_probe_text(&ws.body),
                                timestamp_ms,
                            });
                        } else if let Some((rel_idx, call_id, body)) =
                            turn_rows.iter().enumerate().rev().find_map(|(i, r)| {
                                extract_ask_question_from_row(r, turn_start + i)
                                    .map(|(cid, b)| (i, cid, b))
                            })
                        {
                            let pos = (turn_start + rel_idx + 1) as u64;
                            let row_ts = turn_rows[rel_idx]["created_at"]
                                .as_str()
                                .and_then(parse_valid_timestamp_ms)
                                .unwrap_or(timestamp_ms);
                            events.push(NativeTurnEvent {
                                prompt_id: None,
                                position: pos,
                                turn_id: format!("cli-ask:{turn_start}:{call_id}"),
                                kind: "reply_needed".into(),
                                body: safe_probe_text(&body),
                                timestamp_ms: row_ts,
                            });
                        } else {
                            let pos = last_substantive
                                .map(|(i, _)| (turn_start + i + 1) as u64)
                                .unwrap_or(record_count);
                            events.push(NativeTurnEvent {
                                prompt_id: None,
                                position: pos,
                                turn_id: format!("cli-auth:{turn_start}:{pos}"),
                                kind: "authorization".into(),
                                body: "操作需要授权，请打开 Antigravity 处理。".into(),
                                timestamp_ms,
                            });
                        }
                    }
                    turn_start as u64
                }
                "failed" => {
                    if !canceled {
                        let err_target = turn_rows
                            .iter()
                            .enumerate()
                            .rev()
                            .find(|(_, r)| {
                                r["type"].as_str() == Some("ERROR_MESSAGE")
                                    || matches!(
                                        r["status"].as_str().unwrap_or(""),
                                        "ERROR" | "FAILED"
                                    )
                            })
                            .or(last_substantive);
                        if let Some((rel_idx, err_row)) = err_target {
                            let ts_ms = err_row["created_at"]
                                .as_str()
                                .and_then(parse_valid_timestamp_ms)
                                .or(fallback_ts_ms);
                            if let Some(timestamp_ms) = ts_ms {
                                let raw_err = err_row["content"]
                                    .as_str()
                                    .filter(|s| !s.trim().is_empty())
                                    .or_else(|| {
                                        err_row["error"].as_str().filter(|s| !s.trim().is_empty())
                                    });
                                let body = match raw_err {
                                    Some(txt) => safe_probe_text(txt),
                                    None => "主回合未能完成，请打开 Antigravity 查看详情。".into(),
                                };
                                events.push(NativeTurnEvent {
                                    prompt_id: None,
                                    position: (turn_start + rel_idx + 1) as u64,
                                    turn_id: format!("cli-turn:{turn_start}"),
                                    kind: "failure".into(),
                                    body,
                                    timestamp_ms,
                                });
                            }
                        }
                    }
                    record_count
                }
                _ => {
                    let terminal_tail = last_substantive.is_some_and(|(_, r)| {
                        matches!(
                            r["type"].as_str().unwrap_or(""),
                            "PLANNER_RESPONSE" | "FINISH"
                        ) && r["status"].as_str().unwrap_or("DONE") == "DONE"
                    }) && !turn_rows_have_unsettled_tools(turn_rows);
                    if !canceled && terminal_tail {
                        if let Some((rel_idx, planner)) = turn_rows
                            .iter()
                            .enumerate()
                            .rev()
                            .find(|(_, r)| r["type"].as_str() == Some("PLANNER_RESPONSE"))
                        {
                            let ts_ms = planner["created_at"]
                                .as_str()
                                .and_then(parse_valid_timestamp_ms)
                                .or(fallback_ts_ms);
                            if let Some(timestamp_ms) = ts_ms {
                                let reply_text = planner["content"].as_str().unwrap_or("");
                                let detected_q =
                                    crate::codex::assistant_feedback_request(reply_text);
                                let artifact_fb = extract_artifact_feedback_summary_from_rows(
                                    &turn_rows[..=rel_idx],
                                );
                                let is_reply = summary.status == "reply_needed"
                                    || detected_q.is_some()
                                    || artifact_fb.is_some();
                                let kind = if is_reply {
                                    "reply_needed"
                                } else {
                                    "completion"
                                };
                                let body = if let Some(q) = detected_q {
                                    safe_probe_text(&q)
                                } else if !reply_text.trim().is_empty() {
                                    safe_probe_text(reply_text)
                                } else if let Some(fb) =
                                    artifact_fb.as_deref().filter(|s| !s.is_empty())
                                {
                                    safe_probe_text(fb)
                                } else if is_reply {
                                    "需要回复或确认计划，请打开 Antigravity 处理。".into()
                                } else {
                                    String::new()
                                };
                                events.push(NativeTurnEvent {
                                    prompt_id: None,
                                    position: (turn_start + rel_idx + 1) as u64,
                                    turn_id: format!("cli-turn:{turn_start}"),
                                    kind: kind.into(),
                                    body,
                                    timestamp_ms,
                                });
                            }
                        }
                    }
                    record_count
                }
            };

            return Some(NativeStreamSnapshot {
                provider: NativeProvider::Antigravity,
                session_key: id.into(),
                id: id.into(),
                title,
                identity,
                record_count,
                settled_count,
                events: events.into(),
            });
        }
    }

    // Secondary offline fallback: conversations/<id>.db steps + raw_summary
    let db_path = store.conversations_dir().join(format!("{id}.db"));
    let conn = Connection::open_with_flags(
        &db_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .ok()?;
    let mut stmt = conn
        .prepare("SELECT idx, step_type, COALESCE(status, 0), COALESCE(step_payload, X'') FROM steps ORDER BY idx ASC LIMIT 200000")
        .ok()?;
    let db_steps: Vec<(i64, i64, i64, Vec<u8>)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
        .ok()?
        .flatten()
        .collect();
    if db_steps.is_empty() {
        return None;
    }
    let record_count = db_steps.len() as u64;
    if summary.archived {
        return Some(NativeStreamSnapshot {
            provider: NativeProvider::Antigravity,
            session_key: id.into(),
            id: id.into(),
            title,
            identity,
            record_count,
            settled_count: record_count,
            events: Vec::new().into(),
        });
    }
    let turn_start = db_steps
        .iter()
        .rposition(|(_, st, _, blob)| *st == 14 && extract_db_user_input_text(blob).is_some())
        .or_else(|| db_steps.iter().rposition(|(_, st, _, _)| *st == 14))
        .unwrap_or(0);
    let canceled = raw_summary
        .as_ref()
        .is_some_and(ParsedRawSummary::is_user_canceled_or_killed)
        || db_steps
            .last()
            .is_some_and(|(_, _, status, _)| *status == 4);
    let mut events = Vec::new();
    let settled_count = match summary.status.as_str() {
        "running" => turn_start as u64,
        "waiting" => {
            if let Some(timestamp_ms) = fallback_ts_ms {
                if let Some(ws) = raw_summary.as_ref().and_then(|r| r.waiting_steps.last()) {
                    events.push(NativeTurnEvent {
                        prompt_id: None,
                        position: record_count,
                        turn_id: format!(
                            "cli-wait:{turn_start}:{}:{}",
                            ws.step_index, ws.proof_hash
                        ),
                        kind: ws.kind.into(),
                        body: safe_probe_text(&ws.body),
                        timestamp_ms,
                    });
                } else {
                    events.push(NativeTurnEvent {
                        prompt_id: None,
                        position: record_count,
                        turn_id: format!("cli-auth:{turn_start}:{record_count}"),
                        kind: "authorization".into(),
                        body: "操作需要授权，请打开 Antigravity 处理。".into(),
                        timestamp_ms,
                    });
                }
            }
            turn_start as u64
        }
        "failed" => {
            if !canceled {
                if let Some(timestamp_ms) = fallback_ts_ms {
                    let body = db_steps[turn_start..]
                        .iter()
                        .rev()
                        .find(|(_, st, status, _)| *st == 17 || *status == 7)
                        .and_then(|(_, _, _, blob)| {
                            extract_db_error_text(blob)
                                .filter(|s| !s.trim().is_empty())
                                .map(|s| safe_probe_text(&s))
                        })
                        .unwrap_or_else(|| "主回合未能完成，请打开 Antigravity 查看详情。".into());
                    events.push(NativeTurnEvent {
                        prompt_id: None,
                        position: record_count,
                        turn_id: format!("cli-turn:{turn_start}"),
                        kind: "failure".into(),
                        body,
                        timestamp_ms,
                    });
                }
            }
            record_count
        }
        _ => {
            let terminal_db_tail = db_steps
                .last()
                .is_some_and(|(_, st, status, _)| matches!(*st, 15 | 16) && *status == 3);
            if !canceled && terminal_db_tail {
                if let Some((rel_idx, (_, _, _, blob))) = db_steps[turn_start..]
                    .iter()
                    .enumerate()
                    .rev()
                    .find(|(_, (_, st, _, _))| *st == 15)
                {
                    if let Some(timestamp_ms) = fallback_ts_ms {
                        let reply = extract_db_planner_texts(blob).0.unwrap_or_default();
                        let detected_q = crate::codex::assistant_feedback_request(&reply);
                        let is_reply = summary.status == "reply_needed" || detected_q.is_some();
                        let body = if let Some(q) = detected_q {
                            safe_probe_text(&q)
                        } else if !reply.trim().is_empty() {
                            safe_probe_text(&reply)
                        } else if is_reply {
                            "需要回复或确认计划，请打开 Antigravity 处理。".into()
                        } else {
                            String::new()
                        };
                        events.push(NativeTurnEvent {
                            prompt_id: None,
                            position: (turn_start + rel_idx + 1) as u64,
                            turn_id: format!("cli-turn:{turn_start}"),
                            kind: if is_reply {
                                "reply_needed".into()
                            } else {
                                "completion".into()
                            },
                            body,
                            timestamp_ms,
                        });
                    }
                }
            }
            record_count
        }
    };

    Some(NativeStreamSnapshot {
        provider: NativeProvider::Antigravity,
        session_key: id.into(),
        id: id.into(),
        title,
        identity,
        record_count,
        settled_count,
        events: events.into(),
    })
}

pub fn scan_offline_store(
    store: &AntigravityPaths,
    only: Option<&str>,
) -> Result<crate::native_probe::NativeScan> {
    if !store.home.is_dir() {
        return Ok(crate::native_probe::NativeScan::default());
    }
    if let Some(id) = only {
        if uuid::Uuid::parse_str(id).is_err() {
            return Ok(crate::native_probe::NativeScan::default());
        }
    }
    let summaries = list_offline_summaries(store, only)?;
    let mut scan = crate::native_probe::NativeScan::default();
    for summary in summaries {
        if let Some(snapshot) = build_offline_stream_snapshot(store, &summary) {
            scan.streams.push(snapshot);
        }
    }
    Ok(scan)
}
