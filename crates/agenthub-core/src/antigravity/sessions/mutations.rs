use super::*;

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn open_summaries_rw(db_path: &Path) -> Result<Connection> {
    let conn = Connection::open(db_path).context("无法打开 Antigravity 索引数据库写入")?;
    let _ = conn.busy_timeout(Duration::from_millis(2500));
    Ok(conn)
}

fn patch_db_raw_summary(
    conn: &Connection,
    id: &str,
    new_title: Option<&str>,
    new_archived: Option<bool>,
) -> Result<()> {
    let has_raw = super::summaries_table_columns(conn).contains("raw_summary");
    if !has_raw {
        return Ok(());
    }
    let Ok(mut stmt) =
        conn.prepare("SELECT raw_summary FROM conversation_summaries WHERE conversation_id = ?1")
    else {
        return Ok(());
    };
    let Ok(Some(blob)) = stmt.query_row(params![id], |row| row.get::<_, Option<Vec<u8>>>(0)) else {
        return Ok(());
    };
    if let Some(patched) =
        patch_raw_summary_annotations(&blob, new_title, new_archived, now_unix_secs())
    {
        conn.execute(
            "UPDATE conversation_summaries SET raw_summary = ?1 WHERE conversation_id = ?2",
            params![patched, id],
        )
        .context("更新 Antigravity raw_summary 失败")?;
    }
    Ok(())
}

fn upsert_fts_summary_row(
    conn: &Connection,
    id: &str,
    existing: &ProviderSessionSummary,
    new_title: Option<&str>,
    new_archived: Option<bool>,
) -> Result<()> {
    let cols = crate::db::table_columns(conn, "conversations_fts").unwrap_or_default();
    if !cols.contains("conversation_id") || !cols.contains("chunk_index") {
        return Ok(());
    }
    let effective_title = new_title.unwrap_or(&existing.title);
    let effective_archived = i64::from(new_archived.unwrap_or(existing.archived));
    let updated = match (new_title, new_archived) {
        (Some(_), Some(_)) if cols.contains("title") && cols.contains("archived") => conn.execute(
            "UPDATE conversations_fts SET title = ?1, archived = ?2 WHERE conversation_id = ?3 AND chunk_index = -1",
            params![effective_title, effective_archived, id],
        )?,
        (Some(_), None) if cols.contains("title") => conn.execute(
            "UPDATE conversations_fts SET title = ?1 WHERE conversation_id = ?2 AND chunk_index = -1",
            params![effective_title, id],
        )?,
        (None, Some(_)) if cols.contains("archived") => conn.execute(
            "UPDATE conversations_fts SET archived = ?1 WHERE conversation_id = ?2 AND chunk_index = -1",
            params![effective_archived, id],
        )?,
        _ => return Ok(()),
    };
    if updated > 0 {
        return Ok(());
    }

    let now_nanos = existing
        .updated_at
        .as_deref()
        .and_then(super::cli_meta::parse_valid_timestamp_ms)
        .unwrap_or_else(super::now_epoch_ms)
        .saturating_mul(1_000_000);
    let step_count = (existing.message_count as i64).max(1);
    let project = Path::new(&existing.cwd)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    let full_15_cols = [
        "chunk_first_step",
        "chunk_last_step",
        "last_modified_time",
        "step_count",
        "last_step_created_at",
        "archived",
        "user_step_byte_offsets",
        "agent_step_byte_offsets",
        "title",
        "workspace",
        "project",
        "user_body",
        "agent_body",
    ]
    .iter()
    .all(|c| cols.contains(*c));
    if full_15_cols {
        conn.execute(
            "INSERT INTO conversations_fts (
                conversation_id, chunk_index, chunk_first_step, chunk_last_step,
                last_modified_time, step_count, last_step_created_at, archived,
                user_step_byte_offsets, agent_step_byte_offsets,
                title, workspace, project, user_body, agent_body
            ) VALUES (?1, -1, 0, ?2, ?3, ?4, ?3, ?5, '[]', '[]', ?6, ?7, ?8, '', '')",
            params![
                id,
                step_count.saturating_sub(1),
                now_nanos,
                step_count,
                effective_archived,
                effective_title,
                existing.cwd,
                project,
            ],
        )
        .context("写入 Antigravity conversations_fts 索引行失败")?;
    } else if cols.contains("archived") && cols.contains("title") {
        conn.execute(
            "INSERT INTO conversations_fts (conversation_id, chunk_index, archived, title) VALUES (?1, -1, ?2, ?3)",
            params![id, effective_archived, effective_title],
        )
        .context("写入 Antigravity conversations_fts 简版索引行失败")?;
    }
    Ok(())
}

pub(crate) fn resolve_target_store_and_endpoint(
    paths: &AntigravityPaths,
    key: &str,
) -> Result<(AntigravityPaths, Option<(LiveRpc, Value)>)> {
    if let Some(cli) = paths.cli_store_for_id(key) {
        return Ok((cli, None));
    }
    let primary = paths.primary_store();
    let ep = connect_endpoint(&primary)?;
    let store = paths.store_for_id(key, ep.as_ref().map(|(_, v)| v));
    if store.home == primary.home {
        Ok((primary, ep))
    } else {
        Ok((store, None))
    }
}

fn ensure_offline_unlocked(store: &AntigravityPaths, root_id: &str, verb: &str) -> Result<()> {
    ensure!(
        !probe_presence_locked_shared(store, root_id).context("无法探测 Antigravity 会话锁")?,
        "会话当前已在 Antigravity 中打开，离线模式下无法安全{verb}"
    );
    for child in find_descendant_ids_with_live(store, None, root_id)? {
        ensure!(
            !probe_presence_locked_shared(store, &child).context("无法探测 Antigravity 会话锁")?,
            "子会话当前已在 Antigravity 中打开，离线模式下无法安全{verb}"
        );
    }
    Ok(())
}

pub fn rename_session(
    paths: &AntigravityPaths,
    key: &str,
    raw_title: &str,
) -> Result<ProviderSessionSummary> {
    valid_cascade_id(key)?;
    let title: String = raw_title
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(MAX_TITLE_CHARS)
        .collect();
    ensure!(!title.is_empty(), "会话名称不能为空");
    let existing = session_summary_without_storage(paths, key)?;
    let (store, endpoint) = resolve_target_store_and_endpoint(paths, key)?;

    if let Some((rpc, _)) = endpoint {
        connect_rpc_post(
            rpc.port,
            &rpc.token,
            "UpdateConversationAnnotations",
            &json!({
                "cascadeIds": [key],
                "annotations": { "title": title },
                "mergeAnnotations": true
            }),
        )?;
    } else {
        ensure!(
            !is_session_busy_status(&existing.status),
            "{}",
            existing
                .rename_block_reason
                .as_deref()
                .unwrap_or("会话正在运行，请先在 Antigravity 中停止后再改名")
        );
        ensure_offline_unlocked(&store, key, "改名")?;
        write_annotation_overlay(&store, key, Some(&title), None)?;
        let db_path = store.summaries_db();
        if db_path.is_file() {
            let conn = open_summaries_rw(&db_path)?;
            conn.execute(
                "UPDATE conversation_summaries SET title = ?1 WHERE conversation_id = ?2",
                params![title, key],
            )
            .context("更新 Antigravity 会话标题索引失败")?;
            patch_db_raw_summary(&conn, key, Some(&title), None)?;
            upsert_fts_summary_row(&conn, key, &existing, Some(&title), None)?;
        }
        let metadata_path = store.home.join("cache/conversation_metadata.json");
        if session_delete::within(&store.home, &metadata_path) {
            session_delete::edit_json(&metadata_path, |doc| {
                if let Some(entry) = doc
                    .get_mut("conversations")
                    .and_then(Value::as_object_mut)
                    .and_then(|map| map.get_mut(key))
                    .and_then(Value::as_object_mut)
                {
                    if let Some(summary_obj) =
                        entry.get_mut("summary").and_then(Value::as_object_mut)
                    {
                        summary_obj.insert("Title".into(), Value::String(title.clone()));
                    } else if entry.get("summary").is_some_and(Value::is_string) {
                        entry.insert("summary".into(), Value::String(title.clone()));
                    }
                }
            })?;
        }
        invalidate_proto_cache(&store);
    }
    session_summary(paths, key)
}

pub fn set_archived(
    paths: &AntigravityPaths,
    key: &str,
    archived: bool,
) -> Result<ProviderSessionSummary> {
    valid_cascade_id(key)?;
    let existing = session_summary_without_storage(paths, key)?;
    let (store, endpoint) = resolve_target_store_and_endpoint(paths, key)?;

    if let Some((rpc, _)) = endpoint {
        connect_rpc_post(
            rpc.port,
            &rpc.token,
            "UpdateConversationAnnotations",
            &json!({
                "cascadeIds": [key],
                "annotations": { "archived": archived },
                "mergeAnnotations": true
            }),
        )?;
        if archived {
            let _ = connect_rpc_post(
                rpc.port,
                &rpc.token,
                "ForceStopCascadeTree",
                &json!({ "conversationId": key }),
            );
        }
    } else {
        let verb = if archived { "归档" } else { "恢复" };
        ensure!(
            !is_session_busy_status(&existing.status),
            "{}",
            existing.archive_block_reason.as_deref().unwrap_or(&format!(
                "会话正在运行，请先在 Antigravity 中停止后再{verb}"
            ))
        );
        ensure_offline_unlocked(&store, key, verb)?;
        write_annotation_overlay(&store, key, None, Some(archived))?;
        let db_path = store.summaries_db();
        if db_path.is_file() {
            let conn = open_summaries_rw(&db_path)?;
            patch_db_raw_summary(&conn, key, None, Some(archived))?;
            upsert_fts_summary_row(&conn, key, &existing, None, Some(archived))?;
        }
        invalidate_proto_cache(&store);
    }
    session_summary(paths, key)
}

