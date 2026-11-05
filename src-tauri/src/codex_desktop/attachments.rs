//! OS attachment ingress. A receiver is scoped to one visible main-window
//! composer, machine revision and desktop session. Channels never carry paths.
use super::*;
use agenthub_core::codex::desktop::references::NativeReference;
use tauri::{Manager, WebviewWindow};

#[derive(Clone)]
pub(super) struct Receiver {
    id: String,
    revision: u64,
    intent: u64,
    session: String,
    thread: String,
    bounds: DesktopAttachmentBounds,
    handles: Vec<String>,
    last_paste: Option<isize>,
    channel: Channel<DesktopAttachmentEvent>,
}
impl Receiver {
    pub(super) fn session(&self) -> &str {
        &self.session
    }
    pub(super) fn id(&self) -> &str {
        &self.id
    }
}

fn visible(window: &WebviewWindow) -> bool {
    window.label() == "main"
        && window.is_visible().unwrap_or(false)
        && window.is_focused().unwrap_or(false)
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
    window: &WebviewWindow,
    bridge: &CodexDesktopBridge,
    remote: &RemoteConnections,
    runtime: &BridgeRuntime,
    receiver: &Receiver,
) -> Result<(), String> {
    if !visible(window) || bridge.intent.load(Ordering::SeqCst) != receiver.intent {
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
    if handles.len() > 32 {
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

async fn release(runtime: &BridgeRuntime, references: Vec<NativeReference>) -> Result<(), String> {
    let store = runtime.attachments.clone();
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
        current.take();
    }
    // Hiding/reconnecting preserves the visible draft for its existing 30 min
    // grant lifetime. Explicit chip removal releases; expired drafts are GC'd.
}

pub(super) async fn handle(
    window: WebviewWindow,
    state: &DesktopState,
    remote: &RemoteConnections,
    bridge: &CodexDesktopBridge,
    request: DesktopAttachmentRequest,
    channel: JavaScriptChannelId,
) -> Result<DesktopAttachmentResult, String> {
    if window.label() != "main" {
        return Err("附件仅可从主窗口添加".into());
    }
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
    if !visible(&window) || !view.connected || !view.writable {
        return Err("请在当前协作输入框中添加附件".into());
    }
    if matches!(
        &request.action,
        DesktopAttachmentAction::Bind { .. } | DesktopAttachmentAction::Paste { .. }
    ) {
        verify_thread(state, &view.thread_id).await?;
    }
    if let DesktopAttachmentAction::Bind { bounds, handles } = request.action {
        if !valid_bounds(&bounds) {
            return Err("附件接收区域无效".into());
        }
        // Repeated commands carry the same JS callback ID. Deserializing each
        // one as Channel would create a new owner whose Drop sends `end` and
        // closes the live receiver during its first bounds update. Only Bind
        // owns a channel; other actions keep using the stored receiver.
        let channel = channel.channel_on(window.as_ref().clone());
        let receiver = Receiver {
            id: agenthub_core::security::hash_token(&SecretBox::random_key_material()),
            revision: request.revision,
            intent: bridge.intent.load(Ordering::SeqCst),
            session: request.session_id.clone(),
            thread: view.thread_id,
            bounds,
            handles,
            last_paste: None,
            channel,
        };
        references(bridge, &receiver, &receiver.handles).await?;
        remote.require_local(request.revision)?;
        runtime.worker(&request.session_id).await?;
        let id = receiver.id.clone();
        *bridge.receiver.lock().await = Some(receiver);
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
    context(&window, bridge, remote, &runtime, &receiver).await?;
    let mut paste_disposition = None;
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
        DesktopAttachmentAction::Paste { .. } => {
            let _serial = bridge.importing.lock().await;
            context(&window, bridge, remote, &runtime, &receiver).await?;
            let last = bridge
                .receiver
                .lock()
                .await
                .as_ref()
                .and_then(|current| current.last_paste);
            if last.is_some_and(super::clipboard::unchanged) {
                return Ok(DesktopAttachmentResult {
                    binding_id: receiver.id,
                    paste_disposition: Some(DesktopPasteDisposition::Attachments),
                });
            }
            let store = runtime.attachments.clone();
            let (count, disposition, values) =
                agenthub_core::read_tasks::spawn_blocking(move || {
                    let capture = super::clipboard::capture()?;
                    use super::clipboard::PasteContent;
                    let (disposition, values) = match capture.content {
                        PasteContent::Files(files) => (DesktopPasteDisposition::Attachments, files),
                        PasteContent::Image(png) => {
                            let file = store
                                .lock()
                                .map_err(|_| "无法保存粘贴图片")?
                                .import_png(&png, chrono::Utc::now().timestamp().max(0) as u64)?;
                            (DesktopPasteDisposition::Attachments, vec![file])
                        }
                        PasteContent::Text => (DesktopPasteDisposition::Text, Vec::new()),
                        PasteContent::Unsupported => {
                            (DesktopPasteDisposition::Unsupported, Vec::new())
                        }
                    };
                    Ok::<_, &'static str>((capture.change_count, disposition, values))
                })
                .await
                .map_err(|_| "无法读取剪贴板附件")??;
            paste_disposition = Some(disposition);
            if !super::clipboard::unchanged(count) {
                let _ = release(&runtime, values).await;
                return Err("剪贴板已变化，请重新粘贴".into());
            }
            if values.is_empty() {
                context(&window, bridge, remote, &runtime, &receiver).await?;
                return Ok(DesktopAttachmentResult {
                    binding_id: receiver.id,
                    paste_disposition,
                });
            }
            if let Err(error) = deliver(
                &window,
                state,
                remote,
                bridge,
                &runtime,
                &receiver,
                values.clone(),
            )
            .await
            {
                let _ = release(&runtime, values).await;
                return Err(error);
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
        }
        DesktopAttachmentAction::Release { handles, .. } => {
            if handles
                .iter()
                .any(|handle| !receiver.handles.contains(handle))
            {
                return Err("附件授权已变化".into());
            }
            let values = references(bridge, &receiver, &handles).await?;
            release(&runtime, values).await?;
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
        _ => unreachable!(),
    }
    Ok(DesktopAttachmentResult {
        binding_id: receiver.id,
        paste_disposition,
    })
}

async fn deliver(
    window: &WebviewWindow,
    state: &DesktopState,
    remote: &RemoteConnections,
    bridge: &CodexDesktopBridge,
    runtime: &BridgeRuntime,
    receiver: &Receiver,
    values: Vec<NativeReference>,
) -> Result<(), String> {
    context(window, bridge, remote, runtime, receiver).await?;
    verify_thread(state, &receiver.thread).await?;
    context(window, bridge, remote, runtime, receiver).await?;
    let mut current = bridge.receiver.lock().await;
    let current = current
        .as_mut()
        .filter(|current| current.id == receiver.id)
        .ok_or("附件接收区已变化")?;
    // Validate the entire batch before issuing a single grant. Never silently
    // truncate a drop or admit only the valid prefix of an invalid batch.
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
    if current.handles.len() + values.len() > 32 {
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
    receiver
        .channel
        .send(DesktopAttachmentEvent {
            binding_id: receiver.id.clone(),
            items: items.clone(),
            error: None,
        })
        .map_err(|_| "附件接收区已关闭")?;
    for item in items {
        if !current.handles.contains(&item.handle) {
            current.handles.push(item.handle);
        }
    }
    Ok(())
}

fn capture_drop(paths: Vec<PathBuf>, remaining: usize) -> Result<Vec<NativeReference>, String> {
    if paths.is_empty() || paths.len() > remaining.min(32) {
        return Err("引用数量超出限制，本次附件未添加".into());
    }
    super::clipboard::selected_files(paths).map_err(str::to_owned)
}

pub(super) fn drop_files(
    window: WebviewWindow,
    paths: Vec<PathBuf>,
    position: tauri::PhysicalPosition<f64>,
) {
    // Called only by Tauri's native OS event, never by an RPC path parameter.
    if !visible(&window) {
        return;
    }
    let app = window.app_handle().clone();
    tauri::async_runtime::spawn(async move {
        let bridge = app.state::<CodexDesktopBridge>();
        let remote = app.state::<RemoteConnections>();
        let state = app.state::<DesktopState>();
        let Some(receiver) = bridge.receiver.lock().await.clone() else {
            return;
        };
        let scale = window.scale_factor().unwrap_or(1.0);
        if !contains(&receiver.bounds, position.x / scale, position.y / scale) {
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
            context(&window, &bridge, &remote, &runtime, &receiver).await?;
            let remaining = 32usize.saturating_sub(receiver.handles.len());
            let values =
                agenthub_core::read_tasks::spawn_blocking(move || capture_drop(paths, remaining))
                    .await
                    .map_err(|_| "无法读取拖入附件")??;
            deliver(
                &window, &state, &remote, &bridge, &runtime, &receiver, values,
            )
            .await
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
                let _ = receiver.channel.send(DesktopAttachmentEvent {
                    binding_id: receiver.id,
                    items: Vec::new(),
                    error: Some(error),
                });
            }
        }
    });
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
    fn raw_tauri_events_cannot_be_subscribed_from_the_webview() {
        let capabilities: serde_json::Value =
            serde_json::from_str(include_str!("../../capabilities/default.json")).unwrap();
        assert!(capabilities["permissions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == "core:event:deny-listen"));
    }
}
