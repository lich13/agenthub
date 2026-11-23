use super::*;

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn patch_db_raw_summary(
    conn: &Connection,
    id: &str,
    new_title: Option<&str>,
    new_archived: Option<bool>,
) {
    let Ok(mut stmt) =
        conn.prepare("SELECT raw_summary FROM conversation_summaries WHERE conversation_id = ?1")
    else {
        return;
    };
    let Ok(Some(blob)) = stmt.query_row(params![id], |row| row.get::<_, Option<Vec<u8>>>(0)) else {
        return;
    };
    if let Some(patched) =
        patch_raw_summary_annotations(&blob, new_title, new_archived, now_unix_secs())
    {
        let _ = conn.execute(
            "UPDATE conversation_summaries SET raw_summary = ?1 WHERE conversation_id = ?2",
            params![patched, id],
        );
    }
}

fn resolve_target_store_and_endpoint(
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
        !is_presence_locked_shared(store, root_id),
        "会话当前已在 Antigravity 中打开，离线模式下无法安全{verb}"
    );
    if store.summaries_db().is_file() {
        let children = read_children_map(store, None)?;
        for child in descendant_ids(&children, root_id) {
            ensure!(
                !is_presence_locked_shared(store, &child),
                "子会话当前已在 Antigravity 中打开，离线模式下无法安全{verb}"
            );
        }
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
            existing.status != "running",
            "会话正在运行，请先在 Antigravity 中停止后再改名"
        );
        ensure_offline_unlocked(&store, key, "改名")?;
        write_annotation_overlay(&store, key, Some(&title), None)?;
        let db_path = store.summaries_db();
        if db_path.is_file() {
            if let Ok(conn) = Connection::open(&db_path) {
                let _ = conn.execute(
                    "UPDATE conversation_summaries SET title = ?1 WHERE conversation_id = ?2",
                    params![title, key],
                );
                patch_db_raw_summary(&conn, key, Some(&title), None);
                let _ = conn.execute(
                    "UPDATE conversations_fts SET title = ?1 WHERE conversation_id = ?2 AND chunk_index = -1",
                    params![title, key],
                );
            }
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
                    entry.insert("summary".into(), Value::String(title.clone()));
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
            existing.status != "running",
            "会话正在运行，请先在 Antigravity 中停止后再{verb}"
        );
        ensure_offline_unlocked(&store, key, verb)?;
        write_annotation_overlay(&store, key, None, Some(archived))?;
        let db_path = store.summaries_db();
        if db_path.is_file() {
            if let Ok(conn) = Connection::open(&db_path) {
                patch_db_raw_summary(&conn, key, None, Some(archived));
                let flag = i64::from(archived);
                let _ = conn.execute(
                    "UPDATE conversations_fts SET archived = ?1 WHERE conversation_id = ?2 AND chunk_index = -1",
                    params![flag, key],
                );
            }
        }
        invalidate_proto_cache(&store);
    }
    session_summary(paths, key)
}

pub(super) fn is_presence_locked_shared(paths: &AntigravityPaths, id: &str) -> bool {
    let lock_path = paths.presence_dir().join(format!("{id}.lock"));
    if !lock_path.is_file() {
        return false;
    }
    use std::os::unix::fs::OpenOptionsExt;
    use std::os::unix::io::AsRawFd;
    let Ok(file) = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&lock_path)
    else {
        return false;
    };
    // SAFETY: The File owns this live descriptor throughout the advisory lock operation.
    let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_SH | libc::LOCK_NB) };
    if rc == 0 {
        // SAFETY: The File still owns the descriptor whose advisory lock is being released.
        unsafe {
            let _ = libc::flock(file.as_raw_fd(), libc::LOCK_UN);
        }
        false
    } else {
        let err = std::io::Error::last_os_error();
        matches!(
            err.raw_os_error(),
            Some(code) if code == libc::EWOULDBLOCK || code == libc::EAGAIN
        )
    }
}

pub fn live_item_is_running(item: &Value) -> bool {
    let waiting = item["waitingSteps"]
        .as_array()
        .is_some_and(|arr| !arr.is_empty());
    let run_status = item["status"].as_str().unwrap_or("");
    waiting
        || matches!(
            run_status,
            "CASCADE_RUN_STATUS_RUNNING"
                | "CASCADE_RUN_STATUS_BUSY"
                | "CASCADE_RUN_STATUS_WAITING"
                | "CASCADE_RUN_STATUS_CANCELING"
        )
        || item["notFullyIdle"].as_bool() == Some(true)
        || item["hasRunningSubagents"].as_bool() == Some(true)
}