pub(crate) fn probe_presence_locked_shared(
    paths: &AntigravityPaths,
    id: &str,
) -> std::io::Result<bool> {
    crate::file_stamp::probe_advisory_lock(
        &paths.presence_dir().join(format!("{id}.lock")),
        libc::LOCK_SH,
    )
}

fn live_item_has_waiting_step(item: &Value) -> bool {
    item["status"].as_str() == Some("CASCADE_RUN_STATUS_WAITING")
        || item["waitingSteps"]
            .as_array()
            .is_some_and(|arr| !arr.is_empty())
}

pub fn live_item_is_actively_executing(item: &Value) -> bool {
    let run_status = item["status"].as_str().unwrap_or("");
    matches!(
        run_status,
        "CASCADE_RUN_STATUS_RUNNING" | "CASCADE_RUN_STATUS_BUSY" | "CASCADE_RUN_STATUS_CANCELING"
    ) || item["hasRunningSubagents"].as_bool() == Some(true)
        || (item["notFullyIdle"].as_bool() == Some(true) && !live_item_has_waiting_step(item))
}

pub fn live_item_is_waiting(item: &Value) -> bool {
    !live_item_is_actively_executing(item) && live_item_has_waiting_step(item)
}

pub fn live_item_is_running(item: &Value) -> bool {
    live_item_is_actively_executing(item) || live_item_is_waiting(item)
}

pub fn live_item_is_failed(item: &Value) -> bool {
    if live_item_is_running(item) {
        return false;
    }
    if item["status"].as_str() == Some("CASCADE_RUN_STATUS_ERROR") {
        return true;
    }
    let reason = item["latestExecutorTerminationReason"]
        .as_str()
        .or_else(|| item["trajectoryMetadata"]["latestExecutorTerminationReason"].as_str())
        .unwrap_or("");
    matches!(
        reason,
        "EXECUTOR_TERMINATION_REASON_ERROR"
            | "EXECUTOR_TERMINATION_REASON_MAX_INVOCATIONS"
            | "EXECUTOR_TERMINATION_REASON_MAX_FORCED_INVOCATIONS"
            | "EXECUTOR_TERMINATION_REASON_MAX_TOKEN_BUDGET_EXCEEDED"
    )
}

fn is_cascade_running(all_live: Option<&Value>, id: &str) -> bool {
    all_live
        .and_then(|v| v["trajectorySummaries"].get(id))
        .is_some_and(live_item_is_running)
}

fn expand_transcript_children_for_roots(
    paths: &AntigravityPaths,
    children_map: &mut BTreeMap<String, Vec<String>>,
    roots: &[String],
) {
    let mut visited = BTreeSet::new();
    let mut queue: VecDeque<(String, usize)> = roots.iter().cloned().map(|r| (r, 0)).collect();
    while let Some((curr, depth)) = queue.pop_front() {
        if depth > 32 || !visited.insert(curr.clone()) {
            continue;
        }
        for spawned in extract_transcript_spawned_subagents(paths, &curr) {
            let entry = children_map.entry(curr.clone()).or_default();
            if !entry.contains(&spawned) {
                entry.push(spawned);
            }
        }
        if let Some(kids) = children_map.get(&curr).cloned() {
            for kid in kids {
                queue.push_back((kid, depth + 1));
            }
        }
    }
}

fn find_descendant_ids_with_live(
    paths: &AntigravityPaths,
    all_live: Option<&Value>,
    root_id: &str,
) -> Result<Vec<String>> {
    let mut children_map = read_children_map(paths, all_live)?;
    expand_transcript_children_for_roots(paths, &mut children_map, &[root_id.to_owned()]);
    Ok(descendant_ids(&children_map, root_id))
}

pub(crate) fn read_children_map(
    paths: &AntigravityPaths,
    all_live: Option<&Value>,
) -> Result<BTreeMap<String, Vec<String>>> {
    let mut children_map: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut insert_edge = |parent: String, child: String| {
        if !parent.is_empty()
            && parent != child
            && uuid::Uuid::parse_str(&parent).is_ok()
            && uuid::Uuid::parse_str(&child).is_ok()
        {
            let list = children_map.entry(parent).or_default();
            if !list.contains(&child) {
                list.push(child);
            }
        }
    };
    let db_path = paths.summaries_db();
    let mut open_conn: Option<Connection> = None;
    if db_path.is_file() {
        let conn = Connection::open_with_flags(
            &db_path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .context("无法打开 Antigravity 索引数据库校验子会话")?;
        let cols = super::summaries_table_columns(&conn);
        let raw_expr = if cols.contains("raw_summary") {
            "raw_summary"
        } else {
            "NULL"
        };
        {
            let mut stmt = conn
                .prepare(&format!(
                    "SELECT conversation_id, COALESCE(parent_conversation_id, ''), {raw_expr} FROM conversation_summaries"
                ))
                .context("无法读取 Antigravity 索引数据库子会话表")?;
            let rows = stmt
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<Vec<u8>>>(2).ok().flatten(),
                    ))
                })
                .context("无法查询 Antigravity 子会话关系")?;
            for row in rows {
                let (cid, pid, raw_blob) = row.context("解码 Antigravity 子会话索引行失败")?;
                if !pid.is_empty() {
                    insert_edge(pid, cid);
                } else if let Some(blob) = raw_blob {
                    let parsed = super::annotations::parse_raw_summary(&blob);
                    if let Some(eff_p) = parsed.trajectory_meta.effective_parent(&cid) {
                        insert_edge(eff_p.to_owned(), cid);
                    }
                }
            }
        }
        if super::has_conversation_parents_table(&conn) {
            let mut pstmt = conn
                .prepare(
                    "SELECT conversation_id, COALESCE(parent_id, '') FROM conversation_parents",
                )
                .context("无法读取 Antigravity conversation_parents 子会话表")?;
            let prows = pstmt
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .context("无法查询 Antigravity conversation_parents 关系")?;
            for row in prows {
                let (cid, pid) = row.context("解码 Antigravity conversation_parents 行失败")?;
                insert_edge(pid, cid);
            }
        }
        open_conn = Some(conn);
    }
    let aux = super::load_store_auxiliary_index(paths, open_conn.as_ref());
    for (pid, kids) in &aux.sidecar_children {
        for cid in kids {
            insert_edge(pid.clone(), cid.clone());
        }
    }
    for (cid, entry) in &aux.conv_meta {
        if let Some(pid) = entry.parent_id.as_ref().or(entry.root_id.as_ref()) {
            insert_edge(pid.clone(), cid.clone());
        }
    }
    if paths.conversations_dir().is_dir() {
        if let Ok(entries) = fs::read_dir(paths.conversations_dir()) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.extension().and_then(|s| s.to_str()) != Some("db") {
                    continue;
                }
                let Some(cid) = p.file_stem().and_then(|s| s.to_str()) else {
                    continue;
                };
                if uuid::Uuid::parse_str(cid).is_ok() {
                    if let Some(meta) = super::inspect_conv_db_metadata(paths, cid) {
                        if !meta.parent_ids.is_empty() {
                            for pid in meta.parent_ids {
                                insert_edge(pid, cid.to_owned());
                            }
                        } else if let Some(rid) = meta.root_id {
                            insert_edge(rid, cid.to_owned());
                        }
                    }
                }
            }
        }
    }
    if let Some(map) = all_live.and_then(|v| v["trajectorySummaries"].as_object()) {
        for (cid, item) in map {
            if uuid::Uuid::parse_str(cid).is_err() {
                continue;
            }
            let parent = item["parentConversationId"]
                .as_str()
                .or_else(|| item["trajectoryMetadata"]["parentConversationId"].as_str())
                .unwrap_or("");
            if !parent.is_empty() {
                insert_edge(parent.to_owned(), cid.clone());
            } else {
                let root = item["rootConversationId"]
                    .as_str()
                    .or_else(|| item["trajectoryMetadata"]["rootConversationId"].as_str())
                    .unwrap_or("");
                if !root.is_empty() && root != cid {
                    insert_edge(root.to_owned(), cid.clone());
                }
            }
        }
    }
    Ok(children_map)
}

