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
        !is_presence_locked_shared(store, root_id),
        "会话当前已在 Antigravity 中打开，离线模式下无法安全{verb}"
    );
    for child in find_descendant_ids_with_live(store, None, root_id)? {
        ensure!(
            !is_presence_locked_shared(store, &child),
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

pub(crate) fn is_presence_locked_shared(paths: &AntigravityPaths, id: &str) -> bool {
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
        if !parent.is_empty() && parent != child && uuid::Uuid::parse_str(&child).is_ok() {
            let list = children_map.entry(parent).or_default();
            if !list.contains(&child) {
                list.push(child);
            }
        }
    };
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
            insert_edge(pid, cid);
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

fn delete_ordered_ids_in_store(
    paths: &AntigravityPaths,
    store: &AntigravityPaths,
    live_rpc: Option<&LiveRpc>,
    all_live: Option<&Value>,
    ordered_ids: &[String],
) -> Result<(Vec<String>, Vec<String>)> {
    let db_path = store.summaries_db();
    let offline_conn = if live_rpc.is_none() && db_path.is_file() {
        Some(Connection::open(&db_path).context("无法打开 Antigravity 索引数据库执行删除")?)
    } else {
        None
    };
    let has_parents_table = offline_conn
        .as_ref()
        .is_some_and(super::has_conversation_parents_table);

    let mut outcome = Outcome::default();
    let mut deleted_ids = Vec::new();
    let mut unhydrated_db_cleanup_ids = Vec::new();
    for (idx, id) in ordered_ids.iter().enumerate() {
        if let Some(rpc) = live_rpc {
            let in_live = all_live
                .and_then(|v| v["trajectorySummaries"].get(id))
                .is_some();
            if let Err(err) = connect_rpc_post(
                rpc.port,
                &rpc.token,
                "DeleteCascadeTrajectory",
                &json!({ "cascadeId": id }),
            ) {
                let allow_not_found = !in_live
                    && !is_presence_locked_shared(store, id)
                    && err
                        .downcast_ref::<ConnectRpcHttpError>()
                        .is_some_and(ConnectRpcHttpError::is_not_found);
                if allow_not_found {
                    unhydrated_db_cleanup_ids.push(id.clone());
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

        if let Some(conn) = &offline_conn {
            if let Err(err) = conn.execute(
                "DELETE FROM conversation_summaries WHERE conversation_id = ?1",
                params![id],
            ) {
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

    if !unhydrated_db_cleanup_ids.is_empty() && db_path.is_file() {
        match Connection::open(&db_path) {
            Ok(conn) => {
                let _ = conn.busy_timeout(Duration::from_millis(1000));
                let has_parents = super::has_conversation_parents_table(&conn);
                for cid in &unhydrated_db_cleanup_ids {
                    if deleted_ids.contains(cid) {
                        if let Err(err) = conn.execute(
                            "DELETE FROM conversation_summaries WHERE conversation_id = ?1",
                            params![cid],
                        ) {
                            outcome
                                .residuals
                                .push(format!("{}: {err}", db_path.display()));
                            break;
                        }
                        let _ = conn.execute(
                            "DELETE FROM conversations_fts WHERE conversation_id = ?1",
                            params![cid],
                        );
                        if has_parents {
                            let _ = conn.execute(
                                "DELETE FROM conversation_parents WHERE conversation_id = ?1",
                                params![cid],
                            );
                        }
                    }
                }
            }
            Err(err) => {
                outcome
                    .residuals
                    .push(format!("{}: {err}", db_path.display()));
            }
        }
    }

    if !deleted_ids.is_empty() {
        clean_store_shared_files(store, &deleted_ids, &mut outcome);
        invalidate_proto_cache(store);
        if let Some(other) = paths.other_store_for(&store.home) {
            clean_other_store_mirror_rows(&other, &deleted_ids);
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
                !is_session_busy_status(&summary.status)
                    && !is_cascade_running(all_live.as_ref(), key),
                "{}",
                summary
                    .delete_block_reason
                    .as_deref()
                    .unwrap_or("会话正在运行，请先在 Antigravity 中停止后再删除")
            );
            if live_rpc.is_none() {
                ensure!(
                    !is_presence_locked_shared(&store, key),
                    "会话当前已在 Antigravity 中打开，离线模式下无法安全删除"
                );
            }
            for child in &verified_descendants {
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
    direct_parent: String,
    title: String,
    bytes: u64,
    fingerprint: String,
    descendants: Vec<String>,
    is_cli_store: bool,
}

fn ancestor_exists_anywhere(
    paths: &AntigravityPaths,
    store: &AntigravityPaths,
    all_live: Option<&Value>,
    anc_id: &str,
) -> bool {
    all_live
        .and_then(|v| v["trajectorySummaries"].get(anc_id))
        .is_some()
        || has_primary_session_storage(store, anc_id)
        || is_presence_locked_shared(store, anc_id)
        || paths.other_store_for(&store.home).is_some_and(|other| {
            has_primary_session_storage(&other, anc_id) || is_presence_locked_shared(&other, anc_id)
        })
}

fn scan_store_orphan_subagents(
    paths: &AntigravityPaths,
    store: &AntigravityPaths,
    is_cli_store: bool,
    all_live: Option<&Value>,
    children_map: &BTreeMap<String, Vec<String>>,
    now_ms: i64,
) -> Result<BTreeMap<String, VerifiedOrphanSubagent>> {
    if !store.home.is_dir() {
        return Ok(BTreeMap::new());
    }
    let mut parent_sets: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut titles: BTreeMap<String, String> = BTreeMap::new();
    let mut sqlite_statuses: BTreeMap<String, &'static str> = BTreeMap::new();

    let db_path = store.summaries_db();
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
        let cols = super::summaries_table_columns(&conn);
        let status_expr = if cols.contains("status") {
            "COALESCE(status, '')"
        } else {
            "''"
        };
        let idle_expr = if cols.contains("not_fully_idle") {
            "not_fully_idle"
        } else {
            "0"
        };
        let sql = format!(
            "SELECT conversation_id, COALESCE(title, ''), COALESCE(preview, ''), \
             COALESCE(parent_conversation_id, ''), COALESCE(nesting_depth, 0), \
             {status_expr}, {idle_expr} FROM conversation_summaries"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, super::ValueOrString>(6)
                    .map(|v| v.0)
                    .unwrap_or(false),
            ))
        })?;
        for row in rows {
            let (cid, title, preview, parent, depth, status_str, not_fully_idle) = row?;
            if uuid::Uuid::parse_str(&cid).is_err() {
                continue;
            }
            if let Some(st) = super::offline_summary_row_status(&status_str, not_fully_idle) {
                sqlite_statuses.insert(cid.clone(), st);
            }
            let display = if !title.trim().is_empty() {
                title.trim().to_owned()
            } else if !preview.trim().is_empty() {
                preview.trim().chars().take(80).collect()
            } else {
                String::new()
            };
            if !display.is_empty() {
                titles.insert(cid.clone(), display);
            }
            if !parent.is_empty() && parent != cid {
                parent_sets.entry(cid).or_default().insert(parent);
            } else if depth > 0 {
                parent_sets.entry(cid).or_default();
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
        let mut curr = direct_parent.clone();
        let mut valid_orphan_chain = true;
        let mut hops = 0usize;
        loop {
            hops += 1;
            if hops > 32
                || !visited.insert(curr.clone())
                || ancestor_exists_anywhere(paths, store, all_live, &curr)
            {
                valid_orphan_chain = false;
                break;
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
        if !valid_orphan_chain {
            continue;
        }

        let descendants = descendant_ids(children_map, cid);
        let node_or_desc_busy = std::iter::once(cid)
            .chain(descendants.iter())
            .any(|node_id| {
                is_presence_locked_shared(store, node_id)
                    || is_cascade_running(all_live, node_id)
                    || is_session_busy_status(super::evaluate_node_status_offline(
                        store,
                        node_id,
                        sqlite_statuses.get(node_id).copied(),
                        now_ms,
                    ))
            });
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
                direct_parent,
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
        list_summaries_from_source(paths, None, live.as_ref())?
            .into_iter()
            .filter(|s| s.archived)
            .collect();
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
                scan_store_orphan_subagents(paths, c, true, None, &cli_children, now_ms)?;
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
        list_summaries_from_source(paths, None, live_payload.as_ref())?
            .into_iter()
            .filter(|s| s.archived)
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
    let mut orphan_map = scan_store_orphan_subagents(
        paths,
        &primary,
        false,
        live_payload.as_ref(),
        &primary_children,
        now_ms,
    )?;
    if let Some(c) = &cli {
        if c.home.is_dir() {
            for (k, v) in scan_store_orphan_subagents(paths, c, true, None, &cli_children, now_ms)?
            {
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
            if ancestor_exists_anywhere(paths, &store, store_live, &orphan.direct_parent) {
                results.push((
                    raw_id.clone(),
                    UnifiedAntigravityStepOutcome::Skipped("父会话已恢复或存在，已跳过".into()),
                ));
                continue;
            }
            let mut ordered_ids: Vec<String> = orphan.descendants.iter().rev().cloned().collect();
            ordered_ids.push(orphan.id.clone());
            if ordered_ids.iter().any(|nid| {
                is_presence_locked_shared(&store, nid) || is_cascade_running(store_live, nid)
            }) {
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

        results.push((
            raw_id.clone(),
            UnifiedAntigravityStepOutcome::Skipped(
                "目标不在当前已归档或孤立子会话允许集合中，已跳过".into(),
            ),
        ));
    }
    Ok(results)
}
