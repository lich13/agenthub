//! OS attachment ingress. A receiver is scoped to one visible main-window
//! composer, machine revision and desktop session. Channels never carry paths.
use super::*;
use crate::codex_desktop::clipboard::RawPasteboardSnapshot;
use crate::host::{HostBridge, StreamSink};
use agenthub_core::codex::desktop::{attachments::AttachmentStore, references::NativeReference};

type SharedAttachments = Arc<std::sync::Mutex<AttachmentStore>>;

#[derive(Clone)]
pub(super) struct Receiver {
    id: String,
    revision: u64,
    intent: u64,
    session: String,
    thread: String,
    bounds: DesktopAttachmentBounds,
    handles: Vec<String>,
    last_paste: Option<i64>,
    stream: StreamSink,
}

impl Receiver {
    pub(super) fn session(&self) -> &str {
        &self.session
    }
    pub(super) fn id(&self) -> &str {
        &self.id
    }
}

fn visible(host: &HostBridge) -> bool {
    host.is_main_window_visible_and_focused()
}

pub(super) fn valid_bounds(bounds: &DesktopAttachmentBounds) -> bool {
    [bounds.x, bounds.y, bounds.width, bounds.height]
        .iter()
        .all(|value| value.is_finite() && *value >= 0.0 && *value <= 65536.0)
        && bounds.width > 0.0
        && bounds.height > 0.0
}

fn contains(bounds: &DesktopAttachmentBounds, x: f64, y: f64) -> bool {
    x.is_finite()
        && y.is_finite()
        && x >= bounds.x
        && y >= bounds.y
        && x < bounds.x + bounds.width
        && y < bounds.y + bounds.height
}

async fn context(
    host: &HostBridge,
    bridge: &CodexDesktopBridge,
    remote: &RemoteConnections,
    runtime: &BridgeRuntime,
    receiver: &Receiver,
) -> Result<(), String> {
    if !visible(host) || bridge.intent.load(Ordering::SeqCst) != receiver.intent {
        return Err("附件接收区已变化，请重新连接".into());
    }
    remote.require_local(receiver.revision)?;
    let view = runtime.worker(&receiver.session).await?.current();
    if !view.connected || !view.writable || view.thread_id != receiver.thread {
        return Err("当前线程仅支持阅读".into());
    }
    if !bridge
        .receiver
        .lock()
        .await
        .as_ref()
        .is_some_and(|current| current.id == receiver.id)
    {
        return Err("附件接收区已变化，请重试".into());
    }
    Ok(())
}

async fn references(
    bridge: &CodexDesktopBridge,
    receiver: &Receiver,
    handles: &[String],
) -> Result<Vec<NativeReference>, String> {
    if handles.len() > super::clipboard::MAX_FILES {
        return Err("引用数量超出限制".into());
    }
    let grants = bridge.grants.lock().await;
    handles
        .iter()
        .map(
            |handle| match grants.resolve(receiver.revision, Some(&receiver.thread), handle)? {
                GrantValue::Reference(reference) => Ok(reference),
                _ => Err("所选引用不可用于发送".into()),
            },
        )
        .collect()
}

async fn release(
    store: &SharedAttachments,
    references: Vec<NativeReference>,
) -> Result<(), String> {
    let store = store.clone();
    agenthub_core::read_tasks::spawn_blocking(move || {
        store
            .lock()
            .map_err(|_| "无法释放粘贴附件")?
            .release(&references)
    })
    .await
    .map_err(|_| "无法释放粘贴附件")?
    .map_err(str::to_owned)
}

pub(super) async fn unbind(bridge: &CodexDesktopBridge, id: &str) {
    let mut current = bridge.receiver.lock().await;
    if current.as_ref().is_some_and(|current| current.id == id) {
        if let Some(removed) = current.take() {
            removed.stream.close();
        }
    }
}

pub(super) async fn unbind_all(bridge: &CodexDesktopBridge) {
    if let Some(removed) = bridge.receiver.lock().await.take() {
        removed.stream.close();
    }
}