/// Returns descendants in topological parent-before-child order so `.rev()` deletes leaf-first.
pub(crate) fn descendant_ids(
    children_map: &BTreeMap<String, Vec<String>>,
    root_id: &str,
) -> Vec<String> {
    fn dfs(
        curr: &str,
        root_id: &str,
        children_map: &BTreeMap<String, Vec<String>>,
        visiting: &mut BTreeSet<String>,
        visited: &mut BTreeSet<String>,
        post_order: &mut Vec<String>,
    ) {
        if !visiting.insert(curr.to_owned()) {
            return;
        }
        if let Some(kids) = children_map.get(curr) {
            let mut sorted_kids = kids.clone();
            sorted_kids.sort();
            for kid in sorted_kids {
                if kid == root_id || visited.contains(&kid) {
                    continue;
                }
                dfs(&kid, root_id, children_map, visiting, visited, post_order);
            }
        }
        visiting.remove(curr);
        if curr != root_id && visited.insert(curr.to_owned()) {
            post_order.push(curr.to_owned());
        }
    }

    let mut visiting = BTreeSet::new();
    let mut visited = BTreeSet::new();
    let mut post_order = Vec::new();
    dfs(
        root_id,
        root_id,
        children_map,
        &mut visiting,
        &mut visited,
        &mut post_order,
    );
    post_order.reverse();
    post_order
}

pub(super) fn owned_disk_paths(paths: &AntigravityPaths, id: &str) -> Vec<PathBuf> {
    let candidates = [
        paths.conversations_dir().join(format!("{id}.db")),
        paths.conversations_dir().join(format!("{id}.db-wal")),
        paths.conversations_dir().join(format!("{id}.db-shm")),
        paths.conversations_dir().join(format!("{id}.pb")),
        paths.annotations_dir().join(format!("{id}.pbtxt")),
        paths.brain_dir().join(id),
        paths.presence_dir().join(format!("{id}.lock")),
    ];
    candidates
        .into_iter()
        .filter(|path| path.exists() && session_delete::within(&paths.home, path))
        .collect()
}

fn has_conversations_fts_table(conn: &Connection) -> bool {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'conversations_fts')",
        [],
        |r| r.get::<_, bool>(0),
    )
    .unwrap_or(false)
}

fn record_remaining_antigravity_residuals(
    paths: &AntigravityPaths,
    remaining_ids: &[String],
    outcome: &mut Outcome,
) {
    for rem_id in remaining_ids {
        for path in owned_disk_paths(paths, rem_id) {
            outcome.residuals.push(path.display().to_string());
        }
    }
}

#[cfg(test)]
pub(crate) fn preview_batch_item(
    paths: &AntigravityPaths,
    key: &str,
    operation: SessionOperation,
    include_descendants: bool,
) -> Result<SessionBatchItem> {
    preview_batch(paths, &[key.to_owned()], operation, include_descendants).remove(0)
}

fn clean_store_shared_files(
    store: &AntigravityPaths,
    deleted_ids: &[String],
    outcome: &mut Outcome,
) {
    let deleted_set: BTreeSet<&str> = deleted_ids.iter().map(String::as_str).collect();
    let history_path = store.home.join("history.jsonl");
    if session_delete::within(&store.home, &history_path) {
        outcome.attempt(&history_path, || {
            session_delete::filter_jsonl(&history_path, |row| {
                row.get("conversationId")
                    .or_else(|| row.get("conversation_id"))
                    .and_then(Value::as_str)
                    .is_some_and(|cid| deleted_set.contains(cid))
            })
        });
    }
    let metadata_path = store.home.join("cache/conversation_metadata.json");
    if session_delete::within(&store.home, &metadata_path) {
        outcome.attempt(&metadata_path, || {
            session_delete::edit_json(&metadata_path, |doc| {
                if let Some(convs) = doc.get_mut("conversations").and_then(Value::as_object_mut) {
                    for id in &deleted_set {
                        convs.remove(*id);
                    }
                }
            })
        });
    }
    let last_conv_path = store.home.join("cache/last_conversations.json");
    if session_delete::within(&store.home, &last_conv_path) {
        outcome.attempt(&last_conv_path, || {
            session_delete::edit_json(&last_conv_path, |doc| {
                if let Some(map) = doc.as_object_mut() {
                    map.retain(|_, val| !val.as_str().is_some_and(|cid| deleted_set.contains(cid)));
                }
            })
        });
    }
}

fn clean_other_store_mirror_rows(other: &AntigravityPaths, deleted_ids: &[String]) {
    let eligible: Vec<String> = deleted_ids
        .iter()
        .filter(|id| {
            !has_primary_session_storage(other, id)
                && !probe_presence_locked_shared(other, id).unwrap_or(true)
        })
        .cloned()
        .collect();
    if eligible.is_empty() {
        return;
    }
    let mut mirror_outcome = Outcome::default();
    clean_store_shared_files(other, &eligible, &mut mirror_outcome);

    let db_path = other.summaries_db();
    if !db_path.is_file() {
        return;
    }
    let Ok(mut conn) = Connection::open(&db_path) else {
        return;
    };
    let _ = conn.busy_timeout(Duration::from_millis(1000));
    let has_summaries = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'conversation_summaries')",
            [],
            |r| r.get::<_, bool>(0),
        )
        .unwrap_or(false);
    if !has_summaries {
        return;
    }
    let has_fts = has_conversations_fts_table(&conn);
    let has_parents = super::has_conversation_parents_table(&conn);
    let mut touched = false;
    for id in &eligible {
        if delete_session_rows_in_tx(&mut conn, id, has_fts, has_parents).is_ok() {
            touched = true;
        }
    }
    if touched {
        invalidate_proto_cache(other);
    }
}

pub fn preview_batch(
    paths: &AntigravityPaths,
    keys: &[String],
    operation: SessionOperation,
    include_descendants: bool,
) -> Vec<Result<SessionBatchItem>> {
    let primary = paths.primary_store();
    let cli = paths.cli_store();
    let context = (|| -> Result<_> {
        let live = connect_endpoint(&primary)?.map(|(_, payload)| payload);
        let summaries = list_summaries_from_source(paths, None, live.as_ref())?
            .into_iter()
            .map(|summary| (summary.id.clone(), summary))
            .collect::<BTreeMap<_, _>>();
        let mut primary_children = if operation == SessionOperation::Delete || live.is_none() {
            read_children_map(&primary, live.as_ref())?
        } else {
            BTreeMap::new()
        };
        let mut cli_children = match &cli {
            Some(c) if c.home.is_dir() => read_children_map(c, None)?,
            _ => BTreeMap::new(),
        };
        if operation == SessionOperation::Delete || live.is_none() {
            let valid_keys: Vec<String> = keys
                .iter()
                .filter(|k| uuid::Uuid::parse_str(k).is_ok())
                .cloned()
                .collect();
            expand_transcript_children_for_roots(&primary, &mut primary_children, &valid_keys);
            if let Some(c) = &cli {
                expand_transcript_children_for_roots(c, &mut cli_children, &valid_keys);
            }
        }
        Ok((live, summaries, primary_children, cli_children))
    })();
    keys.iter()
        .map(|key| {
            valid_cascade_id(key)?;
            let (live, summaries, primary_children, cli_children) = context
                .as_ref()
                .map_err(|error| anyhow::anyhow!("{error:#}"))?;
            let summary = summaries
                .get(key)
                .with_context(|| format!("未找到 Antigravity 会话：{key}"))?
                .clone();
            let store = paths.store_for_id(key, live.as_ref());
            if cli.as_ref().is_some_and(|c| c.home == store.home) {
                preview_from_summary(
                    &store,
                    summary,
                    None,
                    cli_children,
                    operation,
                    include_descendants,
                )
            } else {
                preview_from_summary(
                    &primary,
                    summary,
                    live.as_ref(),
                    primary_children,
                    operation,
                    include_descendants,
                )
            }
        })
        .collect()
}

