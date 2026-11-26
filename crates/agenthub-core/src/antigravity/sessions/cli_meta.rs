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

pub(crate) fn load_fts_meta_rows(conn: &Connection) -> Result<BTreeMap<String, FtsMetaRow>> {
    let cols = crate::db::table_columns(conn, "conversations_fts")?;
    if !cols.contains("conversation_id") || !cols.contains("chunk_index") {
        return Ok(BTreeMap::new());
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
        "SELECT COALESCE(CAST(conversation_id AS TEXT), ''), {arch_expr}, {title_expr}, {ws_expr}, {sc_expr}, {lm_expr} \
         FROM conversations_fts WHERE chunk_index = -1"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], |row| {
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
    })?;
    let mut out = BTreeMap::new();
    for row in rows {
        let (cid, meta) = row?;
        if uuid::Uuid::parse_str(&cid).is_ok() {
            out.insert(cid, meta);
        }
    }
    Ok(out)
}

#[derive(Clone, PartialEq, Eq)]
struct AuxIndexStamp {
    has_conn: bool,
    db: Option<SqliteStampPair>,
    conv_meta: Option<FileStamp>,
    last_conv: Option<FileStamp>,
    sidecars: Vec<(PathBuf, FileStamp)>,
}

fn optional_regular_stamp(path: &Path) -> Option<Option<FileStamp>> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_file() && !meta.file_type().is_symlink() => {
            Some(Some(FileStamp::from_metadata(&meta)))
        }
        Ok(_) => Some(None),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Some(None),
        Err(_) => None,
    }
}

fn compute_aux_index_stamp(paths: &AntigravityPaths, has_conn: bool) -> Option<AuxIndexStamp> {
    let db_path = paths.summaries_db();
    let db = if has_conn {
        Some(FileStamp::sqlite_pair(&db_path)?)
    } else {
        if optional_regular_stamp(&db_path)?.is_some() {
            return None;
        }
        None
    };
    let conv_meta_path = paths.home.join("cache/conversation_metadata.json");
    let conv_meta = optional_regular_stamp(&conv_meta_path)?;
    let last_conv_path = paths.home.join("cache/last_conversations.json");
    let last_conv = optional_regular_stamp(&last_conv_path)?;
    let mut sidecars = Vec::new();
    let brain_dir = paths.brain_dir();
    match fs::symlink_metadata(&brain_dir) {
        Ok(meta) if meta.is_dir() => {
            let parents = fs::read_dir(&brain_dir).ok()?;
            for p_res in parents {
                let p_entry = p_res.ok()?;
                let pid = p_entry.file_name().to_string_lossy().into_owned();
                if uuid::Uuid::parse_str(&pid).is_err() {
                    continue;
                }
                let sub_dir = p_entry.path().join(".system_generated/subagents");
                match fs::symlink_metadata(&sub_dir) {
                    Ok(sm) if sm.is_dir() => {
                        let kids = fs::read_dir(&sub_dir).ok()?;
                        for k_res in kids {
                            let k_entry = k_res.ok()?;
                            let k_path = k_entry.path();
                            if k_path.extension().and_then(|s| s.to_str()) != Some("json") {
                                continue;
                            }
                            if let Some(st) = optional_regular_stamp(&k_path)? {
                                sidecars.push((k_path, st));
                            }
                        }
                    }
                    Ok(_) => {}
                    Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                    Err(_) => return None,
                }
            }
            sidecars.sort_by(|a, b| a.0.cmp(&b.0));
        }
        Ok(_) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return None,
    }
    Some(AuxIndexStamp {
        has_conn,
        db,
        conv_meta,
        last_conv,
        sidecars,
    })
}

fn aux_index_weight(idx: &StoreAuxiliaryIndex) -> u64 {
    256 + idx.fts_rows.len() as u64 * 160
        + idx.conv_meta.len() as u64 * 192
        + idx.last_conv_cwd.len() as u64 * 128
        + idx.sidecar_children.len() as u64 * 96
        + idx.sidecar_meta.len() as u64 * 96
}

static STORE_AUX_INDEX_CACHE: StampedCache<AuxIndexStamp, std::sync::Arc<StoreAuxiliaryIndex>> =
    StampedCache::new(16, 8 * 1024 * 1024);

pub(crate) fn load_store_auxiliary_index(
    paths: &AntigravityPaths,
    conn: Option<&Connection>,
) -> std::sync::Arc<StoreAuxiliaryIndex> {
    let stamp = compute_aux_index_stamp(paths, conn.is_some());
    if let Some(ref stamp) = stamp {
        if let Some(cached) = STORE_AUX_INDEX_CACHE.get(&paths.home, stamp) {
            return cached;
        }
    }
    let (raw, complete) = load_store_auxiliary_index_uncached(paths, conn);
    let computed = std::sync::Arc::new(raw);
    if complete {
        if let Some(stamp) = stamp {
            if compute_aux_index_stamp(paths, conn.is_some()).as_ref() == Some(&stamp) {
                let weight = aux_index_weight(&computed);
                STORE_AUX_INDEX_CACHE.insert(paths.home.clone(), stamp, computed.clone(), weight);
            }
        }
    }
    computed
}