pub(crate) async fn handle(
    host: &HostBridge,
    state: &DesktopState,
    remote: &RemoteConnections,
    bridge: &CodexDesktopBridge,
    request: DesktopAttachmentRequest,
    event_stream: Option<StreamSink>,
) -> Result<DesktopAttachmentResult, String> {
    if let DesktopAttachmentAction::Unbind { binding_id } = &request.action {
        unbind(bridge, binding_id).await;
        return Ok(DesktopAttachmentResult {
            binding_id: binding_id.clone(),
            paste_disposition: None,
        });
    }
    let _guard = remote.begin_local_write(request.revision)?;
    let runtime = bridge
        .runtime
        .lock()
        .await
        .clone()
        .ok_or("请先连接 Codex 桌面")?;
    let worker = runtime.worker(&request.session_id).await?;
    let view = worker.current();
    if !visible(host) || !view.connected || !view.writable {
        return Err("请在当前协作输入框中添加附件".into());
    }
    if matches!(&request.action, DesktopAttachmentAction::Bind { .. }) {
        verify_thread(state, &view.thread_id).await?;
    }
    if let DesktopAttachmentAction::Bind { bounds, handles } = request.action {
        if !valid_bounds(&bounds) {
            return Err("附件接收区域无效".into());
        }
        let stream = event_stream.ok_or("附件事件通道未就绪")?;
        let receiver = Receiver {
            id: agenthub_core::security::hash_token(&SecretBox::random_key_material()),
            revision: request.revision,
            intent: bridge.intent.load(Ordering::SeqCst),
            session: request.session_id.clone(),
            thread: view.thread_id,
            bounds,
            handles,
            last_paste: None,
            stream,
        };
        references(bridge, &receiver, &receiver.handles).await?;
        remote.require_local(request.revision)?;
        runtime.worker(&request.session_id).await?;
        let id = receiver.id.clone();
        if let Some(prev) = bridge.receiver.lock().await.replace(receiver) {
            prev.stream.close();
        }
        return Ok(DesktopAttachmentResult {
            binding_id: id,
            paste_disposition: None,
        });
    }
    let receiver = bridge
        .receiver
        .lock()
        .await
        .clone()
        .ok_or("附件接收区已关闭")?;
    let binding_id = match &request.action {
        DesktopAttachmentAction::Bounds { binding_id, .. }
        | DesktopAttachmentAction::Paste { binding_id }
        | DesktopAttachmentAction::Release { binding_id, .. }
        | DesktopAttachmentAction::Unbind { binding_id } => binding_id,
        DesktopAttachmentAction::Bind { .. } => unreachable!(),
    };
    if receiver.id != *binding_id
        || receiver.session != request.session_id
        || receiver.revision != request.revision
    {
        return Err("附件接收区已变化".into());
    }
    context(host, bridge, remote, &runtime, &receiver).await?;
    match request.action {
        DesktopAttachmentAction::Bounds {
            bounds, handles, ..
        } => {
            if !valid_bounds(&bounds) {
                return Err("附件接收区域无效".into());
            }
            references(bridge, &receiver, &handles).await?;
            let mut current = bridge.receiver.lock().await;
            let current = current
                .as_mut()
                .filter(|current| current.id == receiver.id)
                .ok_or("附件接收区已变化")?;
            if current
                .handles
                .iter()
                .any(|handle| !handles.contains(handle))
            {
                current.last_paste = None;
            }
            current.bounds = bounds;
            current.handles = handles;
        }
        DesktopAttachmentAction::Release { handles, .. } => {
            if handles
                .iter()
                .any(|handle| !receiver.handles.contains(handle))
            {
                return Err("附件授权已变化".into());
            }
            let values = references(bridge, &receiver, &handles).await?;
            release(&runtime.attachments, values).await?;
            if let Some(current) = bridge
                .receiver
                .lock()
                .await
                .as_mut()
                .filter(|current| current.id == receiver.id)
            {
                current.handles.retain(|handle| !handles.contains(handle));
                current.last_paste = None;
            }
        }
        DesktopAttachmentAction::Paste { .. } => {
            return Err("请通过原生剪贴板快照入口粘贴".into());
        }
        DesktopAttachmentAction::Bind { .. } | DesktopAttachmentAction::Unbind { .. } => {
            unreachable!()
        }
    }
    Ok(DesktopAttachmentResult {
        binding_id: receiver.id,
        paste_disposition: None,
    })
}