fn is_cascade_running(all_live: Option<&Value>, id: &str) -> bool {
    all_live
        .and_then(|v| v["trajectorySummaries"].get(id))
        .is_some_and(live_item_is_running)
}

fn find_descendant_ids_with_live(
    paths: &AntigravityPaths,
    all_live: Option<&Value>,
    root_id: &str,
) -> Result<Vec<String>> {
    Ok(descendant_ids(
        &read_children_map(paths, all_live)?,
        root_id,
    ))
}

fn read_children_map(
    paths: &AntigravityPaths,
    all_live: Option<&Value>,
) -> Result<BTreeMap<String, Vec<String>>> {
    let mut children_map: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let db_path = paths.summaries_db();
    if db_path.is_file() {
        let conn = Connection::open_with_flags(
            &db_path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .context("无法打开 Antigravity 索引数据库校验子会话")?;
        let mut stmt = conn
            .prepare(
                "SELECT conversation_id, COALESCE(parent_conversation_id, '') FROM conversation_summaries",
            )
            .context("无法读取 Antigravity 索引数据库子会话表")?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .context("无法查询 Antigravity 子会话关系")?;
        for row in rows {
            let (cid, pid) = row.context("解码 Antigravity 子会话索引行失败")?;
            if !pid.is_empty() && uuid::Uuid::parse_str(&cid).is_ok() {
                children_map.entry(pid).or_default().push(cid);
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
                if !pid.is_empty() && uuid::Uuid::parse_str(&cid).is_ok() {
                    children_map.entry(pid).or_default().push(cid);
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
                children_map
                    .entry(parent.to_owned())
                    .or_default()
                    .push(cid.clone());
            }
            let root = item["rootConversationId"]
                .as_str()
                .or_else(|| item["trajectoryMetadata"]["rootConversationId"].as_str())
                .unwrap_or("");
            if !root.is_empty() && root != cid {
                children_map
                    .entry(root.to_owned())
                    .or_default()
                    .push(cid.clone());
            }
        }
    }
    Ok(children_map)
}

pub(super) fn descendant_ids(
    children_map: &BTreeMap<String, Vec<String>>,
    root_id: &str,
) -> Vec<String> {
    let mut result = BTreeSet::new();
    let mut stack = vec![root_id.to_owned()];
    while let Some(curr) = stack.pop() {
        if let Some(kids) = children_map.get(&curr) {
            for kid in kids {
                if kid != root_id && result.insert(kid.clone()) {
                    stack.push(kid.clone());
                }
            }
        }
    }
    result.into_iter().collect()
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

pub fn preview_batch_item(
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
    let db_path = other.summaries_db();
    if !db_path.is_file() {
        return;
    }
    let Ok(conn) = Connection::open(&db_path) else {
        return;
    };
    let has_parents = super::has_conversation_parents_table(&conn);
    let mut touched = false;
    for id in deleted_ids {
        if has_primary_session_storage(other, id) || is_presence_locked_shared(other, id) {
            continue;
        }
        if conn
            .execute(
                "DELETE FROM conversation_summaries WHERE conversation_id = ?1",
                params![id],
            )
            .unwrap_or(0)
            > 0
        {
            touched = true;
        }
        if conn
            .execute(
                "DELETE FROM conversations_fts WHERE conversation_id = ?1",
                params![id],
            )
            .unwrap_or(0)
            > 0
        {
            touched = true;
        }
        if has_parents {
            let _ = conn.execute(
                "DELETE FROM conversation_parents WHERE conversation_id = ?1",
                params![id],
            );
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
        let primary_children = if operation == SessionOperation::Delete || live.is_none() {
            read_children_map(&primary, live.as_ref())?
        } else {
            BTreeMap::new()
        };
        let cli_children = match &cli {
            Some(c) if c.summaries_db().is_file() => read_children_map(c, None)?,
            _ => BTreeMap::new(),
        };
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
        && (summary.status == "running" || is_cascade_running(all_live, &summary.id))
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
        if summary.status == "running" {
            bail!(
                "{}",
                summary.archive_block_reason.as_deref().unwrap_or(&format!(
                    "会话正在运行，请先在 Antigravity 中停止后再{verb}"
                ))
            );
        }
        ensure!(
            !is_presence_locked_shared(paths, &summary.id),
            "会话当前已在 Antigravity 中打开，离线模式下无法安全{verb}"
        );
        for child in descendant_ids(children, &summary.id) {
            ensure!(
                !is_presence_locked_shared(paths, &child),
                "子会话当前已在 Antigravity 中打开，离线模式下无法安全{verb}"
            );
        }
    }
    if operation == SessionOperation::Delete
        && all_live.is_none()
        && is_presence_locked_shared(paths, &summary.id)
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
                    !is_presence_locked_shared(paths, child),
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
            let summary = session_summary_without_storage(paths, key)?;
            let (store, endpoint) = resolve_target_store_and_endpoint(paths, key)?;
            let (live_rpc, all_live) = match endpoint {
                Some((rpc, live)) => (Some(rpc), Some(live)),
                None => (None, None),
            };
            let verified_descendants =
                find_descendant_ids_with_live(&store, all_live.as_ref(), key)?;
            let verified_set: BTreeSet<&str> =
                verified_descendants.iter().map(String::as_str).collect();
            for child in descendants {
                valid_cascade_id(child)?;
                ensure!(
                    include_descendants && verified_set.contains(child.as_str()),
                    "子会话 {child} 不属于当前主会话，拒绝删除"
                );
            }
            ensure!(
                summary.status != "running" && !is_cascade_running(all_live.as_ref(), key),
                "会话正在运行，请先在 Antigravity 中停止后再删除"
            );
            if live_rpc.is_none() {
                ensure!(
                    !is_presence_locked_shared(&store, key),
                    "会话当前已在 Antigravity 中打开，离线模式下无法安全删除"
                );
            }
            for child in descendants {
                ensure!(
                    !is_cascade_running(all_live.as_ref(), child),
                    "子会话正在运行，请先在 Antigravity 中停止后再删除"
                );
                if live_rpc.is_none() {
                    ensure!(
                        !is_presence_locked_shared(&store, child),
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
            ordered_ids.push(key.to_owned());

            let db_path = store.summaries_db();
            let offline_conn = if live_rpc.is_none() && db_path.is_file() {
                Some(
                    Connection::open(&db_path)
                        .context("无法打开 Antigravity 索引数据库执行删除")?,
                )
            } else {
                None
            };
            let has_parents_table = offline_conn
                .as_ref()
                .is_some_and(super::has_conversation_parents_table);

            let mut outcome = Outcome::default();
            let mut deleted_ids = Vec::new();
            for (idx, id) in ordered_ids.iter().enumerate() {
                if let Some(rpc) = &live_rpc {
                    if let Err(err) = connect_rpc_post(
                        rpc.port,
                        &rpc.token,
                        "DeleteCascadeTrajectory",
                        &json!({ "cascadeId": id }),
                    ) {
                        outcome.residuals.push(format!("{id}: {err}"));
                        record_remaining_antigravity_residuals(
                            &store,
                            &ordered_ids[idx..],
                            &mut outcome,
                        );
                        break;
                    }
                }

                let mut disk_failed = false;
                for path in owned_disk_paths(&store, id) {
                    if !outcome.remove_strict(&path) {
                        disk_failed = true;
                        break;
                    }
                }
                if disk_failed {
                    record_remaining_antigravity_residuals(
                        &store,
                        &ordered_ids[idx..],
                        &mut outcome,
                    );
                    break;
                }

                if let Some(conn) = &offline_conn {
                    if let Err(err) = conn.execute(
                        "DELETE FROM conversation_summaries WHERE conversation_id = ?1",
                        params![id],
                    ) {
                        outcome
                            .residuals
                            .push(format!("{}: {err}", db_path.display()));
                        record_remaining_antigravity_residuals(
                            &store,
                            &ordered_ids[idx + 1..],
                            &mut outcome,
                        );
                        break;
                    }
                    let _ = conn.execute(
                        "DELETE FROM conversations_fts WHERE conversation_id = ?1",
                        params![id],
                    );
                    if has_parents_table {
                        let _ = conn.execute(
                            "DELETE FROM conversation_parents WHERE conversation_id = ?1",
                            params![id],
                        );
                    }
                }
                deleted_ids.push(id.clone());
            }

            if !deleted_ids.is_empty() {
                clean_store_shared_files(&store, &deleted_ids, &mut outcome);
                invalidate_proto_cache(&store);
                if let Some(other) = paths.other_store_for(&store.home) {
                    clean_other_store_mirror_rows(&other, &deleted_ids);
                }
            }
            outcome.finish();
            Ok(SessionBatchItemResult {
                session_key: key.to_owned(),
                status: if outcome.residuals.is_empty() {
                    "succeeded"
                } else {
                    "failed"
                }
                .into(),
                message: (!outcome.residuals.is_empty())
                    .then(|| "部分 Antigravity 会话未完成删除，已停止后续删除".into()),
                deleted_ids,
                residuals: outcome.residuals,
            })
        }
    }
}