fn load_store_auxiliary_index_uncached(
    paths: &AntigravityPaths,
    conn: Option<&Connection>,
) -> (StoreAuxiliaryIndex, bool) {
    let mut idx = StoreAuxiliaryIndex::default();
    let mut complete = true;
    if let Some(conn) = conn {
        match load_fts_meta_rows(conn) {
            Ok(rows) => idx.fts_rows = rows,
            Err(_) => complete = false,
        }
    }

    let conv_meta_path = paths.home.join("cache/conversation_metadata.json");
    match fs::symlink_metadata(&conv_meta_path) {
        Ok(meta)
            if meta.is_file()
                && !meta.file_type().is_symlink()
                && session_delete::within(&paths.home, &conv_meta_path) =>
        {
            match fs::read(&conv_meta_path) {
                Ok(bytes) => {
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
                    } else {
                        complete = false;
                    }
                }
                Err(_) => complete = false,
            }
        }
        Ok(_) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => complete = false,
    }

    let last_conv_path = paths.home.join("cache/last_conversations.json");
    match fs::symlink_metadata(&last_conv_path) {
        Ok(meta)
            if meta.is_file()
                && !meta.file_type().is_symlink()
                && session_delete::within(&paths.home, &last_conv_path) =>
        {
            match fs::read(&last_conv_path) {
                Ok(bytes) => match serde_json::from_slice::<Value>(&bytes) {
                    Ok(Value::Object(map)) => {
                        for (ws, val) in map {
                            if let Some(cid) =
                                val.as_str().filter(|s| uuid::Uuid::parse_str(s).is_ok())
                            {
                                let decoded = decode_file_uri(&ws);
                                if !decoded.is_empty() {
                                    idx.last_conv_cwd.entry(cid.to_owned()).or_insert(decoded);
                                }
                            }
                        }
                    }
                    Ok(_) => {}
                    Err(_) => complete = false,
                },
                Err(_) => complete = false,
            }
        }
        Ok(_) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => complete = false,
    }

    let brain_dir = paths.brain_dir();
    match fs::symlink_metadata(&brain_dir) {
        Ok(meta) if meta.is_dir() => match fs::read_dir(&brain_dir) {
            Ok(parents) => {
                for p_res in parents {
                    let Ok(p_entry) = p_res else {
                        complete = false;
                        continue;
                    };
                    let pid = p_entry.file_name().to_string_lossy().into_owned();
                    if uuid::Uuid::parse_str(&pid).is_err() {
                        continue;
                    }
                    let sub_dir = p_entry.path().join(".system_generated/subagents");
                    match fs::symlink_metadata(&sub_dir) {
                        Ok(sm) if sm.is_dir() && session_delete::within(&paths.home, &sub_dir) => {
                            let Ok(kids) = fs::read_dir(&sub_dir) else {
                                complete = false;
                                continue;
                            };
                            for k_res in kids {
                                let Ok(k_entry) = k_res else {
                                    complete = false;
                                    continue;
                                };
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
                                match fs::read(&k_path) {
                                    Ok(raw) => {
                                        if let Ok(doc) = serde_json::from_slice::<Value>(&raw) {
                                            let state = doc["state"]
                                                .as_str()
                                                .or_else(|| doc["status"].as_str())
                                                .unwrap_or("");
                                            let killed = state.contains("KILLED")
                                                || state.contains("CANCELED");
                                            let title = doc["subagentDescriptor"]["role"]
                                                .as_str()
                                                .or_else(|| {
                                                    doc["subagentDescriptor"]["typeName"].as_str()
                                                })
                                                .or_else(|| doc["title"].as_str())
                                                .or_else(|| doc["name"].as_str())
                                                .or_else(|| doc["type_name"].as_str())
                                                .or_else(|| doc["typeName"].as_str())
                                                .map(str::trim)
                                                .filter(|s| !s.is_empty())
                                                .map(str::to_owned);
                                            idx.sidecar_meta.insert(
                                                cid.to_owned(),
                                                SubagentSidecarMeta { killed, title },
                                            );
                                        } else {
                                            complete = false;
                                        }
                                    }
                                    Err(_) => complete = false,
                                }
                            }
                        }
                        Ok(_) => {}
                        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                        Err(_) => complete = false,
                    }
                }
            }
            Err(_) => complete = false,
        },
        Ok(_) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => complete = false,
    }

    (idx, complete)
}

use crate::file_stamp::{FileStamp, StampedCache};

type SqliteStampPair = (FileStamp, Option<FileStamp>);

static CONV_DB_META_CACHE: StampedCache<SqliteStampPair, Option<ConvDbMetadata>> =
    StampedCache::new(512, 4 * 1024 * 1024);

#[derive(Clone)]
struct CachedTranscriptPlanner {
    pos: u64,
    ts_ms: Option<i64>,
    reply_non_empty: bool,
    safe_reply_body: String,
    detected_question: Option<String>,
    artifact_feedback: Option<String>,
}

#[derive(Clone)]
struct CachedTranscriptTurnSummary {
    has_subagent_reminder: bool,
    record_count: u64,
    first_created_at: Option<String>,
    file_mtime_ms: Option<i64>,
    turn_start: usize,
    last_substantive_pos: Option<u64>,
    last_substantive_ts_ms: Option<i64>,
    last_substantive_canceled: bool,
    last_substantive_terminal_done: bool,
    has_unsettled_tools: bool,
    waiting_ask_question: Option<(u64, String, String, Option<i64>)>,
    failed_target: Option<(u64, Option<i64>, String)>,
    planner_target: Option<CachedTranscriptPlanner>,
}

#[derive(Clone)]
struct CachedDbPlanner {
    pos: u64,
    reply_non_empty: bool,
    safe_reply_body: String,
    detected_question: Option<String>,
}

#[derive(Clone)]
struct CachedDbStepsTurnSummary {
    record_count: u64,
    turn_start: usize,
    last_status_canceled: bool,
    failed_body: String,
    terminal_planner: Option<CachedDbPlanner>,
}

static OFFLINE_TRANSCRIPT_CACHE: StampedCache<FileStamp, Option<CachedTranscriptTurnSummary>> =
    StampedCache::new(512, 8 * 1024 * 1024);
static OFFLINE_DB_STEPS_CACHE: StampedCache<SqliteStampPair, Option<CachedDbStepsTurnSummary>> =
    StampedCache::new(512, 8 * 1024 * 1024);

pub(crate) fn evict_idle_meta_cache(max_idle: std::time::Duration) {
    STORE_AUX_INDEX_CACHE.evict_idle(max_idle);
    CONV_DB_META_CACHE.evict_idle(max_idle);
    OFFLINE_TRANSCRIPT_CACHE.evict_idle(max_idle);
    OFFLINE_DB_STEPS_CACHE.evict_idle(max_idle);
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
    inspect_conv_db_metadata_checked(paths, id).ok().flatten()
}

pub(crate) fn inspect_conv_db_metadata_checked(
    paths: &AntigravityPaths,
    id: &str,
) -> Result<Option<ConvDbMetadata>> {
    let db_path = paths.conversations_dir().join(format!("{id}.db"));
    match fs::symlink_metadata(&db_path) {
        Ok(meta) if meta.is_file() => {}
        Ok(_) => return Ok(None),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err.into()),
    }
    let Some(stamp) = FileStamp::sqlite_pair(&db_path) else {
        return Ok(None);
    };
    if let Some(cached_val) = CONV_DB_META_CACHE.get(&db_path, &stamp) {
        return Ok(cached_val);
    }
    let computed = inspect_conv_db_metadata_uncached(&db_path, id)?;
    if FileStamp::sqlite_pair(&db_path).as_ref() == Some(&stamp) {
        let weight = 192
            + computed.as_ref().map_or(0, |m| {
                m.parent_ids
                    .iter()
                    .map(|p| 24 + p.len() as u64)
                    .sum::<u64>()
                    + m.root_id.as_ref().map_or(0, String::len) as u64
                    + m.cwd.as_ref().map_or(0, String::len) as u64
            });
        CONV_DB_META_CACHE.insert(db_path, stamp, computed.clone(), weight);
    }
    Ok(computed)
}