pub(crate) async fn paste(
    host: &HostBridge,
    state: &DesktopState,
    remote: &RemoteConnections,
    bridge: &CodexDesktopBridge,
    request: DesktopAttachmentRequest,
    snapshot: RawPasteboardSnapshot,
) -> Result<DesktopAttachmentResult, String> {
    let DesktopAttachmentAction::Paste { ref binding_id } = request.action else {
        return Err("粘贴请求类型无效".into());
    };
    let _guard = remote.begin_local_write(request.revision)?;
    let runtime = bridge
        .runtime
        .lock()
        .await
        .clone()
        .ok_or("请先连接 Codex 桌面")?;
    let worker = runtime.worker(&request.session_id).await?;
    let view = worker.current();
    if !visible(host) || !view.connected || !view.writable {
        return Err("请在当前协作输入框中添加附件".into());
    }
    verify_thread(state, &view.thread_id).await?;

    let receiver = bridge
        .receiver
        .lock()
        .await
        .clone()
        .ok_or("附件接收区已关闭")?;
    if receiver.id != *binding_id
        || receiver.session != request.session_id
        || receiver.revision != request.revision
    {
        return Err("附件接收区已变化".into());
    }
    let _serial = bridge.importing.lock().await;
    context(host, bridge, remote, &runtime, &receiver).await?;

    let last = bridge
        .receiver
        .lock()
        .await
        .as_ref()
        .and_then(|current| current.last_paste);
    if last.is_some_and(|prev| {
        prev == snapshot.change_count_before && prev == snapshot.change_count_after
    }) {
        return Ok(DesktopAttachmentResult {
            binding_id: receiver.id,
            paste_disposition: Some(DesktopPasteDisposition::Attachments),
        });
    }

    let store = runtime.attachments.clone();
    let (count, disposition, values) = agenthub_core::read_tasks::spawn_blocking(move || {
        let capture = super::clipboard::read_paste(&snapshot, dirs::home_dir().as_deref())?;
        use super::clipboard::PasteContent;
        let (disposition, values) = match capture.content {
            PasteContent::Files(files) => (DesktopPasteDisposition::Attachments, files),
            PasteContent::Image(png) => {
                let file = store
                    .lock()
                    .map_err(|_| "无法保存粘贴图片")?
                    .import_png(&png, jiff::Timestamp::now().as_second().max(0) as u64)?;
                (DesktopPasteDisposition::Attachments, vec![file])
            }
            PasteContent::Text => (DesktopPasteDisposition::Text, Vec::new()),
            PasteContent::Unsupported => (DesktopPasteDisposition::Unsupported, Vec::new()),
        };
        Ok::<_, &'static str>((capture.change_count, disposition, values))
    })
    .await
    .map_err(|_| "无法读取剪贴板附件")??;

    let has_attachments = !values.is_empty();
    finish_paste(
        &runtime.attachments,
        count,
        &values,
        host.pasteboard_change_count(),
        async {
            if has_attachments {
                deliver(
                    host,
                    state,
                    remote,
                    bridge,
                    &runtime,
                    &receiver,
                    values.clone(),
                )
                .await
            } else {
                context(host, bridge, remote, &runtime, &receiver).await
            }
        },
    )
    .await?;
    if !has_attachments {
        return Ok(DesktopAttachmentResult {
            binding_id: receiver.id,
            paste_disposition: Some(disposition),
        });
    }
    if let Some(current) = bridge
        .receiver
        .lock()
        .await
        .as_mut()
        .filter(|current| current.id == receiver.id)
    {
        current.last_paste = Some(count);
    }
    Ok(DesktopAttachmentResult {
        binding_id: receiver.id,
        paste_disposition: Some(disposition),
    })
}

async fn finish_paste(
    store: &SharedAttachments,
    snapshot_count: i64,
    values: &[NativeReference],
    current_count: impl std::future::Future<Output = Result<i64, &'static str>>,
    deliver: impl std::future::Future<Output = Result<(), String>>,
) -> Result<(), String> {
    let result = match current_count.await {
        Ok(current) if current == snapshot_count => deliver.await,
        Ok(_) => Err("剪贴板已变化，请重新粘贴".into()),
        Err(error) => Err(error.into()),
    };
    if let Err(error) = &result {
        if !values.is_empty() {
            release(store, values.to_vec())
                .await
                .map_err(|cleanup| format!("{error}；{cleanup}"))?;
        }
    }
    result
}