fn preview_from_summary(
    paths: &AntigravityPaths,
    summary: ProviderSessionSummary,
    all_live: Option<&Value>,
    children: &BTreeMap<String, Vec<String>>,
    operation: SessionOperation,
    include_descendants: bool,
) -> Result<SessionBatchItem> {
    if operation == SessionOperation::Delete
        && (is_session_busy_status(&summary.status) || is_cascade_running(all_live, &summary.id))
    {
        bail!(
            "{}",
            summary
                .delete_block_reason
                .as_deref()
                .unwrap_or("会话正在运行，无法删除")
        );
    }
    if all_live.is_none() && operation != SessionOperation::Delete {
        let verb = if operation == SessionOperation::Archive {
            "归档"
        } else {
            "恢复"
        };
        if is_session_busy_status(&summary.status) {
            bail!(
                "{}",
                summary.archive_block_reason.as_deref().unwrap_or(&format!(
                    "会话正在运行，请先在 Antigravity 中停止后再{verb}"
                ))
            );
        }
        ensure!(
            !probe_presence_locked_shared(paths, &summary.id)
                .context("无法探测 Antigravity 会话锁")?,
            "会话当前已在 Antigravity 中打开，离线模式下无法安全{verb}"
        );
        for child in descendant_ids(children, &summary.id) {
            ensure!(
                !probe_presence_locked_shared(paths, &child)
                    .context("无法探测 Antigravity 会话锁")?,
                "子会话当前已在 Antigravity 中打开，离线模式下无法安全{verb}"
            );
        }
    }
    if operation == SessionOperation::Delete
        && all_live.is_none()
        && probe_presence_locked_shared(paths, &summary.id)
            .context("无法探测 Antigravity 会话锁")?
    {
        bail!("会话当前已在 Antigravity 中打开，离线模式下无法安全删除");
    }
    let all_descendants = if operation == SessionOperation::Delete {
        descendant_ids(children, &summary.id)
    } else {
        Vec::new()
    };
    if operation == SessionOperation::Delete {
        for child in &all_descendants {
            ensure!(
                !is_cascade_running(all_live, child),
                "子会话正在运行，请先在 Antigravity 中停止后再删除"
            );
            if all_live.is_none() {
                ensure!(
                    !probe_presence_locked_shared(paths, child)
                        .context("无法探测 Antigravity 会话锁")?,
                    "子会话当前已在 Antigravity 中打开，离线模式下无法安全删除"
                );
            }
        }
    }
    let descendants = if operation == SessionOperation::Delete && include_descendants {
        all_descendants
    } else {
        Vec::new()
    };
    let mut target_paths = owned_disk_paths(paths, &summary.id);
    for child in &descendants {
        target_paths.extend(owned_disk_paths(paths, child));
    }
    if target_paths.is_empty() {
        target_paths.push(summary.path.clone());
    }
    let (_, bytes) = session_delete::size(&target_paths);
    let fingerprint = hex::encode(Sha256::digest(format!(
        "antigravity:{}:{}:{}:{}",
        summary.id,
        summary.archived,
        bytes,
        summary.updated_at.as_deref().unwrap_or("")
    )));
    Ok(SessionBatchItem {
        session_key: summary.session_key,
        id: summary.id,
        title: summary.title,
        paths: target_paths,
        bytes,
        allowed: true,
        reason: None,
        fingerprint: Some(fingerprint),
        descendants,
        warnings: Vec::new(),
    })
}

fn delete_session_rows_in_tx(
    conn: &mut Connection,
    id: &str,
    has_fts: bool,
    has_parents: bool,
) -> rusqlite::Result<()> {
    let tx = conn.transaction()?;
    tx.execute(
        "DELETE FROM conversation_summaries WHERE conversation_id = ?1",
        params![id],
    )?;
    if has_fts {
        tx.execute(
            "DELETE FROM conversations_fts WHERE conversation_id = ?1",
            params![id],
        )?;
    }
    if has_parents {
        tx.execute(
            "DELETE FROM conversation_parents WHERE conversation_id = ?1",
            params![id],
        )?;
    }
    tx.commit()
}

fn store_has_residual_session_data(store: &AntigravityPaths, id: &str) -> bool {
    if !owned_disk_paths(store, id).is_empty() {
        return true;
    }
    let db_path = store.summaries_db();
    if db_path.is_file() {
        if let Ok(conn) = Connection::open_with_flags(
            &db_path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        ) {
            let in_summaries = conn
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM conversation_summaries WHERE conversation_id = ?1)",
                    params![id],
                    |r| r.get::<_, bool>(0),
                )
                .unwrap_or(false);
            if in_summaries {
                return true;
            }
            if has_conversations_fts_table(&conn)
                && conn
                    .query_row(
                        "SELECT EXISTS(SELECT 1 FROM conversations_fts WHERE conversation_id = ?1)",
                        params![id],
                        |r| r.get::<_, bool>(0),
                    )
                    .unwrap_or(false)
            {
                return true;
            }
            if super::has_conversation_parents_table(&conn)
                && conn
                    .query_row(
                        "SELECT EXISTS(SELECT 1 FROM conversation_parents WHERE conversation_id = ?1)",
                        params![id],
                        |r| r.get::<_, bool>(0),
                    )
                    .unwrap_or(false)
            {
                return true;
            }
        }
    }
    store_has_shared_session_entry(store, id)
}

fn store_has_shared_session_entry(store: &AntigravityPaths, id: &str) -> bool {
    let history_path = store.home.join("history.jsonl");
    if session_delete::within(&store.home, &history_path) && history_path.is_file() {
        if let Ok(file) = fs::File::open(&history_path) {
            let mut reader = BufReader::new(file);
            let mut line = String::new();
            while let Ok(read) = std::io::BufRead::read_line(&mut reader, &mut line) {
                if read == 0 {
                    break;
                }
                if line.contains(id) {
                    if let Ok(row) = serde_json::from_str::<Value>(&line) {
                        if row
                            .get("conversationId")
                            .or_else(|| row.get("conversation_id"))
                            .and_then(Value::as_str)
                            == Some(id)
                        {
                            return true;
                        }
                    }
                }
                line.clear();
            }
        }
    }

    let metadata_path = store.home.join("cache/conversation_metadata.json");
    if session_delete::within(&store.home, &metadata_path) && metadata_path.is_file() {
        if let Ok(text) = fs::read_to_string(&metadata_path) {
            if text.contains(id) {
                if let Ok(doc) = serde_json::from_str::<Value>(&text) {
                    if doc
                        .get("conversations")
                        .and_then(Value::as_object)
                        .is_some_and(|convs| convs.contains_key(id))
                    {
                        return true;
                    }
                }
            }
        }
    }

    let last_conv_path = store.home.join("cache/last_conversations.json");
    if session_delete::within(&store.home, &last_conv_path) && last_conv_path.is_file() {
        if let Ok(text) = fs::read_to_string(&last_conv_path) {
            if text.contains(id) {
                if let Ok(doc) = serde_json::from_str::<Value>(&text) {
                    if doc
                        .as_object()
                        .is_some_and(|map| map.values().any(|val| val.as_str() == Some(id)))
                    {
                        return true;
                    }
                }
            }
        }
    }
    false
}

fn store_has_unclaimed_summary_row(
    store: &AntigravityPaths,
    expected_root: &str,
    verified_set: &BTreeSet<&str>,
    candidate_child: &str,
) -> bool {
    let db_path = store.summaries_db();
    if !db_path.is_file() {
        return false;
    }
    let Ok(conn) = Connection::open_with_flags(
        &db_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) else {
        return false;
    };
    let cols = super::summaries_table_columns(&conn);
    if !cols.contains("conversation_id") {
        return false;
    }
    let parent_expr = if cols.contains("parent_conversation_id") {
        "COALESCE(parent_conversation_id, '')"
    } else {
        "''"
    };
    let sql =
        format!("SELECT {parent_expr} FROM conversation_summaries WHERE conversation_id = ?1");
    let Ok(parent) = conn.query_row(&sql, params![candidate_child], |r| r.get::<_, String>(0))
    else {
        return false;
    };
    let parent = parent.trim();
    parent != expected_root && !verified_set.contains(parent)
}

fn resolve_delete_store_and_endpoint(
    paths: &AntigravityPaths,
    key: &str,
) -> Result<(AntigravityPaths, Option<(LiveRpc, Value)>)> {
    if let Some(cli) = paths.cli_store_for_id(key) {
        return Ok((cli, None));
    }
    let primary = paths.primary_store();
    let ep = connect_endpoint(&primary)?;
    if ep
        .as_ref()
        .and_then(|(_, v)| v["trajectorySummaries"].get(key))
        .is_some()
        || has_primary_session_storage(&primary, key)
    {
        return Ok((primary, ep));
    }
    if let Some(cli) = paths.cli_store() {
        if store_has_shared_session_entry(&cli, key)
            || (store_has_residual_session_data(&cli, key)
                && !store_has_residual_session_data(&primary, key))
        {
            return Ok((cli, None));
        }
    }
    let store = paths.store_for_id(key, ep.as_ref().map(|(_, v)| v));
    if store.home == primary.home {
        Ok((primary, ep))
    } else {
        Ok((store, None))
    }
}