fn inspect_conv_db_metadata_uncached(db_path: &Path, id: &str) -> Result<Option<ConvDbMetadata>> {
    use rusqlite::OptionalExtension;
    let mut f = match fs::File::open(db_path) {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err.into()),
    };
    let mut header = Vec::with_capacity(16);
    (&mut f).take(16).read_to_end(&mut header)?;
    if header.as_slice() != b"SQLite format 3\0" {
        return Ok(None);
    }
    drop(f);
    let conn = Connection::open_with_flags(
        db_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    let _ = conn.busy_timeout(std::time::Duration::from_millis(50));
    let _ = conn.execute_batch("PRAGMA cache_size = -512; PRAGMA temp_store = MEMORY;");
    let mut out = ConvDbMetadata::default();

    let tables: std::collections::HashSet<String> = {
        let mut s = conn.prepare("SELECT name FROM sqlite_master WHERE type = 'table'")?;
        let rows = s.query_map([], |r| r.get::<_, String>(0))?;
        rows.collect::<rusqlite::Result<std::collections::HashSet<_>>>()?
    };
    let has_table = |name: &str| -> bool { tables.contains(name) };

    if has_table("trajectory_metadata_blob") {
        let cols = crate::db::table_columns(&conn, "trajectory_metadata_blob")?;
        let has_id = cols.contains("id");
        let mut data_cols: Vec<&str> = cols
            .iter()
            .map(String::as_str)
            .filter(|&c| c != "id")
            .collect();
        data_cols.sort_unstable();
        let blob: Option<Vec<u8>> = if !data_cols.is_empty() {
            let expr = if cols.contains("data") && cols.contains("blob") {
                "COALESCE(\"data\", \"blob\")".to_owned()
            } else {
                format!("\"{}\"", data_cols[0])
            };
            let order = if has_id {
                "ORDER BY CASE WHEN id = 'main' THEN 0 ELSE 1 END"
            } else {
                ""
            };
            conn.query_row(
                &format!("SELECT {expr} FROM trajectory_metadata_blob {order} LIMIT 1"),
                [],
                |r| r.get::<_, Option<Vec<u8>>>(0),
            )
            .optional()?
            .flatten()
        } else {
            None
        };
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
        let cols = crate::db::table_columns(&conn, "parent_references")?;
        for candidate in ["parent_conversation_id", "parent_id", "conversation_id"] {
            if cols.contains(candidate) {
                let mut stmt =
                    conn.prepare(&format!("SELECT \"{candidate}\" FROM parent_references"))?;
                let rows = stmt.query_map([], |r| r.get::<_, Option<String>>(0))?;
                for pid in rows {
                    if let Some(pid) = pid? {
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

    if has_table("steps") {
        let cnt: i64 = conn.query_row("SELECT count(*) FROM steps", [], |r| r.get(0))?;
        out.step_count = cnt.max(0) as usize;
    }

    if has_table("executor_metadata") {
        let cols = crate::db::table_columns(&conn, "executor_metadata")?;
        if let Some(blob_col) = ["metadata", "data", "blob", "payload"]
            .into_iter()
            .find(|c| cols.contains(*c))
        {
            let order_col = if cols.contains("idx") { "idx" } else { "rowid" };
            if let Some(raw) = conn
                .query_row(
                    &format!(
                        "SELECT \"{blob_col}\" FROM executor_metadata ORDER BY {order_col} DESC LIMIT 1"
                    ),
                    [],
                    |r| r.get::<_, Option<Vec<u8>>>(0),
                )
                .optional()?
                .flatten()
            {
                if let Some(exec_meta) = parse_executor_metadata_blob(&raw) {
                    out.latest_termination_reason = exec_meta.termination_reason;
                    out.has_execution_error = exec_meta.has_execution_error;
                }
            }
        }
    }

    Ok(Some(out))
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
    first_transcript_ts: Option<&str>,
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
    if let Some(first_ts) = first_transcript_ts.filter(|s| !s.is_empty()) {
        return format!("ts:{first_ts}");
    }
    format!("id:{id}")
}

fn inspect_offline_transcript_cached(
    store: &AntigravityPaths,
    id: &str,
) -> Result<Option<CachedTranscriptTurnSummary>> {
    let logs = store.brain_dir().join(id).join(".system_generated/logs");
    let mut chosen = None;
    for candidate in [
        logs.join("transcript_full.jsonl"),
        logs.join("transcript.jsonl"),
    ] {
        match fs::symlink_metadata(&candidate) {
            Ok(meta) if meta.is_file() => {
                chosen = Some(candidate);
                break;
            }
            Ok(_) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(err.into()),
        }
    }
    let Some(chosen) = chosen else {
        return Ok(None);
    };
    let Some(stamp) = FileStamp::of_nofollow(&chosen) else {
        return Ok(None);
    };
    if let Some(cached_val) = OFFLINE_TRANSCRIPT_CACHE.get(&chosen, &stamp) {
        return Ok(cached_val);
    }
    let computed = inspect_offline_transcript_uncached(&chosen)?;
    if FileStamp::of_nofollow(&chosen).as_ref() == Some(&stamp) {
        let weight = 256
            + computed.as_ref().map_or(0, |s| {
                s.first_created_at.as_ref().map_or(0, String::len) as u64
                    + s.waiting_ask_question
                        .as_ref()
                        .map_or(0, |(_, a, b, _)| (a.len() + b.len()) as u64)
                    + s.failed_target
                        .as_ref()
                        .map_or(0, |(_, _, b)| b.len() as u64)
                    + s.planner_target.as_ref().map_or(0, |p| {
                        p.safe_reply_body.len() as u64
                            + p.detected_question.as_ref().map_or(0, String::len) as u64
                            + p.artifact_feedback.as_ref().map_or(0, String::len) as u64
                    })
            });
        OFFLINE_TRANSCRIPT_CACHE.insert(chosen, stamp, computed.clone(), weight);
    }
    Ok(computed)
}

fn inspect_offline_transcript_uncached(
    chosen: &Path,
) -> Result<Option<CachedTranscriptTurnSummary>> {
    use std::io::{Seek, SeekFrom};
    let mut file = match crate::file_stamp::open_regular_nofollow(chosen) {
        Ok(file) => file,
        Err(err)
            if err
                .downcast_ref::<std::io::Error>()
                .is_some_and(|io_err| io_err.kind() == std::io::ErrorKind::NotFound) =>
        {
            return Ok(None);
        }
        Err(err) => return Err(err),
    };
    let meta = file.metadata()?;
    if meta.len() > 128 * 1024 * 1024 {
        return Ok(None);
    }
    let file_mtime_ms = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .and_then(valid_epoch_ms);
    let mut head_buf = Vec::with_capacity(16 * 1024);
    (&mut file).take(16 * 1024).read_to_end(&mut head_buf)?;
    let has_subagent_reminder = head_buf
        .windows(b"<subagent_reminder>".len())
        .any(|w| w == b"<subagent_reminder>");
    if has_subagent_reminder {
        return Ok(Some(CachedTranscriptTurnSummary {
            has_subagent_reminder: true,
            record_count: 0,
            first_created_at: None,
            file_mtime_ms: None,
            turn_start: 0,
            last_substantive_pos: None,
            last_substantive_ts_ms: None,
            last_substantive_canceled: false,
            last_substantive_terminal_done: false,
            has_unsettled_tools: false,
            waiting_ask_question: None,
            failed_target: None,
            planner_target: None,
        }));
    }
    file.seek(SeekFrom::Start(0))?;
    let mut reader = BufReader::new(file);
    let mut record_count = 0usize;
    let mut first_created_at = None;
    let mut turn_start = 0usize;
    let mut turn_rows = Vec::new();
    super::prompt::for_each_bounded_jsonl_row(&mut reader, |row| {
        if record_count == 0 {
            first_created_at = row["created_at"]
                .as_str()
                .filter(|s| !s.is_empty())
                .map(str::to_owned);
        }
        let st = row["type"].as_str().unwrap_or("");
        let src = row["source"].as_str().unwrap_or("");
        let is_turn_boundary = (super::prompt::is_transcript_user_row(&row)
            && super::prompt::transcript_row_has_user_payload(&row))
            || (st == "SYSTEM_MESSAGE" && src == "SYSTEM");
        if is_turn_boundary {
            turn_start = record_count;
            turn_rows.clear();
        }
        turn_rows.push(row);
        record_count += 1;
        if record_count >= 200_000 {
            std::ops::ControlFlow::Break(())
        } else {
            std::ops::ControlFlow::Continue(())
        }
    })?;
    if record_count == 0 {
        return Ok(None);
    }
    let record_count = record_count as u64;
    let turn_rows = turn_rows.as_slice();
    let last_substantive = turn_rows.iter().enumerate().rev().find(|(_, row)| {
        let st = row["type"].as_str().unwrap_or("");
        let src = row["source"].as_str().unwrap_or("");
        !matches!(st, "CHECKPOINT" | "EPHEMERAL_MESSAGE" | "")
            && !(st == "SYSTEM_MESSAGE" && src == "SYSTEM_SDK")
            && !(super::prompt::is_transcript_user_row(row)
                && !super::prompt::transcript_row_has_user_payload(row))
    });
    let last_substantive_pos = last_substantive.map(|(i, _)| (turn_start + i + 1) as u64);
    let last_substantive_ts_ms = last_substantive
        .and_then(|(_, r)| r["created_at"].as_str())
        .and_then(parse_valid_timestamp_ms);
    let last_substantive_canceled = last_substantive.is_some_and(|(_, r)| {
        matches!(
            r["status"].as_str().unwrap_or(""),
            "CANCELED" | "INTERRUPTED"
        )
    });
    let last_substantive_terminal_done = last_substantive.is_some_and(|(_, r)| {
        matches!(
            r["type"].as_str().unwrap_or(""),
            "PLANNER_RESPONSE" | "FINISH"
        ) && r["status"].as_str().unwrap_or("DONE") == "DONE"
    });
    let has_unsettled_tools = turn_rows_have_unsettled_tools(turn_rows);
    let waiting_ask_question = turn_rows.iter().enumerate().rev().find_map(|(i, r)| {
        let (call_id, body) = extract_ask_question_from_row(r, turn_start + i)?;
        let pos = (turn_start + i + 1) as u64;
        let row_ts = r["created_at"].as_str().and_then(parse_valid_timestamp_ms);
        Some((pos, call_id, safe_probe_text(&body), row_ts))
    });
    let failed_target = turn_rows
        .iter()
        .enumerate()
        .rev()
        .find(|(_, r)| {
            r["type"].as_str() == Some("ERROR_MESSAGE")
                || matches!(r["status"].as_str().unwrap_or(""), "ERROR" | "FAILED")
        })
        .or(last_substantive)
        .map(|(rel_idx, err_row)| {
            let ts_ms = err_row["created_at"]
                .as_str()
                .and_then(parse_valid_timestamp_ms);
            let raw_err = err_row["content"]
                .as_str()
                .filter(|s| !s.trim().is_empty())
                .or_else(|| err_row["error"].as_str().filter(|s| !s.trim().is_empty()));
            let body = match raw_err {
                Some(txt) => safe_probe_text(txt),
                None => "主回合未能完成，请打开 Antigravity 查看详情。".into(),
            };
            ((turn_start + rel_idx + 1) as u64, ts_ms, body)
        });
    let planner_target = turn_rows
        .iter()
        .enumerate()
        .rev()
        .find(|(_, r)| r["type"].as_str() == Some("PLANNER_RESPONSE"))
        .map(|(rel_idx, planner)| {
            let ts_ms = planner["created_at"]
                .as_str()
                .and_then(parse_valid_timestamp_ms);
            let reply_text = planner["content"].as_str().unwrap_or("");
            let detected_question =
                crate::codex::assistant_feedback_request(reply_text).map(|q| safe_probe_text(&q));
            let artifact_feedback =
                extract_artifact_feedback_summary_from_rows(&turn_rows[..=rel_idx]).map(|fb| {
                    if fb.is_empty() {
                        String::new()
                    } else {
                        safe_probe_text(&fb)
                    }
                });
            let reply_non_empty = !reply_text.trim().is_empty();
            CachedTranscriptPlanner {
                pos: (turn_start + rel_idx + 1) as u64,
                ts_ms,
                reply_non_empty,
                safe_reply_body: if reply_non_empty {
                    safe_probe_text(reply_text)
                } else {
                    String::new()
                },
                detected_question,
                artifact_feedback,
            }
        });
    Ok(Some(CachedTranscriptTurnSummary {
        has_subagent_reminder: false,
        record_count,
        first_created_at,
        file_mtime_ms,
        turn_start,
        last_substantive_pos,
        last_substantive_ts_ms,
        last_substantive_canceled,
        last_substantive_terminal_done,
        has_unsettled_tools,
        waiting_ask_question,
        failed_target,
        planner_target,
    }))
}

fn inspect_offline_db_steps_cached(
    store: &AntigravityPaths,
    id: &str,
) -> Result<Option<CachedDbStepsTurnSummary>> {
    let db_path = store.conversations_dir().join(format!("{id}.db"));
    match fs::symlink_metadata(&db_path) {
        Ok(meta) if meta.is_file() => {}
        Ok(_) => return Ok(None),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err.into()),
    }
    let Some(stamp) = FileStamp::sqlite_pair(&db_path) else {
        return Ok(None);
    };
    if let Some(cached_val) = OFFLINE_DB_STEPS_CACHE.get(&db_path, &stamp) {
        return Ok(cached_val);
    }
    let computed = inspect_offline_db_steps_uncached(&db_path)?;
    if FileStamp::sqlite_pair(&db_path).as_ref() == Some(&stamp) {
        let weight = 256
            + computed.as_ref().map_or(0, |s| {
                s.failed_body.len() as u64
                    + s.terminal_planner.as_ref().map_or(0, |p| {
                        p.safe_reply_body.len() as u64
                            + p.detected_question.as_ref().map_or(0, String::len) as u64
                    })
            });
        OFFLINE_DB_STEPS_CACHE.insert(db_path, stamp, computed.clone(), weight);
    }
    Ok(computed)
}

fn inspect_offline_db_steps_uncached(db_path: &Path) -> Result<Option<CachedDbStepsTurnSummary>> {
    let conn = Connection::open_with_flags(
        db_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    let _ = conn.busy_timeout(std::time::Duration::from_millis(50));
    let has_steps: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'steps')",
        [],
        |r| r.get(0),
    )?;
    if !has_steps {
        return Ok(None);
    }
    let total_check: usize = conn.query_row(
        "SELECT COUNT(*) FROM (SELECT 1 FROM steps LIMIT 200001)",
        [],
        |r| r.get(0),
    )?;
    if total_check == 0 {
        return Ok(None);
    }
    let total = total_check.min(200_000);
    let sql = if total_check <= 200_000 {
        "SELECT step_type, COALESCE(status, 0), COALESCE(step_payload, X'') FROM steps ORDER BY idx DESC"
    } else {
        "SELECT step_type, COALESCE(status, 0), COALESCE(step_payload, X'') FROM (SELECT idx, step_type, status, step_payload FROM steps ORDER BY idx ASC LIMIT 200000) ORDER BY idx DESC"
    };
    let mut stmt = conn.prepare(sql)?;
    let mut rows = stmt.query([])?;
    let mut offset = 0usize;
    let mut last_status_canceled = false;
    let mut terminal_db_tail = false;
    let mut latest_failed: Option<(usize, Option<String>)> = None;
    let mut latest_planner: Option<(usize, String)> = None;
    let mut first_rev_st14_offset: Option<usize> = None;
    let mut turn_start_offset: Option<usize> = None;

    while let Some(row) = rows.next()? {
        if offset >= total {
            break;
        }
        let st: i64 = row.get(0)?;
        let status: i64 = row.get(1)?;
        let blob_ref = row.get_ref(2)?;
        let blob = blob_ref.as_bytes().unwrap_or(&[]);

        if offset == 0 {
            last_status_canceled = status == 4;
            terminal_db_tail = matches!(st, 15 | 16) && status == 3;
        }
        if latest_failed.is_none() && (st == 17 || status == 7) {
            let txt = extract_db_error_text(blob)
                .filter(|s| !s.trim().is_empty())
                .map(|s| safe_probe_text(&s));
            latest_failed = Some((offset, txt));
        }
        if terminal_db_tail && latest_planner.is_none() && st == 15 {
            let reply = extract_db_planner_texts(blob).0.unwrap_or_default();
            latest_planner = Some((offset, reply));
        }
        if st == 14 {
            if first_rev_st14_offset.is_none() {
                first_rev_st14_offset = Some(offset);
            }
            if extract_db_user_input_text(blob).is_some() {
                turn_start_offset = Some(offset);
                break;
            }
        }
        offset += 1;
    }

    let max_offset = turn_start_offset
        .or(first_rev_st14_offset)
        .unwrap_or_else(|| total.saturating_sub(1));
    let turn_start = total.saturating_sub(1).saturating_sub(max_offset);
    let failed_body = latest_failed
        .filter(|(err_off, _)| *err_off <= max_offset)
        .and_then(|(_, txt)| txt)
        .unwrap_or_else(|| "主回合未能完成，请打开 Antigravity 查看详情。".into());
    let terminal_planner = if terminal_db_tail {
        latest_planner
            .filter(|(plan_off, _)| *plan_off <= max_offset)
            .map(|(plan_off, reply)| {
                let detected_question =
                    crate::codex::assistant_feedback_request(&reply).map(|q| safe_probe_text(&q));
                let reply_non_empty = !reply.trim().is_empty();
                CachedDbPlanner {
                    pos: (total - plan_off) as u64,
                    reply_non_empty,
                    safe_reply_body: if reply_non_empty {
                        safe_probe_text(&reply)
                    } else {
                        String::new()
                    },
                    detected_question,
                }
            })
    } else {
        None
    };
    Ok(Some(CachedDbStepsTurnSummary {
        record_count: total as u64,
        turn_start,
        last_status_canceled,
        failed_body,
        terminal_planner,
    }))
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
    let raw_summary = read_raw_summary_for_id(store, &summary.id);
    build_offline_stream_snapshot_with_raw(store, summary, raw_summary.as_ref(), None)
}

pub(crate) fn build_offline_stream_snapshot_with_raw(
    store: &AntigravityPaths,
    summary: &ProviderSessionSummary,
    raw_summary: Option<&ParsedRawSummary>,
    store_canon_override: Option<&Path>,
) -> Option<NativeStreamSnapshot> {
    build_offline_stream_snapshot_with_raw_checked(
        store,
        summary,
        raw_summary,
        store_canon_override,
    )
    .ok()
    .flatten()
}

pub(crate) fn build_offline_stream_snapshot_with_raw_checked(
    store: &AntigravityPaths,
    summary: &ProviderSessionSummary,
    raw_summary: Option<&ParsedRawSummary>,
    store_canon_override: Option<&Path>,
) -> Result<Option<NativeStreamSnapshot>> {
    let id = summary.id.as_str();
    if raw_summary.is_some_and(|r| r.trajectory_meta.is_subagent(id)) {
        return Ok(None);
    }
    if inspect_conv_db_metadata_checked(store, id)?.is_some_and(|m| m.is_subagent) {
        return Ok(None);
    }

    let transcript = inspect_offline_transcript_cached(store, id)?;
    if transcript.as_ref().is_some_and(|t| t.has_subagent_reminder)
        || (transcript.is_none() && transcript_has_subagent_reminder(store, id))
    {
        return Ok(None);
    }

    let owned_canon;
    let store_canon = match store_canon_override {
        Some(path) => path,
        None => {
            owned_canon = store
                .home
                .canonicalize()
                .unwrap_or_else(|_| store.home.clone());
            owned_canon.as_path()
        }
    };
    let origin = offline_origin_anchor(
        store,
        id,
        transcript
            .as_ref()
            .and_then(|t| t.first_created_at.as_deref()),
        raw_summary,
    );
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
        .or_else(|| transcript.as_ref().and_then(|t| t.file_mtime_ms))
        .or_else(|| file_mtime_ms(&store.canonical_path(id)).and_then(valid_epoch_ms));

    if let Some(t) = &transcript {
        if t.record_count > 0 {
            let record_count = t.record_count;
            if summary.archived {
                return Ok(Some(NativeStreamSnapshot {
                    provider: NativeProvider::Antigravity,
                    session_key: id.into(),
                    id: id.into(),
                    title,
                    identity,
                    record_count,
                    settled_count: record_count,
                    events: Vec::new().into(),
                }));
            }
            let turn_start = t.turn_start;
            let canceled = raw_summary.is_some_and(ParsedRawSummary::is_user_canceled_or_killed)
                || t.last_substantive_canceled;

            let mut events = Vec::new();
            let settled_count = match summary.status.as_str() {
                "running" => turn_start as u64,
                "waiting" => {
                    let ts_ms = t.last_substantive_ts_ms.or(fallback_ts_ms);
                    if let Some(timestamp_ms) = ts_ms {
                        if let Some(ws) = raw_summary.and_then(|r| r.waiting_steps.last()) {
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
                        } else if let Some((pos, call_id, body, row_ts)) = &t.waiting_ask_question {
                            events.push(NativeTurnEvent {
                                prompt_id: None,
                                position: *pos,
                                turn_id: format!("cli-ask:{turn_start}:{call_id}"),
                                kind: "reply_needed".into(),
                                body: body.clone(),
                                timestamp_ms: row_ts.unwrap_or(timestamp_ms),
                            });
                        } else {
                            let pos = t.last_substantive_pos.unwrap_or(record_count);
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
                        if let Some((pos, err_ts_ms, body)) = &t.failed_target {
                            if let Some(timestamp_ms) = (*err_ts_ms).or(fallback_ts_ms) {
                                events.push(NativeTurnEvent {
                                    prompt_id: None,
                                    position: *pos,
                                    turn_id: format!("cli-turn:{turn_start}"),
                                    kind: "failure".into(),
                                    body: body.clone(),
                                    timestamp_ms,
                                });
                            }
                        }
                    }
                    record_count
                }
                _ => {
                    let terminal_tail = t.last_substantive_terminal_done && !t.has_unsettled_tools;
                    if !canceled && terminal_tail {
                        if let Some(planner) = &t.planner_target {
                            if let Some(timestamp_ms) = planner.ts_ms.or(fallback_ts_ms) {
                                let is_reply = summary.status == "reply_needed"
                                    || planner.detected_question.is_some()
                                    || planner.artifact_feedback.is_some();
                                let kind = if is_reply {
                                    "reply_needed"
                                } else {
                                    "completion"
                                };
                                let body = if let Some(q) = &planner.detected_question {
                                    q.clone()
                                } else if planner.reply_non_empty {
                                    planner.safe_reply_body.clone()
                                } else if let Some(fb) = planner
                                    .artifact_feedback
                                    .as_deref()
                                    .filter(|s| !s.is_empty())
                                {
                                    fb.to_owned()
                                } else if is_reply {
                                    "需要回复或确认计划，请打开 Antigravity 处理。".into()
                                } else {
                                    String::new()
                                };
                                events.push(NativeTurnEvent {
                                    prompt_id: None,
                                    position: planner.pos,
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

            return Ok(Some(NativeStreamSnapshot {
                provider: NativeProvider::Antigravity,
                session_key: id.into(),
                id: id.into(),
                title,
                identity,
                record_count,
                settled_count,
                events: events.into(),
            }));
        }
    }

    // Secondary offline fallback: conversations/<id>.db steps + raw_summary (bounded stamp cache)
    let Some(db_turn) = inspect_offline_db_steps_cached(store, id)? else {
        return Ok(None);
    };
    let record_count = db_turn.record_count;
    if summary.archived {
        return Ok(Some(NativeStreamSnapshot {
            provider: NativeProvider::Antigravity,
            session_key: id.into(),
            id: id.into(),
            title,
            identity,
            record_count,
            settled_count: record_count,
            events: Vec::new().into(),
        }));
    }
    let turn_start = db_turn.turn_start;
    let canceled = raw_summary.is_some_and(ParsedRawSummary::is_user_canceled_or_killed)
        || db_turn.last_status_canceled;
    let mut events = Vec::new();
    let settled_count = match summary.status.as_str() {
        "running" => turn_start as u64,
        "waiting" => {
            if let Some(timestamp_ms) = fallback_ts_ms {
                if let Some(ws) = raw_summary.and_then(|r| r.waiting_steps.last()) {
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
                    events.push(NativeTurnEvent {
                        prompt_id: None,
                        position: record_count,
                        turn_id: format!("cli-turn:{turn_start}"),
                        kind: "failure".into(),
                        body: db_turn.failed_body,
                        timestamp_ms,
                    });
                }
            }
            record_count
        }
        _ => {
            if !canceled {
                if let (Some(planner), Some(timestamp_ms)) =
                    (&db_turn.terminal_planner, fallback_ts_ms)
                {
                    let is_reply =
                        summary.status == "reply_needed" || planner.detected_question.is_some();
                    let body = if let Some(q) = &planner.detected_question {
                        q.clone()
                    } else if planner.reply_non_empty {
                        planner.safe_reply_body.clone()
                    } else if is_reply {
                        "需要回复或确认计划，请打开 Antigravity 处理。".into()
                    } else {
                        String::new()
                    };
                    events.push(NativeTurnEvent {
                        prompt_id: None,
                        position: planner.pos,
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
            record_count
        }
    };

    Ok(Some(NativeStreamSnapshot {
        provider: NativeProvider::Antigravity,
        session_key: id.into(),
        id: id.into(),
        title,
        identity,
        record_count,
        settled_count,
        events: events.into(),
    }))
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
    let (summaries, raw_summaries) = super::list_offline_summaries_with_raw(store, only)?;
    let store_canon = store
        .home
        .canonicalize()
        .unwrap_or_else(|_| store.home.clone());
    let mut scan = crate::native_probe::NativeScan::default();
    for summary in summaries {
        let raw = raw_summaries.get(&summary.id);
        match build_offline_stream_snapshot_with_raw_checked(
            store,
            &summary,
            raw,
            Some(&store_canon),
        ) {
            Ok(Some(snapshot)) => scan.streams.push(snapshot),
            Ok(None) => {}
            Err(_) => scan.errors += 1,
        }
    }
    Ok(scan)
}

#[cfg(test)]
mod tests {
    use super::annotations::ParsedWaitingStep;
    use super::*;
    use crate::provider_sessions::ProviderSessionSummary;

    fn push_varint(out: &mut Vec<u8>, mut v: u64) {
        while v >= 0x80 {
            out.push(((v as u8) & 0x7f) | 0x80);
            v >>= 7;
        }
        out.push(v as u8);
    }

    fn pb_bytes(field: u64, bytes: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        push_varint(&mut out, (field << 3) | 2);
        push_varint(&mut out, bytes.len() as u64);
        out.extend_from_slice(bytes);
        out
    }

    fn pb_string(field: u64, val: &str) -> Vec<u8> {
        pb_bytes(field, val.as_bytes())
    }

    #[test]
    fn offline_dual_source_cache_detects_wal_commit_and_raw_summary_question_change() {
        let root = std::env::temp_dir().join(format!(
            "agenthub-ag-offline-cache-{}",
            uuid::Uuid::new_v4()
        ));
        let conv_dir = root.join("conversations");
        fs::create_dir_all(&conv_dir).unwrap();
        let store = AntigravityPaths::offline(root.clone());

        // 1. SQLite .db in WAL mode without checkpoint: committing to -wal must invalidate OFFLINE_DB_STEPS_CACHE
        let db_id = "11111111-2222-4333-8444-555555555555";
        let db_path = conv_dir.join(format!("{db_id}.db"));
        let conn = Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA wal_autocheckpoint = 0;
             CREATE TABLE steps (idx INTEGER PRIMARY KEY, step_type INTEGER, status INTEGER, step_payload BLOB);",
        )
        .unwrap();
        let user_blob = pb_bytes(19, &pb_string(1, "测试离线 WAL 缓存"));
        conn.execute(
            "INSERT INTO steps (idx, step_type, status, step_payload) VALUES (0, 14, 3, ?1)",
            params![user_blob],
        )
        .unwrap();
        let mut summary = ProviderSessionSummary {
            id: db_id.into(),
            session_key: db_id.into(),
            format_version: String::new(),
            title: "WAL Cache Test".into(),
            cwd: "/workspace/example.invalid".into(),
            workspace_root: None,
            path: db_path.clone(),
            status: "recent".into(),
            archived: false,
            updated_at: Some("2026-11-25T12:00:00Z".into()),
            message_count: 1,
            last_message: None,
            can_rename: true,
            rename_block_reason: None,
            can_archive: true,
            archive_block_reason: None,
            can_delete: true,
            delete_block_reason: None,
            storage_size: None,
            read_error: None,
            read_warning: None,
        };
        let first_snap =
            build_offline_stream_snapshot_with_raw(&store, &summary, None, None).unwrap();
        assert!(
            first_snap.events.is_empty(),
            "user-only step must not emit completion"
        );

        let planner_inner = pb_string(1, "已在未 checkpoint 的 WAL 中完成最终答复。");
        let planner_blob = pb_bytes(20, &planner_inner);
        conn.execute(
            "INSERT INTO steps (idx, step_type, status, step_payload) VALUES (1, 15, 3, ?1)",
            params![planner_blob],
        )
        .unwrap();
        summary.updated_at = Some("2026-11-25T12:00:05Z".into());

        let second_snap =
            build_offline_stream_snapshot_with_raw(&store, &summary, None, None).unwrap();
        assert_eq!(second_snap.events.len(), 1);
        assert_eq!(second_snap.events[0].kind, "completion");
        assert!(second_snap.events[0].body.contains("完成最终答复"));
        drop(conn);

        // 2. Transcript unchanged (cache hit) while raw_summary waiting question proof_hash changes
        let tr_id = "66666666-7777-4888-9999-aaaaaaaaaaaa";
        let logs_dir = store.brain_dir().join(tr_id).join(".system_generated/logs");
        fs::create_dir_all(&logs_dir).unwrap();
        let tr_path = logs_dir.join("transcript.jsonl");
        fs::write(
            &tr_path,
            concat!(
                r#"{"type":"USER_INPUT","source":"USER","content":"部署确认","created_at":"2026-11-25T12:01:00Z"}"#,
                "\n",
                r#"{"type":"PLANNER_RESPONSE","status":"WAITING","content":"等待确认","created_at":"2026-11-25T12:01:02Z"}"#,
                "\n"
            ),
        )
        .unwrap();
        let mut tr_summary = summary.clone();
        tr_summary.id = tr_id.into();
        tr_summary.session_key = tr_id.into();
        tr_summary.status = "waiting".into();
        let mut raw_a = ParsedRawSummary::default();
        raw_a.waiting_steps.push(ParsedWaitingStep {
            step_index: 2,
            kind: "reply_needed",
            body: "问题一：确认目标环境？".into(),
            proof_hash: "proof-a".into(),
        });
        let snap_a =
            build_offline_stream_snapshot_with_raw(&store, &tr_summary, Some(&raw_a), None)
                .unwrap();
        assert_eq!(snap_a.events.len(), 1);
        assert_eq!(snap_a.events[0].turn_id, "cli-wait:0:2:proof-a");
        assert_eq!(snap_a.events[0].body, "问题一：确认目标环境？");

        let mut raw_b = ParsedRawSummary::default();
        raw_b.waiting_steps.push(ParsedWaitingStep {
            step_index: 3,
            kind: "reply_needed",
            body: "问题二：是否同时升级本机？".into(),
            proof_hash: "proof-b".into(),
        });
        let snap_b =
            build_offline_stream_snapshot_with_raw(&store, &tr_summary, Some(&raw_b), None)
                .unwrap();
        assert_eq!(snap_b.events.len(), 1);
        assert_eq!(snap_b.events[0].turn_id, "cli-wait:0:3:proof-b");
        assert_eq!(snap_b.events[0].body, "问题二：是否同时升级本机？");

        evict_idle_meta_cache(std::time::Duration::ZERO);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn transient_sqlite_lock_is_not_cached_and_recovers_with_unchanged_file_stamp() {
        let root = std::env::temp_dir().join(format!(
            "agenthub-ag-transient-db-lock-{}",
            uuid::Uuid::new_v4()
        ));
        let conv_dir = root.join("conversations");
        fs::create_dir_all(&conv_dir).unwrap();
        let store = AntigravityPaths::offline(root.clone());

        let db_id = "22222222-3333-4444-8555-666666666666";
        let db_path = conv_dir.join(format!("{db_id}.db"));
        // Use DELETE journal mode so BEGIN EXCLUSIVE blocks readers with SQLITE_BUSY
        // and ROLLBACK leaves the file's FileStamp 100% identical!
        let writer = Connection::open(&db_path).unwrap();
        writer
            .execute_batch(
                "PRAGMA journal_mode = DELETE;
                 CREATE TABLE steps (idx INTEGER PRIMARY KEY, step_type INTEGER, status INTEGER, step_payload BLOB);",
            )
            .unwrap();
        let user_blob = pb_bytes(19, &pb_string(1, "锁释放后不能漏发通知"));
        let planner_blob = pb_bytes(20, &pb_string(1, "锁释放后的最终完成答复。"));
        writer
            .execute(
                "INSERT INTO steps (idx, step_type, status, step_payload) VALUES (0, 14, 3, ?1)",
                params![user_blob],
            )
            .unwrap();
        writer
            .execute(
                "INSERT INTO steps (idx, step_type, status, step_payload) VALUES (1, 15, 3, ?1)",
                params![planner_blob],
            )
            .unwrap();

        let sum_conn = Connection::open(store.summaries_db()).unwrap();
        sum_conn
            .execute_batch(
                "CREATE TABLE conversation_summaries (
                    conversation_id TEXT PRIMARY KEY,
                    title TEXT,
                    preview TEXT,
                    step_count INTEGER,
                    last_modified_time TEXT,
                    workspace_uris TEXT,
                    parent_conversation_id TEXT,
                    nesting_depth INTEGER
                );",
            )
            .unwrap();
        sum_conn
            .execute(
                "INSERT INTO conversation_summaries VALUES (?1, 'Locked DB Test', 'p', 2, '2026-11-26T01:00:00Z', '', '', 0)",
                params![db_id],
            )
            .unwrap();
        drop(sum_conn);

        writer.execute_batch("BEGIN EXCLUSIVE;").unwrap();
        let stamp_locked = FileStamp::sqlite_pair(&db_path).unwrap();
        let scan_while_locked = scan_offline_store(&store, Some(db_id)).unwrap();
        assert!(scan_while_locked.streams.is_empty());
        assert_eq!(
            scan_while_locked.errors, 1,
            "transient SQLite lock must record scan error instead of silently succeeding as empty"
        );

        writer.execute_batch("ROLLBACK;").unwrap();
        let stamp_unlocked = FileStamp::sqlite_pair(&db_path).unwrap();
        assert_eq!(
            stamp_locked, stamp_unlocked,
            "FileStamp must remain unchanged across ROLLBACK"
        );

        let scan_after_unlock = scan_offline_store(&store, Some(db_id)).unwrap();
        assert_eq!(scan_after_unlock.errors, 0);
        assert_eq!(scan_after_unlock.streams.len(), 1);
        assert_eq!(scan_after_unlock.streams[0].events.len(), 1);
        assert_eq!(scan_after_unlock.streams[0].events[0].kind, "completion");
        assert!(scan_after_unlock.streams[0].events[0]
            .body
            .contains("锁释放后的最终完成答复"));

        // Also verify a failed `.db` session (where `CONV_DB_STEPS_CACHE` & `CONV_DB_META_CACHE`
        // derive `summary.status == "failed"`) is not poisoned during a transient exclusive lock.
        let err_id = "33333333-4444-4555-8666-777777777777";
        let err_db_path = conv_dir.join(format!("{err_id}.db"));
        let err_writer = Connection::open(&err_db_path).unwrap();
        err_writer
            .execute_batch(
                "PRAGMA journal_mode = DELETE;
                 CREATE TABLE steps (idx INTEGER PRIMARY KEY, step_type INTEGER, status INTEGER, step_payload BLOB);",
            )
            .unwrap();
        err_writer
            .execute(
                "INSERT INTO steps (idx, step_type, status, step_payload) VALUES (0, 14, 3, ?1)",
                params![user_blob],
            )
            .unwrap();
        let err_blob = pb_bytes(24, &pb_bytes(3, &pb_string(2, "锁释放后可见的执行失败")));
        err_writer
            .execute(
                "INSERT INTO steps (idx, step_type, status, step_payload) VALUES (1, 17, 7, ?1)",
                params![err_blob],
            )
            .unwrap();
        Connection::open(store.summaries_db())
            .unwrap()
            .execute(
                "INSERT INTO conversation_summaries VALUES (?1, 'Locked Fail Test', 'p', 2, '2026-11-26T01:05:00Z', '', '', 0)",
                params![err_id],
            )
            .unwrap();

        err_writer.execute_batch("BEGIN EXCLUSIVE;").unwrap();
        let err_stamp_locked = FileStamp::sqlite_pair(&err_db_path).unwrap();
        let err_scan_locked = scan_offline_store(&store, Some(err_id)).unwrap();
        assert!(err_scan_locked.streams.is_empty());
        assert_eq!(err_scan_locked.errors, 1);

        err_writer.execute_batch("ROLLBACK;").unwrap();
        assert_eq!(
            err_stamp_locked,
            FileStamp::sqlite_pair(&err_db_path).unwrap()
        );
        let err_scan_unlocked = scan_offline_store(&store, Some(err_id)).unwrap();
        assert_eq!(err_scan_unlocked.errors, 0);
        assert_eq!(err_scan_unlocked.streams.len(), 1);
        assert_eq!(err_scan_unlocked.streams[0].events.len(), 1);
        assert_eq!(err_scan_unlocked.streams[0].events[0].kind, "failure");
        assert!(err_scan_unlocked.streams[0].events[0]
            .body
            .contains("锁释放后可见的执行失败"));

        drop(err_writer);
        drop(writer);
        evict_idle_meta_cache(std::time::Duration::ZERO);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn store_aux_index_cache_skips_caching_on_transient_lock_and_recovers_fts_archived_state() {
        let empty_root =
            std::env::temp_dir().join(format!("agenthub-ag-aux-empty-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&empty_root).unwrap();
        let empty_store = AntigravityPaths::offline(empty_root.clone());
        let empty_db = Connection::open(empty_store.summaries_db()).unwrap();
        empty_db
            .execute_batch(
                "PRAGMA journal_mode = DELETE;
                 CREATE TABLE conversation_summaries (conversation_id TEXT PRIMARY KEY);",
            )
            .unwrap();
        assert!(load_fts_meta_rows(&empty_db).unwrap().is_empty());
        let empty_first = load_store_auxiliary_index(&empty_store, Some(&empty_db));
        let empty_second = load_store_auxiliary_index(&empty_store, Some(&empty_db));
        assert!(
            std::sync::Arc::ptr_eq(&empty_first, &empty_second),
            "genuinely empty FTS schema must still be cached"
        );
        drop(empty_db);
        let _ = fs::remove_dir_all(&empty_root);

        let root =
            std::env::temp_dir().join(format!("agenthub-ag-aux-lock-{}", uuid::Uuid::new_v4()));
        let conv_dir = root.join("conversations");
        let cache_dir = root.join("cache");
        fs::create_dir_all(&conv_dir).unwrap();
        fs::create_dir_all(&cache_dir).unwrap();
        let store = AntigravityPaths::offline(root.clone());

        let cid = "44444444-5555-4666-8777-888888888888";
        fs::write(conv_dir.join(format!("{cid}.pb")), b"primary-session-bytes").unwrap();

        let writer = Connection::open(store.summaries_db()).unwrap();
        writer
            .execute_batch(
                "PRAGMA journal_mode = DELETE;
                 CREATE TABLE conversation_summaries (
                     conversation_id TEXT PRIMARY KEY,
                     title TEXT,
                     preview TEXT,
                     step_count INTEGER,
                     last_modified_time TEXT,
                     workspace_uris TEXT,
                     parent_conversation_id TEXT,
                     nesting_depth INTEGER
                 );
                 CREATE VIRTUAL TABLE conversations_fts USING fts5(
                     conversation_id UNINDEXED,
                     chunk_index UNINDEXED,
                     archived UNINDEXED,
                     title,
                     workspace UNINDEXED,
                     step_count UNINDEXED,
                     last_modified_time UNINDEXED
                 );",
            )
            .unwrap();
        writer
            .execute(
                "INSERT INTO conversation_summaries VALUES (?1, 'FTS Archived CLI', 'p', 1, '2026-11-26T02:00:00Z', '[\"file:///workspace/example.invalid\"]', '', 0)",
                params![cid],
            )
            .unwrap();
        writer
            .execute(
                "INSERT INTO conversations_fts (conversation_id, chunk_index, archived, title, workspace, step_count, last_modified_time) \
                 VALUES (?1, -1, 1, 'FTS Archived CLI', 'file:///workspace/example.invalid', 1, 1795658400000000000)",
                params![cid],
            )
            .unwrap();

        let reader = Connection::open_with_flags(
            store.summaries_db(),
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .unwrap();
        reader.busy_timeout(std::time::Duration::ZERO).unwrap();

        writer.execute_batch("BEGIN EXCLUSIVE;").unwrap();
        let stamp_locked = FileStamp::sqlite_pair(&store.summaries_db()).unwrap();
        assert!(
            load_fts_meta_rows(&reader).is_err(),
            "transient SQLITE_BUSY must surface Err rather than empty BTreeMap"
        );
        let aux_while_locked = load_store_auxiliary_index(&store, Some(&reader));
        assert!(aux_while_locked.fts_rows.is_empty());

        writer.execute_batch("ROLLBACK;").unwrap();
        assert_eq!(
            stamp_locked,
            FileStamp::sqlite_pair(&store.summaries_db()).unwrap(),
            "ROLLBACK in DELETE journal mode preserves identical SqliteStampPair"
        );

        let aux_after_unlock = load_store_auxiliary_index(&store, Some(&reader));
        assert_eq!(
            aux_after_unlock.fts_rows.get(cid).and_then(|r| r.archived),
            Some(true),
            "STORE_AUX_INDEX_CACHE must not retain poisoned unarchived state after lock release"
        );
        let aux_cached = load_store_auxiliary_index(&store, Some(&reader));
        assert!(std::sync::Arc::ptr_eq(&aux_after_unlock, &aux_cached));

        let active_rows =
            list_summaries_from_source_filtered(&store, None, Some(false), None).unwrap();
        assert!(
            active_rows.iter().all(|s| s.id != cid),
            "FTS-archived session must not appear in unarchived list after unlock"
        );
        let archived_rows =
            list_summaries_from_source_filtered(&store, None, Some(true), None).unwrap();
        assert_eq!(archived_rows.len(), 1);
        assert_eq!(archived_rows[0].id, cid);
        assert!(archived_rows[0].archived);

        // Partial/corrupt conversation_metadata.json must not be cached either.
        let conv_meta_file = cache_dir.join("conversation_metadata.json");
        fs::write(&conv_meta_file, b"{\"conversations\":").unwrap();
        let partial_a = load_store_auxiliary_index(&store, Some(&reader));
        let partial_b = load_store_auxiliary_index(&store, Some(&reader));
        assert!(!std::sync::Arc::ptr_eq(&partial_a, &partial_b));

        drop(reader);
        drop(writer);
        evict_idle_meta_cache(std::time::Duration::ZERO);
        let _ = fs::remove_dir_all(root);
    }
}