async fn deliver(
    host: &HostBridge,
    state: &DesktopState,
    remote: &RemoteConnections,
    bridge: &CodexDesktopBridge,
    runtime: &BridgeRuntime,
    receiver: &Receiver,
    values: Vec<NativeReference>,
) -> Result<(), String> {
    context(host, bridge, remote, runtime, receiver).await?;
    verify_thread(state, &receiver.thread).await?;
    context(host, bridge, remote, runtime, receiver).await?;
    let mut current = bridge.receiver.lock().await;
    let current = current
        .as_mut()
        .filter(|current| current.id == receiver.id)
        .ok_or("附件接收区已变化")?;
    let mut grants = bridge.grants.lock().await;
    let existing = current
        .handles
        .iter()
        .filter_map(|handle| {
            match grants
                .resolve(receiver.revision, Some(&receiver.thread), handle)
                .ok()?
            {
                GrantValue::Reference(reference) => Some(reference),
                _ => None,
            }
        })
        .collect::<Vec<_>>();
    let values = values
        .into_iter()
        .filter(|value| !existing.iter().any(|old| old.same_selected_file(value)))
        .collect::<Vec<_>>();
    if values.is_empty() {
        return Ok(());
    }
    if current.handles.len() + values.len() > super::clipboard::MAX_FILES {
        return Err("引用数量超出限制，本次附件未添加".into());
    }
    let items = values
        .into_iter()
        .map(|reference| {
            let name = match &reference {
                NativeReference::File { name, .. } => name.clone(),
                _ => unreachable!(),
            };
            let kind = reference.kind().to_owned();
            let handle = grants.insert(
                receiver.revision,
                Some(receiver.thread.clone()),
                GrantValue::Reference(reference),
            );
            DesktopChosenItem { handle, name, kind }
        })
        .collect::<Vec<_>>();
    if !receiver.stream.send_value(&DesktopAttachmentEvent {
        binding_id: receiver.id.clone(),
        items: items.clone(),
        error: None,
    }) {
        return Err("附件接收区已关闭".to_string());
    }
    for item in items {
        if !current.handles.contains(&item.handle) {
            current.handles.push(item.handle);
        }
    }
    Ok(())
}

fn capture_drop(paths: Vec<PathBuf>, remaining: usize) -> Result<Vec<NativeReference>, String> {
    if paths.is_empty() || paths.len() > remaining.min(super::clipboard::MAX_FILES) {
        return Err("引用数量超出限制，本次附件未添加".into());
    }
    super::clipboard::selected_files(paths).map_err(str::to_owned)
}