fn session_exists_or_claimed_by_other_root(
    paths: &AntigravityPaths,
    store: &AntigravityPaths,
    all_live: Option<&Value>,
    expected_root: &str,
    verified_set: &BTreeSet<&str>,
    candidate_child: &str,
) -> Result<bool> {
    if all_live
        .and_then(|v| v["trajectorySummaries"].get(candidate_child))
        .is_some()
    {
        return Ok(true);
    }
    let primary = paths.primary_store();
    if has_primary_session_storage(&primary, candidate_child)
        || paths
            .cli_store()
            .as_ref()
            .is_some_and(|cli| has_primary_session_storage(cli, candidate_child))
    {
        return Ok(true);
    }
    let children_map = read_children_map(store, all_live)?;
    if children_map.contains_key(candidate_child) {
        return Ok(true);
    }
    for (parent, kids) in &children_map {
        if parent != expected_root
            && !verified_set.contains(parent.as_str())
            && kids.iter().any(|k| k == candidate_child)
        {
            return Ok(true);
        }
    }
    if store_has_unclaimed_summary_row(store, expected_root, verified_set, candidate_child) {
        return Ok(true);
    }
    if let Some(other) = paths.other_store_for(&store.home) {
        if other.home.is_dir() {
            let other_children = read_children_map(&other, None)?;
            if other_children.contains_key(candidate_child) {
                return Ok(true);
            }
            for (parent, kids) in &other_children {
                if parent != expected_root
                    && !verified_set.contains(parent.as_str())
                    && kids.iter().any(|k| k == candidate_child)
                {
                    return Ok(true);
                }
            }
            if store_has_unclaimed_summary_row(&other, expected_root, verified_set, candidate_child)
            {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

fn delete_ordered_ids_in_store(
    paths: &AntigravityPaths,
    store: &AntigravityPaths,
    live_rpc: Option<&LiveRpc>,
    all_live: Option<&Value>,
    ordered_ids: &[String],
) -> Result<(Vec<String>, Vec<String>)> {
    let db_path = store.summaries_db();
    let mut outcome = Outcome::default();
    let mut offline_conn = if live_rpc.is_none() && db_path.is_file() {
        match Connection::open(&db_path) {
            Ok(conn) => {
                let _ = conn.busy_timeout(Duration::from_millis(1000));
                Some(conn)
            }
            Err(err) => {
                outcome
                    .residuals
                    .push(format!("{}: {err}", db_path.display()));
                record_remaining_antigravity_residuals(store, ordered_ids, &mut outcome);
                outcome.finish();
                return Ok((Vec::new(), outcome.residuals));
            }
        }
    } else {
        None
    };
    let has_parents_table = offline_conn
        .as_ref()
        .is_some_and(super::has_conversation_parents_table);
    let has_fts_table = offline_conn
        .as_ref()
        .is_some_and(has_conversations_fts_table);

    let mut deleted_ids = Vec::new();
    let mut shared_cleanup_ids = Vec::new();
    let mut unhydrated_db_cleanup_ids = Vec::new();
    for (idx, id) in ordered_ids.iter().enumerate() {
        if let Some(rpc) = live_rpc {
            let in_live = all_live
                .and_then(|v| v["trajectorySummaries"].get(id))
                .is_some();
            if !in_live {
                match probe_presence_locked_shared(store, id) {
                    Ok(false) => {}
                    Ok(true) => {
                        outcome
                            .residuals
                            .push(format!("{id}: 会话当前已被锁定，已停止删除"));
                        record_remaining_antigravity_residuals(
                            store,
                            &ordered_ids[idx..],
                            &mut outcome,
                        );
                        break;
                    }
                    Err(lock_err) => {
                        outcome
                            .residuals
                            .push(format!("{id}: 无法探测 Antigravity 会话锁：{lock_err}"));
                        record_remaining_antigravity_residuals(
                            store,
                            &ordered_ids[idx..],
                            &mut outcome,
                        );
                        break;
                    }
                }
            }
            match connect_rpc_post(
                rpc.port,
                &rpc.token,
                "DeleteCascadeTrajectory",
                &json!({ "cascadeId": id }),
            ) {
                Ok(_) => {
                    if !in_live {
                        match probe_presence_locked_shared(store, id) {
                            Ok(false) => unhydrated_db_cleanup_ids.push(id.clone()),
                            Ok(true) => {
                                outcome
                                    .residuals
                                    .push(format!("{id}: 会话当前已被锁定，已停止删除"));
                                record_remaining_antigravity_residuals(
                                    store,
                                    &ordered_ids[idx..],
                                    &mut outcome,
                                );
                                break;
                            }
                            Err(lock_err) => {
                                outcome
                                    .residuals
                                    .push(format!("{id}: 无法探测 Antigravity 会话锁：{lock_err}"));
                                record_remaining_antigravity_residuals(
                                    store,
                                    &ordered_ids[idx..],
                                    &mut outcome,
                                );
                                break;
                            }
                        }
                    }
                }
                Err(err) => {
                    let is_not_found = err
                        .downcast_ref::<ConnectRpcHttpError>()
                        .is_some_and(ConnectRpcHttpError::is_not_found);
                    if !in_live && is_not_found {
                        match probe_presence_locked_shared(store, id) {
                            Ok(false) => unhydrated_db_cleanup_ids.push(id.clone()),
                            Ok(true) => {
                                outcome.residuals.push(format!("{id}: {err}"));
                                record_remaining_antigravity_residuals(
                                    store,
                                    &ordered_ids[idx..],
                                    &mut outcome,
                                );
                                break;
                            }
                            Err(lock_err) => {
                                outcome.residuals.push(format!(
                                    "{id}: 无法探测 Antigravity 会话锁：{lock_err} ({err})"
                                ));
                                record_remaining_antigravity_residuals(
                                    store,
                                    &ordered_ids[idx..],
                                    &mut outcome,
                                );
                                break;
                            }
                        }
                    } else {
                        outcome.residuals.push(format!("{id}: {err}"));
                        record_remaining_antigravity_residuals(
                            store,
                            &ordered_ids[idx..],
                            &mut outcome,
                        );
                        break;
                    }
                }
            }
        }

        let mut disk_failed = false;
        for path in owned_disk_paths(store, id) {
            if !outcome.remove_strict(&path) {
                disk_failed = true;
                break;
            }
        }
        if disk_failed {
            record_remaining_antigravity_residuals(store, &ordered_ids[idx..], &mut outcome);
            break;
        }
        shared_cleanup_ids.push(id.clone());

        if let Some(conn) = offline_conn.as_mut() {
            if let Err(err) = delete_session_rows_in_tx(conn, id, has_fts_table, has_parents_table)
            {
                outcome
                    .residuals
                    .push(format!("{}: {err}", db_path.display()));
                record_remaining_antigravity_residuals(
                    store,
                    &ordered_ids[idx + 1..],
                    &mut outcome,
                );
                break;
            }
        }
        deleted_ids.push(id.clone());
    }

    if !unhydrated_db_cleanup_ids.is_empty() && db_path.is_file() {
        match Connection::open(&db_path) {
            Ok(mut conn) => {
                let _ = conn.busy_timeout(Duration::from_millis(1000));
                let has_parents = super::has_conversation_parents_table(&conn);
                let has_fts = has_conversations_fts_table(&conn);
                for cid in &unhydrated_db_cleanup_ids {
                    if deleted_ids.contains(cid) {
                        if let Err(err) =
                            delete_session_rows_in_tx(&mut conn, cid, has_fts, has_parents)
                        {
                            deleted_ids.retain(|existing| existing != cid);
                            outcome
                                .residuals
                                .push(format!("{}: {err}", db_path.display()));
                            break;
                        }
                    }
                }
            }
            Err(err) => {
                for cid in &unhydrated_db_cleanup_ids {
                    deleted_ids.retain(|existing| existing != cid);
                }
                outcome
                    .residuals
                    .push(format!("{}: {err}", db_path.display()));
            }
        }
    }

    if !shared_cleanup_ids.is_empty() {
        let residuals_before = outcome.residuals.len();
        clean_store_shared_files(store, &shared_cleanup_ids, &mut outcome);
        if outcome.residuals.len() > residuals_before {
            deleted_ids.clear();
        }
        invalidate_proto_cache(store);
        if let Some(other) = paths.other_store_for(&store.home) {
            clean_other_store_mirror_rows(&other, &shared_cleanup_ids);
        }
    }
    outcome.finish();
    Ok((deleted_ids, outcome.residuals))
}

pub fn execute_batch_item(
    paths: &AntigravityPaths,
    key: &str,
    operation: SessionOperation,
    descendants: &[String],
    include_descendants: bool,
) -> Result<SessionBatchItemResult> {
    valid_cascade_id(key)?;
    match operation {
        SessionOperation::Archive => {
            set_archived(paths, key, true)?;
            Ok(SessionBatchItemResult {
                session_key: key.to_owned(),
                status: "succeeded".into(),
                message: None,
                deleted_ids: Vec::new(),
                residuals: Vec::new(),
            })
        }
        SessionOperation::Restore => {
            set_archived(paths, key, false)?;
            Ok(SessionBatchItemResult {
                session_key: key.to_owned(),
                status: "succeeded".into(),
                message: None,
                deleted_ids: Vec::new(),
                residuals: Vec::new(),
            })
        }
        SessionOperation::Delete => {
            let (store, endpoint) = resolve_delete_store_and_endpoint(paths, key)?;
            let (live_rpc, all_live) = match endpoint {
                Some((rpc, live)) => (Some(rpc), Some(live)),
                None => (None, None),
            };
            let maybe_summary = super::list_summaries_from_source_filtered(
                paths,
                Some(key),
                None,
                all_live.as_ref(),
            )?
            .into_iter()
            .next();
            if maybe_summary.is_none() && paths.collect_all_known_subagents().contains(key) {
                bail!("未找到 Antigravity 会话：{key}");
            }
            let verified_descendants =
                find_descendant_ids_with_live(&store, all_live.as_ref(), key)?;
            let verified_set: BTreeSet<&str> =
                verified_descendants.iter().map(String::as_str).collect();
            for child in descendants {
                valid_cascade_id(child)?;
                let allowed = include_descendants
                    && child != key
                    && (verified_set.contains(child.as_str())
                        || !session_exists_or_claimed_by_other_root(
                            paths,
                            &store,
                            all_live.as_ref(),
                            key,
                            &verified_set,
                            child,
                        )?);
                ensure!(allowed, "子会话 {child} 不属于当前主会话，拒绝删除");
            }
            if let Some(summary) = &maybe_summary {
                ensure!(
                    !is_session_busy_status(&summary.status)
                        && !is_cascade_running(all_live.as_ref(), key),
                    "{}",
                    summary
                        .delete_block_reason
                        .as_deref()
                        .unwrap_or("会话正在运行，请先在 Antigravity 中停止后再删除")
                );
            } else {
                ensure!(
                    !is_cascade_running(all_live.as_ref(), key),
                    "会话正在运行，请先在 Antigravity 中停止后再删除"
                );
            }
            if live_rpc.is_none() {
                ensure!(
                    !probe_presence_locked_shared(&store, key)
                        .context("无法探测 Antigravity 会话锁")?,
                    "会话当前已在 Antigravity 中打开，离线模式下无法安全删除"
                );
            }
            for child in verified_descendants.iter().chain(
                descendants
                    .iter()
                    .filter(|id| !verified_set.contains(id.as_str())),
            ) {
                ensure!(
                    !is_cascade_running(all_live.as_ref(), child),
                    "子会话正在运行，请先在 Antigravity 中停止后再删除"
                );
                if live_rpc.is_none() {
                    ensure!(
                        !probe_presence_locked_shared(&store, child)
                            .context("无法探测 Antigravity 会话锁")?,
                        "子会话当前已在 Antigravity 中打开，离线模式下无法安全删除"
                    );
                }
            }

            let selected: BTreeSet<&str> = descendants.iter().map(String::as_str).collect();
            let mut ordered_ids: Vec<String> = verified_descendants
                .into_iter()
                .rev()
                .filter(|id| selected.contains(id.as_str()))
                .collect();
            for child in descendants {
                if !ordered_ids.iter().any(|existing| existing == child) {
                    ordered_ids.push(child.clone());
                }
            }
            ordered_ids.push(key.to_owned());

            let (deleted_ids, residuals) = delete_ordered_ids_in_store(
                paths,
                &store,
                live_rpc.as_ref(),
                all_live.as_ref(),
                &ordered_ids,
            )?;
            Ok(SessionBatchItemResult {
                session_key: key.to_owned(),
                status: if residuals.is_empty() {
                    "succeeded"
                } else {
                    "failed"
                }
                .into(),
                message: (!residuals.is_empty())
                    .then(|| "部分 Antigravity 会话未完成删除，已停止后续删除".into()),
                deleted_ids,
                residuals,
            })
        }
    }
}

pub struct UnifiedAntigravityCleanupCandidate {
    pub id: String,
    pub title: String,
    pub bytes: u64,
    pub fingerprint: Option<String>,
    pub blocked_reason: Option<String>,
}

pub enum UnifiedAntigravityStepOutcome {
    Deleted(u64),
    Skipped(String),
    Failed(Option<String>, Vec<String>),
}

struct VerifiedOrphanSubagent {
    id: String,
    missing_ancestors: Vec<String>,
    title: String,
    bytes: u64,
    fingerprint: String,
    descendants: Vec<String>,
    is_cli_store: bool,
}

fn check_ancestor_exists(
    paths: &AntigravityPaths,
    store: &AntigravityPaths,
    all_live: Option<&Value>,
    anc_id: &str,
) -> std::io::Result<bool> {
    if all_live
        .and_then(|v| v["trajectorySummaries"].get(anc_id))
        .is_some()
        || has_primary_session_storage(store, anc_id)
    {
        return Ok(true);
    }
    let other = paths.other_store_for(&store.home);
    if other
        .as_ref()
        .is_some_and(|other| has_primary_session_storage(other, anc_id))
    {
        return Ok(true);
    }
    if probe_presence_locked_shared(store, anc_id)? {
        return Ok(true);
    }
    if let Some(other) = other {
        if probe_presence_locked_shared(&other, anc_id)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn scan_store_orphan_subagents(
    paths: &AntigravityPaths,
    store: &AntigravityPaths,
    is_cli_store: bool,
    all_live: Option<&Value>,
    children_map: &BTreeMap<String, Vec<String>>,
    now_ms: i64,
    mut lock_errors: Option<&mut BTreeMap<String, String>>,
) -> Result<BTreeMap<String, VerifiedOrphanSubagent>> {
    if !store.home.is_dir() {
        return Ok(BTreeMap::new());
    }
    let mut parent_sets: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut root_ids: BTreeMap<String, String> = BTreeMap::new();
    let mut titles: BTreeMap<String, String> = BTreeMap::new();
    let mut sqlite_statuses: BTreeMap<String, &'static str> = BTreeMap::new();

    let db_path = store.summaries_db();
    let mut open_conn: Option<Connection> = None;
    if db_path.is_file() {
        let conn = Connection::open_with_flags(
            &db_path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .context("无法打开 Antigravity 索引数据库扫描孤立子会话")?;
        if super::has_conversation_parents_table(&conn) {
            let mut stmt = conn.prepare(
                "SELECT conversation_id, COALESCE(parent_id, '') FROM conversation_parents",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?;
            for row in rows {
                let (cid, pid) = row?;
                if !pid.is_empty() && cid != pid && uuid::Uuid::parse_str(&cid).is_ok() {
                    parent_sets.entry(cid).or_default().insert(pid);
                }
            }
        }
        let (_, status_cols_sql) = super::summaries_status_sql_exprs(&conn);
        let sql = format!(
            "SELECT conversation_id, COALESCE(title, ''), COALESCE(preview, ''), \
             COALESCE(parent_conversation_id, ''), COALESCE(nesting_depth, 0), \
             {status_cols_sql} FROM conversation_summaries"
        );
        {
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt.query_map([], |row| {
                let (status_str, not_fully_idle, raw_blob) =
                    super::read_summary_status_cols(row, 5)?;
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                    status_str,
                    not_fully_idle,
                    raw_blob,
                ))
            })?;
            for row in rows {
                let (cid, title, preview, parent, depth, status_str, not_fully_idle, raw_blob) =
                    row?;
                if uuid::Uuid::parse_str(&cid).is_err() {
                    continue;
                }
                let (parsed_raw, st) = super::parse_summary_status_and_raw(
                    &status_str,
                    not_fully_idle,
                    raw_blob.as_deref(),
                );
                if let Some(st) = st {
                    sqlite_statuses.insert(cid.clone(), st);
                }
                let display = parsed_raw
                    .as_ref()
                    .and_then(|r| r.annotations.title.as_deref())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .unwrap_or_else(|| {
                        if !title.trim().is_empty() {
                            title.trim().to_owned()
                        } else if !preview.trim().is_empty() {
                            preview.trim().chars().take(80).collect()
                        } else {
                            String::new()
                        }
                    });
                if !display.is_empty() {
                    titles.insert(cid.clone(), display);
                }
                if let Some(rid) = parsed_raw
                    .as_ref()
                    .and_then(|r| r.trajectory_meta.root_conversation_id.as_deref())
                    .filter(|r| !r.is_empty() && *r != cid && uuid::Uuid::parse_str(r).is_ok())
                {
                    root_ids.insert(cid.clone(), rid.to_owned());
                }
                if !parent.is_empty() && parent != cid {
                    parent_sets.entry(cid).or_default().insert(parent);
                } else if let Some(pid) = parsed_raw
                    .as_ref()
                    .and_then(|r| r.trajectory_meta.effective_parent(&cid))
                {
                    parent_sets
                        .entry(cid.clone())
                        .or_default()
                        .insert(pid.to_owned());
                } else if depth > 0
                    || parsed_raw
                        .as_ref()
                        .is_some_and(|r| r.trajectory_meta.is_subagent(&cid))
                {
                    parent_sets.entry(cid).or_default();
                }
            }
        }
        open_conn = Some(conn);
    }

    let aux = super::load_store_auxiliary_index(store, open_conn.as_ref());
    for (pid, kids) in &aux.sidecar_children {
        for cid in kids {
            parent_sets
                .entry(cid.clone())
                .or_default()
                .insert(pid.clone());
        }
    }
    for (cid, s_meta) in &aux.sidecar_meta {
        if let Some(t) = &s_meta.title {
            titles.entry(cid.clone()).or_insert_with(|| t.clone());
        }
    }
    for (cid, c_meta) in &aux.conv_meta {
        if let Some(rid) = c_meta
            .root_id
            .as_deref()
            .filter(|r| uuid::Uuid::parse_str(r).is_ok())
        {
            root_ids
                .entry(cid.clone())
                .or_insert_with(|| rid.to_owned());
            if c_meta.parent_id.is_none() {
                parent_sets
                    .entry(cid.clone())
                    .or_default()
                    .insert(rid.to_owned());
            }
        }
        if let Some(pid) = c_meta
            .parent_id
            .as_deref()
            .filter(|p| uuid::Uuid::parse_str(p).is_ok())
        {
            parent_sets
                .entry(cid.clone())
                .or_default()
                .insert(pid.to_owned());
        }
        if let Some(t) = &c_meta.title {
            titles.entry(cid.clone()).or_insert_with(|| t.clone());
        }
    }
    if store.conversations_dir().is_dir() {
        if let Ok(entries) = fs::read_dir(store.conversations_dir()) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.extension().and_then(|s| s.to_str()) != Some("db") {
                    continue;
                }
                let Some(cid) = p.file_stem().and_then(|s| s.to_str()) else {
                    continue;
                };
                if uuid::Uuid::parse_str(cid).is_ok() {
                    if let Some(meta) = super::inspect_conv_db_metadata(store, cid) {
                        if let Some(rid) = meta.root_id {
                            root_ids.entry(cid.to_owned()).or_insert(rid);
                        }
                        for pid in meta.parent_ids {
                            parent_sets.entry(cid.to_owned()).or_default().insert(pid);
                        }
                    }
                }
            }
        }
    }

    if let Some(map) = all_live.and_then(|v| v["trajectorySummaries"].as_object()) {
        for (cid, item) in map {
            if uuid::Uuid::parse_str(cid).is_err() || !is_subtrajectory(cid, item) {
                continue;
            }
            let parent = item["parentConversationId"]
                .as_str()
                .or_else(|| item["trajectoryMetadata"]["parentConversationId"].as_str())
                .filter(|s| !s.is_empty() && *s != cid)
                .or_else(|| {
                    item["rootConversationId"]
                        .as_str()
                        .or_else(|| item["trajectoryMetadata"]["rootConversationId"].as_str())
                        .filter(|s| !s.is_empty() && *s != cid)
                });
            if let Some(pid) = parent {
                parent_sets
                    .entry(cid.clone())
                    .or_default()
                    .insert(pid.to_owned());
            }
            if !titles.contains_key(cid) {
                if let Some(sum) = item["annotations"]["title"]
                    .as_str()
                    .or_else(|| item["summary"].as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                {
                    titles.insert(cid.clone(), sum.to_owned());
                }
            }
        }
    }

    let mut out = BTreeMap::new();
    for (cid, parents) in &parent_sets {
        if parents.len() != 1
            || !has_primary_session_storage(store, cid)
            || paths.store_for_id(cid, all_live).home != store.home
        {
            continue;
        }
        let direct_parent = parents.iter().next().expect("single parent").clone();
        if uuid::Uuid::parse_str(&direct_parent).is_err() {
            continue;
        }
        let mut visited = BTreeSet::from([cid.clone()]);
        let mut missing_ancestors = Vec::new();
        let mut curr = direct_parent.clone();
        let mut valid_orphan_chain = true;
        let mut ancestor_lock_err = None;
        let mut hops = 0usize;
        loop {
            hops += 1;
            if hops > 32 || !visited.insert(curr.clone()) {
                valid_orphan_chain = false;
                break;
            }
            match check_ancestor_exists(paths, store, all_live, &curr) {
                Ok(true) => {
                    valid_orphan_chain = false;
                    break;
                }
                Ok(false) => missing_ancestors.push(curr.clone()),
                Err(err) => {
                    ancestor_lock_err = Some(format!("无法探测 Antigravity 会话锁：{err}"));
                    valid_orphan_chain = false;
                    break;
                }
            }
            match parent_sets.get(&curr) {
                Some(anc_parents) if anc_parents.len() == 1 => {
                    curr = anc_parents.iter().next().expect("single parent").clone();
                }
                Some(anc_parents) if anc_parents.is_empty() => break,
                Some(_) => {
                    valid_orphan_chain = false;
                    break;
                }
                None => break,
            }
        }
        if let Some(reason) = ancestor_lock_err {
            if let Some(errs) = lock_errors.as_deref_mut() {
                errs.entry(cid.clone()).or_insert(reason);
            }
            continue;
        }
        if !valid_orphan_chain {
            continue;
        }
        if let Some(rid) = root_ids.get(cid) {
            if !missing_ancestors.iter().any(|anc| anc == rid) {
                match check_ancestor_exists(paths, store, all_live, rid) {
                    Ok(true) => continue,
                    Ok(false) => missing_ancestors.push(rid.clone()),
                    Err(err) => {
                        if let Some(errs) = lock_errors.as_deref_mut() {
                            errs.entry(cid.clone())
                                .or_insert_with(|| format!("无法探测 Antigravity 会话锁：{err}"));
                        }
                        continue;
                    }
                }
            }
        }

        let descendants = descendant_ids(children_map, cid);
        let mut node_or_desc_busy = false;
        let mut subtree_lock_err = None;
        for node_id in std::iter::once(cid).chain(descendants.iter()) {
            match probe_presence_locked_shared(store, node_id) {
                Ok(true) => {
                    node_or_desc_busy = true;
                    break;
                }
                Ok(false) => {}
                Err(err) => {
                    subtree_lock_err = Some(format!("无法探测 Antigravity 会话锁：{err}"));
                    break;
                }
            }
            if is_cascade_running(all_live, node_id)
                || is_session_busy_status(super::evaluate_node_status_offline(
                    store,
                    node_id,
                    sqlite_statuses.get(node_id).copied(),
                    now_ms,
                ))
            {
                node_or_desc_busy = true;
                break;
            }
        }
        if let Some(reason) = subtree_lock_err {
            if let Some(errs) = lock_errors.as_deref_mut() {
                errs.entry(cid.clone()).or_insert(reason);
            }
            continue;
        }
        if node_or_desc_busy {
            continue;
        }

        let mut target_paths = owned_disk_paths(store, cid);
        for desc in &descendants {
            target_paths.extend(owned_disk_paths(store, desc));
        }
        if target_paths.is_empty() {
            continue;
        }
        let (_, bytes) = session_delete::size(&target_paths);
        let base_title = read_annotation_overlay(store, cid)
            .title
            .filter(|s| !s.trim().is_empty())
            .or_else(|| titles.get(cid).cloned())
            .unwrap_or_else(|| cid.clone());
        let title = format!("{base_title} (孤立子会话 ← {direct_parent})");
        let fingerprint = hex::encode(Sha256::digest(format!(
            "antigravity:orphan:{cid}:{bytes}:{direct_parent}"
        )));
        out.insert(
            cid.clone(),
            VerifiedOrphanSubagent {
                id: cid.clone(),
                missing_ancestors,
                title,
                bytes,
                fingerprint,
                descendants,
                is_cli_store,
            },
        );
    }
    Ok(out)
}

pub fn preview_unified_cleanup_candidates(
    paths: &AntigravityPaths,
) -> Result<Vec<UnifiedAntigravityCleanupCandidate>> {
    let primary = paths.primary_store();
    let cli = paths.cli_store();
    let now_ms = super::now_epoch_ms();
    let live = connect_endpoint(&primary)?.map(|(_, payload)| payload);
    let archived_summaries: Vec<ProviderSessionSummary> =
        super::list_summaries_from_source_filtered(paths, None, Some(true), live.as_ref())?;
    let mut primary_children = read_children_map(&primary, live.as_ref())?;
    let archived_ids: Vec<String> = archived_summaries.iter().map(|s| s.id.clone()).collect();
    expand_transcript_children_for_roots(&primary, &mut primary_children, &archived_ids);
    let cli_children = match &cli {
        Some(c) if c.home.is_dir() => {
            let mut m = read_children_map(c, None)?;
            expand_transcript_children_for_roots(c, &mut m, &archived_ids);
            m
        }
        _ => BTreeMap::new(),
    };

    let mut out = Vec::new();
    let mut seen_ids = BTreeSet::new();
    for summary in archived_summaries {
        seen_ids.insert(summary.id.clone());
        let store = paths.store_for_id(&summary.id, live.as_ref());
        let res = if cli.as_ref().is_some_and(|c| c.home == store.home) {
            preview_from_summary(
                &store,
                summary.clone(),
                None,
                &cli_children,
                SessionOperation::Delete,
                true,
            )
        } else {
            preview_from_summary(
                &primary,
                summary.clone(),
                live.as_ref(),
                &primary_children,
                SessionOperation::Delete,
                true,
            )
        };
        match res {
            Ok(preview) => {
                let blocked = (!preview.allowed)
                    .then(|| preview.reason.unwrap_or_else(|| "当前无法安全清理".into()));
                out.push(UnifiedAntigravityCleanupCandidate {
                    id: summary.session_key,
                    title: summary.title,
                    bytes: preview.bytes,
                    fingerprint: preview.fingerprint,
                    blocked_reason: blocked,
                });
            }
            Err(err) => {
                out.push(UnifiedAntigravityCleanupCandidate {
                    id: summary.session_key,
                    title: summary.title,
                    bytes: 0,
                    fingerprint: None,
                    blocked_reason: Some(err.to_string()),
                });
            }
        }
    }

    let primary_orphans = scan_store_orphan_subagents(
        paths,
        &primary,
        false,
        live.as_ref(),
        &primary_children,
        now_ms,
        None,
    )?;
    for (_, orphan) in primary_orphans {
        if seen_ids.insert(orphan.id.clone()) {
            out.push(UnifiedAntigravityCleanupCandidate {
                id: orphan.id,
                title: orphan.title,
                bytes: orphan.bytes,
                fingerprint: Some(orphan.fingerprint),
                blocked_reason: None,
            });
        }
    }
    if let Some(c) = &cli {
        if c.home.is_dir() {
            let cli_orphans =
                scan_store_orphan_subagents(paths, c, true, None, &cli_children, now_ms, None)?;
            for (_, orphan) in cli_orphans {
                if seen_ids.insert(orphan.id.clone()) {
                    out.push(UnifiedAntigravityCleanupCandidate {
                        id: orphan.id,
                        title: orphan.title,
                        bytes: orphan.bytes,
                        fingerprint: Some(orphan.fingerprint),
                        blocked_reason: None,
                    });
                }
            }
        }
    }

    Ok(out)
}

pub fn execute_unified_cleanup_targets(
    paths: &AntigravityPaths,
    targets: &[(String, Option<String>)],
) -> Result<Vec<(String, UnifiedAntigravityStepOutcome)>> {
    let primary = paths.primary_store();
    let cli = paths.cli_store();
    let now_ms = super::now_epoch_ms();
    let endpoint = connect_endpoint(&primary)?;
    let (live_rpc, live_payload) = match endpoint {
        Some((rpc, payload)) => (Some(rpc), Some(payload)),
        None => (None, None),
    };
    let archived_map: BTreeMap<String, ProviderSessionSummary> =
        super::list_summaries_from_source_filtered(paths, None, Some(true), live_payload.as_ref())?
            .into_iter()
            .map(|s| (s.id.clone(), s))
            .collect();
    let mut primary_children = read_children_map(&primary, live_payload.as_ref())?;
    let target_ids: Vec<String> = targets
        .iter()
        .map(|(raw, _)| raw.strip_prefix("ag:").unwrap_or(raw).to_owned())
        .filter(|id| uuid::Uuid::parse_str(id).is_ok())
        .collect();
    expand_transcript_children_for_roots(&primary, &mut primary_children, &target_ids);
    let cli_children = match &cli {
        Some(c) if c.home.is_dir() => {
            let mut m = read_children_map(c, None)?;
            expand_transcript_children_for_roots(c, &mut m, &target_ids);
            m
        }
        _ => BTreeMap::new(),
    };
    let mut orphan_lock_errors = BTreeMap::new();
    let mut orphan_map = scan_store_orphan_subagents(
        paths,
        &primary,
        false,
        live_payload.as_ref(),
        &primary_children,
        now_ms,
        Some(&mut orphan_lock_errors),
    )?;
    if let Some(c) = &cli {
        if c.home.is_dir() {
            for (k, v) in scan_store_orphan_subagents(
                paths,
                c,
                true,
                None,
                &cli_children,
                now_ms,
                Some(&mut orphan_lock_errors),
            )? {
                orphan_map.entry(k).or_insert(v);
            }
        }
    }

    let mut results = Vec::with_capacity(targets.len());
    for (raw_id, maybe_fp) in targets {
        let Some(req_fp) = maybe_fp.as_deref() else {
            results.push((
                raw_id.clone(),
                UnifiedAntigravityStepOutcome::Skipped("缺少预览指纹，已跳过".into()),
            ));
            continue;
        };
        let id = raw_id.strip_prefix("ag:").unwrap_or(raw_id);
        if let Some(summary) = archived_map.get(id).cloned() {
            let store = paths.store_for_id(id, live_payload.as_ref());
            let preview_res = if cli.as_ref().is_some_and(|c| c.home == store.home) {
                preview_from_summary(
                    &store,
                    summary,
                    None,
                    &cli_children,
                    SessionOperation::Delete,
                    true,
                )
            } else {
                preview_from_summary(
                    &primary,
                    summary,
                    live_payload.as_ref(),
                    &primary_children,
                    SessionOperation::Delete,
                    true,
                )
            };
            let preview = match preview_res {
                Ok(p) if p.allowed => p,
                Ok(p) => {
                    results.push((
                        raw_id.clone(),
                        UnifiedAntigravityStepOutcome::Failed(p.reason, Vec::new()),
                    ));
                    continue;
                }
                Err(err) => {
                    results.push((
                        raw_id.clone(),
                        UnifiedAntigravityStepOutcome::Failed(Some(err.to_string()), Vec::new()),
                    ));
                    continue;
                }
            };
            if preview.fingerprint.as_deref() != Some(req_fp) {
                results.push((
                    raw_id.clone(),
                    UnifiedAntigravityStepOutcome::Skipped("会话状态或指纹已变化，已跳过".into()),
                ));
                continue;
            }
            match session_summary_without_storage(paths, id) {
                Ok(latest) if !latest.archived => {
                    results.push((
                        raw_id.clone(),
                        UnifiedAntigravityStepOutcome::Skipped("会话已恢复为非归档状态".into()),
                    ));
                    continue;
                }
                Ok(_) => {}
                Err(err) => {
                    results.push((
                        raw_id.clone(),
                        UnifiedAntigravityStepOutcome::Failed(Some(err.to_string()), Vec::new()),
                    ));
                    continue;
                }
            }
            let step = match execute_batch_item(
                paths,
                id,
                SessionOperation::Delete,
                &preview.descendants,
                true,
            ) {
                Ok(item) if item.status == "succeeded" && item.residuals.is_empty() => {
                    UnifiedAntigravityStepOutcome::Deleted(preview.bytes)
                }
                Ok(item) => UnifiedAntigravityStepOutcome::Failed(item.message, item.residuals),
                Err(err) => {
                    UnifiedAntigravityStepOutcome::Failed(Some(err.to_string()), Vec::new())
                }
            };
            results.push((raw_id.clone(), step));
            continue;
        }

        if let Some(orphan) = orphan_map.get(id) {
            if orphan.fingerprint != req_fp {
                results.push((
                    raw_id.clone(),
                    UnifiedAntigravityStepOutcome::Skipped(
                        "孤立子会话状态或指纹已变化，已跳过".into(),
                    ),
                ));
                continue;
            }
            let store = if orphan.is_cli_store {
                match &cli {
                    Some(c) => c.clone(),
                    None => {
                        results.push((
                            raw_id.clone(),
                            UnifiedAntigravityStepOutcome::Skipped("CLI 存储不可用，已跳过".into()),
                        ));
                        continue;
                    }
                }
            } else {
                primary.clone()
            };
            let store_live = (!orphan.is_cli_store)
                .then_some(live_payload.as_ref())
                .flatten();
            let store_rpc = (!orphan.is_cli_store)
                .then_some(live_rpc.as_ref())
                .flatten();
            let mut anc_exists = false;
            let mut anc_err = None;
            for anc_id in &orphan.missing_ancestors {
                match check_ancestor_exists(paths, &store, store_live, anc_id) {
                    Ok(true) => {
                        anc_exists = true;
                        break;
                    }
                    Ok(false) => {}
                    Err(err) => {
                        anc_err = Some(err);
                        break;
                    }
                }
            }
            if let Some(err) = anc_err {
                results.push((
                    raw_id.clone(),
                    UnifiedAntigravityStepOutcome::Failed(
                        Some(format!("无法探测 Antigravity 会话锁：{err}")),
                        Vec::new(),
                    ),
                ));
                continue;
            }
            if anc_exists {
                results.push((
                    raw_id.clone(),
                    UnifiedAntigravityStepOutcome::Skipped("父会话已恢复或存在，已跳过".into()),
                ));
                continue;
            }
            let mut ordered_ids: Vec<String> = orphan.descendants.iter().rev().cloned().collect();
            ordered_ids.push(orphan.id.clone());
            let mut lock_check_failed = None;
            let mut active_or_open = false;
            for nid in &ordered_ids {
                if is_cascade_running(store_live, nid) {
                    active_or_open = true;
                    break;
                }
                match probe_presence_locked_shared(&store, nid) {
                    Ok(true) => {
                        active_or_open = true;
                        break;
                    }
                    Ok(false) => {}
                    Err(err) => {
                        lock_check_failed = Some(format!("无法探测 Antigravity 会话锁：{err}"));
                        break;
                    }
                }
            }
            if let Some(reason) = lock_check_failed {
                results.push((
                    raw_id.clone(),
                    UnifiedAntigravityStepOutcome::Failed(Some(reason), Vec::new()),
                ));
                continue;
            }
            if active_or_open {
                results.push((
                    raw_id.clone(),
                    UnifiedAntigravityStepOutcome::Skipped(
                        "孤立子会话当前处于活跃或打开状态，已跳过".into(),
                    ),
                ));
                continue;
            }
            let step = match delete_ordered_ids_in_store(
                paths,
                &store,
                store_rpc,
                store_live,
                &ordered_ids,
            ) {
                Ok((_, residuals)) if residuals.is_empty() => {
                    UnifiedAntigravityStepOutcome::Deleted(orphan.bytes)
                }
                Ok((_, residuals)) => UnifiedAntigravityStepOutcome::Failed(
                    Some("部分孤立子会话文件未完成删除".into()),
                    residuals,
                ),
                Err(err) => {
                    UnifiedAntigravityStepOutcome::Failed(Some(err.to_string()), Vec::new())
                }
            };
            results.push((raw_id.clone(), step));
            continue;
        }

        if let Some(reason) = orphan_lock_errors.get(id) {
            results.push((
                raw_id.clone(),
                UnifiedAntigravityStepOutcome::Failed(Some(reason.clone()), Vec::new()),
            ));
            continue;
        }

        results.push((
            raw_id.clone(),
            UnifiedAntigravityStepOutcome::Skipped(
                "目标不在当前已归档或孤立子会话允许集合中，已跳过".into(),
            ),
        ));
    }
    Ok(results)
}