pub(crate) async fn drop_files(
    host: Arc<HostBridge>,
    state: DesktopState,
    remote: RemoteConnections,
    bridge: Arc<CodexDesktopBridge>,
    paths: Vec<PathBuf>,
    logical_x: f64,
    logical_y: f64,
) {
    if !visible(&host) {
        return;
    }
    let Some(receiver) = bridge.receiver.lock().await.clone() else {
        return;
    };
    if !contains(&receiver.bounds, logical_x, logical_y) {
        return;
    }
    let result = async {
        let _serial = bridge.importing.lock().await;
        let _guard = remote.begin_local_write(receiver.revision)?;
        let runtime = bridge
            .runtime
            .lock()
            .await
            .clone()
            .ok_or("请先连接 Codex 桌面")?;
        context(&host, &bridge, &remote, &runtime, &receiver).await?;
        let remaining = super::clipboard::MAX_FILES.saturating_sub(receiver.handles.len());
        let values =
            agenthub_core::read_tasks::spawn_blocking(move || capture_drop(paths, remaining))
                .await
                .map_err(|_| "无法读取拖入附件")??;
        deliver(&host, &state, &remote, &bridge, &runtime, &receiver, values).await
    }
    .await;
    if let Err(error) = result {
        if bridge
            .receiver
            .lock()
            .await
            .as_ref()
            .is_some_and(|current| current.id == receiver.id)
        {
            let _ = receiver.stream.send_value(&DesktopAttachmentEvent {
                binding_id: receiver.id,
                items: Vec::new(),
                error: Some(error),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn receiver_bounds_are_finite_and_drop_is_inside_the_composer() {
        let mut bounds = DesktopAttachmentBounds {
            x: 10.,
            y: 20.,
            width: 100.,
            height: 40.,
        };
        assert!(valid_bounds(&bounds));
        assert!(contains(&bounds, 10., 20.));
        assert!(!contains(&bounds, 110., 20.));
        assert!(!contains(&bounds, 10., 60.));
        assert!(!contains(&bounds, f64::NAN, 20.));
        bounds.width = f64::INFINITY;
        assert!(!valid_bounds(&bounds));
        bounds.width = 0.;
        assert!(!valid_bounds(&bounds));
    }

    #[test]
    fn native_drop_batch_has_no_partial_grants_and_rechecks_identity() {
        let temporary = tempfile::tempdir().unwrap();
        let file = temporary.path().join("fixture.txt");
        std::fs::write(&file, "fixture").unwrap();
        assert!(capture_drop(vec![file.clone(), temporary.path().into()], 32).is_err());
        assert!(capture_drop(vec![file.clone()], 0).is_err());
        assert!(capture_drop(vec![file.clone(); 33], 32).is_err());
        let captured = capture_drop(vec![file.clone()], 1).unwrap();
        let replacement = temporary.path().join("replacement.txt");
        std::fs::write(&replacement, "changed").unwrap();
        std::fs::rename(replacement, file).unwrap();
        assert!(captured[0].validate().is_err());
    }

    #[test]
    fn attachment_contract_rejects_client_paths_bytes_and_unknown_actions() {
        for action in [
            serde_json::json!({"type":"paste","bindingId":"fixture","path":"/fixture/file.png"}),
            serde_json::json!({"type":"paste","bindingId":"fixture","bytes":[0,1]}),
            serde_json::json!({"type":"drop","paths":["/fixture/file.png"]}),
        ] {
            assert!(serde_json::from_value::<DesktopAttachmentRequest>(
                serde_json::json!({"revision":1,"sessionId":"fixture","action":action})
            )
            .is_err());
        }
        let event = DesktopAttachmentEvent {
            binding_id: "fixture".into(),
            error: None,
            items: vec![DesktopChosenItem {
                handle: "opaque".into(),
                name: "fixture.png".into(),
                kind: "image".into(),
            }],
        };
        let value = serde_json::to_value(event).unwrap();
        assert_eq!(value["items"][0].as_object().unwrap().len(), 3);
        assert!(value["items"][0].get("path").is_none());
    }

    #[test]
    fn untyped_event_listen_actions_are_rejected_by_attachment_contract() {
        assert!(serde_json::from_value::<DesktopAttachmentRequest>(
            serde_json::json!({"revision":1,"sessionId":"fixture","action":{"type":"listen"}})
        )
        .is_err());
    }
}

#[cfg(test)]
mod paste_completion_tests {
    use super::*;

    #[tokio::test]
    async fn failed_clipboard_recheck_or_lost_receiver_releases_only_undelivered_imports() {
        let directory = tempfile::tempdir().unwrap();
        let key = SecretBox::from_key_material(&"11".repeat(32)).unwrap();
        let store = Arc::new(std::sync::Mutex::new(
            AttachmentStore::open(directory.path().join("attachments"), key, 1).unwrap(),
        ));
        let import = || {
            store
                .lock()
                .unwrap()
                .import_png(b"\x89PNG\r\n\x1a\nfixture", 1)
                .unwrap()
        };
        let path = |reference: &NativeReference| match reference {
            NativeReference::File { path, .. } => path.clone(),
            _ => panic!("expected imported PNG"),
        };
        let delivered = import();
        finish_paste(
            &store,
            4,
            std::slice::from_ref(&delivered),
            async { Ok(4) },
            async { Ok(()) },
        )
        .await
        .unwrap();
        for (count, receiver) in [
            (Ok(5), Ok(())),
            (Err("timeout"), Ok(())),
            (Ok(4), Err("focus")),
            (Ok(4), Err("receiver")),
        ] {
            let pending = import();
            let error = finish_paste(
                &store,
                4,
                std::slice::from_ref(&pending),
                async { count },
                async { receiver.map_err(str::to_owned) },
            )
            .await
            .unwrap_err();
            assert_eq!(
                error,
                match count {
                    Ok(5) => "剪贴板已变化，请重新粘贴",
                    Err(reason) => reason,
                    _ => receiver.unwrap_err(),
                }
            );
            assert!(!path(&pending).exists());
            assert!(path(&delivered).is_file());
        }
    }
}
