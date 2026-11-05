//! The only desktop mutation entrypoint. It is not registered with webd or the
//! remote bridge. Paths and credentials originate in Rust/native choosers.
use crate::{overview::DesktopState, remote::RemoteConnections};
use agenthub_core::{
    codex::{
        self,
        desktop::{helper::DesktopHelper, worker::DesktopWorker, DesktopError, DesktopSession},
    },
    crypto::SecretBox,
    db::desktop_submissions::{DesktopLedger, DesktopWorkerLease},
    resource_limits::CREDENTIAL_WAIT,
    services::{codex_desktop::*, credentials::CredentialAccess},
};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Weak,
    },
};
use tauri::{
    ipc::{Channel, JavaScriptChannelId},
    State,
};
use tauri_plugin_opener::OpenerExt;
use tokio::sync::Mutex;

mod attachments;
mod catalogs;
mod clipboard;
mod diagnostics;
mod selection;
use selection::{GrantValue, Grants};

const KEY_SERVICE: &str = "com.lich13.agenthub.codex-desktop";
const KEY_ACCOUNT: &str = "submission-encryption-v1";
const MAX_WORKERS: usize = 4;

#[tauri::command(rename = "codex.desktop.diagnostics")]
pub async fn diagnostics(
    window: tauri::WebviewWindow,
    remote: State<'_, RemoteConnections>,
    request: DesktopDiagnosticsRequest,
) -> Result<DesktopDiagnostics, String> {
    let _guard = if request.copy == Some(true) {
        Some(remote.begin_local_write(request.revision)?)
    } else {
        None
    };
    remote.require_local(request.revision)?;
    let result = diagnostics::metadata().await;
    remote.require_local(request.revision)?;
    if request.copy == Some(true) {
        if window.label() != "main" || !window.is_focused().unwrap_or(false) {
            return Err("请在 AgentHub 中重试复制诊断".into());
        }
        clipboard::copy_diagnostics(&result).map_err(str::to_owned)?;
    }
    Ok(result)
}

pub struct CodexDesktopBridge {
    runtime: Mutex<Option<Arc<BridgeRuntime>>>,
    credentials: CredentialAccess,
    intent: AtomicU64,
    grants: Mutex<Grants>,
    catalogs: catalogs::Catalogs,
    receiver: Mutex<Option<attachments::Receiver>>,
    importing: Mutex<()>,
}

struct BridgeRuntime {
    _lease: Arc<DesktopWorkerLease>,
    ledger: DesktopLedger,
    codex_home: PathBuf,
    source: String,
    workers: Mutex<HashMap<String, DesktopWorker>>,
    connecting: Mutex<HashMap<String, Weak<Mutex<()>>>>,
    selection: Mutex<Option<Selection>>,
    attachments: Arc<std::sync::Mutex<agenthub_core::codex::desktop::attachments::AttachmentStore>>,
}

struct Selection {
    id: String,
    worker: DesktopWorker,
    listener: tokio::task::AbortHandle,
}

impl CodexDesktopBridge {
    pub fn new(credentials: CredentialAccess) -> Self {
        Self {
            runtime: Mutex::new(None),
            credentials,
            intent: AtomicU64::new(0),
            grants: Mutex::new(Grants::default()),
            catalogs: catalogs::Catalogs::default(),
            receiver: Mutex::new(None),
            importing: Mutex::new(()),
        }
    }

    async fn runtime(&self, state: &DesktopState) -> Result<Arc<BridgeRuntime>, String> {
        let mut slot = self.runtime.lock().await;
        if let Some(runtime) = slot.as_ref() {
            return Ok(runtime.clone());
        }
        let db = state.db.clone();
        let attachment_root =
            agenthub_core::codex::desktop::attachments::root_for_database(db.path());
        let existing_root = attachment_root.clone();
        let home = state.codex_paths().home;
        let (lease, home, existing) = agenthub_core::read_tasks::spawn_blocking(move || {
            let lease = DesktopLedger::lock_database(&db)
                .map_err(|_| "另一个 AgentHub 正在使用本机发送队列")?;
            let existing = DesktopLedger::has_encrypted_messages(&db)
                .map_err(|_| "无法读取本机发送队列")?
                || match std::fs::symlink_metadata(existing_root) {
                    Ok(_) => true,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
                    Err(_) => return Err("无法核对粘贴附件存储".into()),
                };
            let home = home.canonicalize().map_err(|_| "无法定位本机 Codex")?;
            Ok::<_, String>((lease, home, existing))
        })
        .await
        .map_err(|_| "无法初始化本机发送队列")??;
        // The cross-process lease moves INTO the credential transaction: a
        // cancelled caller cannot release it while Keychain is still writing.
        let (crypto, lease) = self
            .credentials
            .transaction(CREDENTIAL_WAIT, move || {
                let entry = keyring::Entry::new(KEY_SERVICE, KEY_ACCOUNT)
                    .map_err(|_| "无法访问发送队列钥匙串")?;
                let material = match entry.get_password() {
                    Ok(value) => value,
                    Err(keyring::Error::NoEntry) if !existing => {
                        let value = SecretBox::random_key_material();
                        entry
                            .set_password(&value)
                            .map_err(|_| "无法保存发送队列密钥")?;
                        value
                    }
                    Err(keyring::Error::NoEntry) => {
                        return Err("发送队列密钥缺失，已保留待发送记录".to_owned())
                    }
                    Err(_) => return Err("无法读取发送队列密钥，请检查系统授权".to_owned()),
                };
                let crypto =
                    SecretBox::from_key_material(&material).map_err(|_| "发送队列密钥不可用")?;
                Ok::<_, String>((crypto, lease))
            })
            .await
            .map_err(|_| "钥匙串未及时响应，请检查系统授权")??;
        let source = agenthub_core::security::hash_token(&home.to_string_lossy());
        let attachment_crypto = crypto.clone();
        let (attachments, lease) = agenthub_core::read_tasks::spawn_blocking(move || {
            let attachments = agenthub_core::codex::desktop::attachments::AttachmentStore::open(
                attachment_root,
                attachment_crypto,
                chrono::Utc::now().timestamp().max(0) as u64,
            )?;
            Ok::<_, &'static str>((attachments, Arc::new(lease)))
        })
        .await
        .map_err(|_| "无法初始化粘贴附件存储")??;
        let ledger = DesktopLedger::new(state.db.clone(), crypto);
        let recovery = ledger.clone();
        let recovery_source = source.clone();
        let recovery_lease = lease.clone();
        agenthub_core::read_tasks::spawn_blocking(move || {
            let _lease = recovery_lease;
            recovery.recover(&recovery_source)
        })
        .await
        .map_err(|_| "无法恢复本机发送队列")?
        .map_err(|_| "无法恢复本机发送队列")?;
        let runtime = Arc::new(BridgeRuntime {
            _lease: lease,
            ledger,
            codex_home: home,
            source,
            workers: Mutex::new(HashMap::new()),
            connecting: Mutex::new(HashMap::new()),
            selection: Mutex::new(None),
            attachments: Arc::new(std::sync::Mutex::new(attachments)),
        });
        *slot = Some(runtime.clone());
        Ok(runtime)
    }
}

impl BridgeRuntime {
    async fn attach(
        &self,
        thread_id: &str,
        confirm_owner: Option<bool>,
    ) -> Result<DesktopWorker, String> {
        // Per-thread admission shares attachment work without holding the global
        // worker registry over socket I/O. Weak locks do not retain old threads.
        let admission = {
            let mut connecting = self.connecting.lock().await;
            connecting.retain(|_, gate| gate.strong_count() > 0);
            if let Some(gate) = connecting.get(thread_id).and_then(Weak::upgrade) {
                gate
            } else {
                if connecting.len() >= MAX_WORKERS {
                    return Err("本机协作连接繁忙，请稍后重试".into());
                }
                let gate = Arc::new(Mutex::new(()));
                connecting.insert(thread_id.into(), Arc::downgrade(&gate));
                gate
            }
        };
        let _admission = admission.lock().await;
        let existing = {
            let mut workers = self.workers.lock().await;
            if workers.get(thread_id).is_some_and(DesktopWorker::is_closed)
                && confirm_owner != Some(true)
            {
                return Err("桌面连接已变化，请点击连接确认".into());
            }
            workers.retain(|_, worker| !worker.is_closed());
            workers.get(thread_id).cloned()
        };
        if let Some(worker) = existing {
            if confirm_owner == Some(true) {
                worker.confirm_owner().await.map_err(desktop_error)?;
            }
            return Ok(worker);
        }
        if confirm_owner == Some(false) {
            return Err("桌面连接已变化，请点击连接确认".into());
        }
        let session = DesktopSession::attach(&self.codex_home, thread_id)
            .await
            .map_err(desktop_error)?;
        session.state().check_writable().map_err(desktop_error)?;
        let mut workers = self.workers.lock().await;
        if workers.len() >= MAX_WORKERS {
            let selected = self
                .selection
                .lock()
                .await
                .as_ref()
                .map(|selection| selection.worker.current().thread_id);
            let stale = workers
                .iter()
                .find(|(key, worker)| {
                    let current = worker.current();
                    selected.as_ref() != Some(*key)
                        && current.runtime == "idle"
                        && current.queue.is_empty()
                })
                .map(|(key, _)| key.clone());
            if let Some(stale) = stale {
                workers.remove(&stale);
            } else {
                return Err("本机协作线程已达到上限，请稍后重试".into());
            }
        }
        let worker = DesktopWorker::spawn_checked(
            session,
            self.ledger.clone(),
            self.source.clone(),
            codex::CodexPaths::new(self.codex_home.clone()),
        )
        .await
        .map_err(desktop_error)?;
        workers.insert(thread_id.into(), worker.clone());
        Ok(worker)
    }

    async fn worker(&self, session_id: &str) -> Result<DesktopWorker, String> {
        self.selection
            .lock()
            .await
            .as_ref()
            .filter(|selection| {
                session_token(&selection.id, &selection.worker.current().session_id) == session_id
                    && !selection.worker.is_closed()
            })
            .map(|selection| selection.worker.clone())
            .ok_or_else(|| "桌面连接已变化，请重新连接".into())
    }
}

fn session_token(subscription: &str, epoch: &str) -> String {
    format!("{subscription}.{epoch}")
}

pub(crate) fn desktop_error(error: DesktopError) -> String {
    agenthub_core::codex::desktop::diagnostics::record(
        agenthub_core::codex::desktop::diagnostics::Source::Local,
        "connect",
        None,
        None,
        None,
        error,
    );
    error.user_message().into()
}

async fn verify_thread(state: &DesktopState, thread_id: &str) -> Result<(), String> {
    let paths = state.codex_paths();
    let id = thread_id.to_owned();
    agenthub_core::read_tasks::spawn_blocking(move || {
        let summary = codex::local_thread_summary(&paths, &id)
            .map_err(|_| "无法核对本机线程")?
            .ok_or("此线程当前不可用于桌面协作")?;
        if matches!(summary.status, codex::ThreadStatus::Archived) {
            return Err("归档线程仅支持阅读");
        }
        Ok(())
    })
    .await
    .map_err(|_| "无法核对本机线程")?
    .map_err(str::to_owned)
}

async fn await_owner<T, F: std::future::Future<Output = Result<T, String>>>(
    mut connect: impl FnMut() -> F,
) -> Result<T, String> {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(20);
    tokio::time::timeout_at(deadline, async {
        loop {
            // Tokio may poll the inner future at the deadline before polling
            // the timer; never start one more connection attempt at 20 seconds.
            if tokio::time::Instant::now() >= deadline {
                return Err(desktop_error(DesktopError::OwnerUnavailable));
            }
            match connect().await {
                Ok(value) => return Ok(value),
                Err(error)
                    if [
                        DesktopError::OwnerUnavailable,
                        DesktopError::Unavailable,
                        DesktopError::Disconnected,
                    ]
                    .iter()
                    .any(|kind| error == kind.user_message()) =>
                {
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                }
                Err(error) => return Err(error),
            }
        }
    })
    .await
    .map_err(|_| desktop_error(DesktopError::OwnerUnavailable))?
}

#[tauri::command(rename = "codex.desktop.attach")]
pub async fn attach(
    state: State<'_, DesktopState>,
    remote: State<'_, RemoteConnections>,
    bridge: State<'_, CodexDesktopBridge>,
    request: DesktopThreadRequest,
    on_event: Channel<DesktopView>,
) -> Result<DesktopView, String> {
    let intent = bridge.intent.fetch_add(1, Ordering::SeqCst).wrapping_add(1);
    remote.require_local(request.revision)?;
    verify_thread(&state, &request.thread_id).await?;
    let runtime = bridge.runtime(&state).await?;
    remote.require_local(request.revision)?;
    let (worker, metadata) = tokio::join!(
        await_owner(|| runtime.attach(&request.thread_id, request.confirm_owner)),
        diagnostics::metadata(),
    );
    let worker = worker?;
    let desktop_build = metadata.desktop_build;
    remote.require_local(request.revision)?;
    if bridge.intent.load(Ordering::SeqCst) != intent {
        return Err("所选线程已变化，已丢弃旧连接".into());
    }
    let token = agenthub_core::security::hash_token(&SecretBox::random_key_material());
    let mut updates = worker.subscribe();
    // Read the initial value from this subscription so a change between the
    // snapshot and subscription cannot be missed while native Codex is idle.
    let mut initial = updates.borrow_and_update().clone();
    initial.session_id = session_token(&token, &initial.session_id);
    initial.desktop_build = desktop_build.clone();
    let connection = remote.inner().clone();
    let event_token = token.clone();
    let listener = tokio::spawn(async move {
        while updates.changed().await.is_ok() {
            // Coalesce token patches into at most ten full projected views per
            // second; the native state actor keeps processing every revision.
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            if connection.require_local(request.revision).is_err() {
                break;
            }
            let mut value = updates.borrow_and_update().clone();
            value.session_id = session_token(&event_token, &value.session_id);
            value.desktop_build = desktop_build.clone();
            if on_event.send(value).is_err() {
                break;
            }
        }
    })
    .abort_handle();
    let mut selection = runtime.selection.lock().await;
    if bridge.intent.load(Ordering::SeqCst) != intent {
        listener.abort();
        return Err("所选线程已变化，已丢弃旧连接".into());
    }
    if let Some(previous) = selection.replace(Selection {
        id: token,
        worker,
        listener,
    }) {
        previous.listener.abort();
    }
    Ok(initial)
}

#[tauri::command(rename = "codex.desktop.action")]
pub async fn action(
    state: State<'_, DesktopState>,
    remote: State<'_, RemoteConnections>,
    bridge: State<'_, CodexDesktopBridge>,
    request: DesktopActionRequest,
) -> Result<DesktopView, String> {
    let _guard = remote.begin_local_write(request.revision)?;
    let intent = bridge.intent.load(Ordering::SeqCst);
    let runtime = bridge.runtime(&state).await?;
    remote.require_local(request.revision)?;
    let worker = runtime.worker(&request.session_id).await?;
    let thread = worker.current().thread_id;
    verify_thread(&state, &thread).await?;
    let handles = match &request.action {
        DesktopAction::Send { references, .. } | DesktopAction::Steer { references, .. } => {
            references.as_slice()
        }
        _ => &[],
    };
    if handles.len() > 32 {
        return Err("引用数量超出限制".into());
    }
    let references = {
        let grants = bridge.grants.lock().await;
        handles
            .iter()
            .map(
                |handle| match grants.resolve(request.revision, Some(&thread), handle)? {
                    GrantValue::Reference(reference) => Ok(reference),
                    _ => Err("所选引用不可用于发送".into()),
                },
            )
            .collect::<Result<Vec<_>, String>>()?
    };
    let attachments = runtime.attachments.clone();
    let references = agenthub_core::read_tasks::spawn_blocking(move || {
        // The same mutex serializes draft cleanup and handoff retention. A file
        // cannot be released between capture and the durable retention marker.
        let mut store = attachments.lock().map_err(|_| "无法核对粘贴附件")?;
        let references = references
            .into_iter()
            .map(|reference| reference.capture_detailed())
            .collect::<Result<Vec<_>, _>>()
            .map_err(|(_, message)| message)?;
        store.retain_before_send(&references)?;
        Ok::<_, &'static str>(references)
    })
    .await
    .map_err(|_| "无法核对所选引用")?
    .map_err(str::to_owned)?;
    if let DesktopAction::Settings {
        model,
        reasoning_effort,
        mode,
    } = &request.action
    {
        let cwd = worker.project_directory().ok_or("无法确认当前工作目录")?;
        let helper = DesktopHelper::installed(state.codex_paths().home)
            .await
            .map_err(desktop_error)?;
        for kind in ["models", "modes"] {
            bridge
                .catalogs
                .read(
                    &helper,
                    cwd,
                    &thread,
                    &DesktopCatalogRequest {
                        force_refetch: None,
                        revision: request.revision,
                        session_id: Some(request.session_id.clone()),
                        project_handle: None,
                        kind: kind.into(),
                        query: String::new(),
                        cursor: None,
                    },
                    || require_catalog_context(&remote, &bridge, request.revision, intent),
                )
                .await?;
        }
        bridge
            .catalogs
            .validate_settings(
                request.revision,
                &thread,
                model,
                reasoning_effort.as_deref(),
                *mode,
            )
            .await?;
    }
    remote.require_local(request.revision)?;
    let worker = runtime.worker(&request.session_id).await?;
    let mut view = worker
        .act_in_session(
            request.action,
            references,
            request
                .session_id
                .rsplit_once('.')
                .ok_or("桌面连接已变化")?
                .1
                .to_owned(),
        )
        .await
        .map_err(desktop_error)?;
    view.session_id = request.session_id;
    Ok(view)
}

#[tauri::command(rename = "codex.desktop.detach")]
pub async fn detach(
    bridge: State<'_, CodexDesktopBridge>,
    request: DesktopSessionRequest,
) -> Result<DesktopAck, String> {
    let receiver = bridge.receiver.lock().await.clone();
    if let Some(receiver) = receiver.filter(|receiver| receiver.session() == request.session_id) {
        attachments::unbind(&bridge, receiver.id()).await;
    }
    // Releasing an old callback is always safe, including after machine change.
    // Only the matching subscription is revoked; queued user work remains owned
    // by its original local worker and cannot migrate to a remote machine.
    if let Some(runtime) = bridge.runtime.lock().await.as_ref() {
        let mut selection = runtime.selection.lock().await;
        if selection.as_ref().is_some_and(|selection| {
            request
                .session_id
                .starts_with(&format!("{}.", selection.id))
        }) {
            if let Some(selection) = selection.take() {
                selection.listener.abort();
            }
            bridge.intent.fetch_add(1, Ordering::SeqCst);
        }
    }
    Ok(DesktopAck { ok: true })
}

#[tauri::command(rename = "codex.desktop.attachments")]
pub async fn attachments(
    window: tauri::WebviewWindow,
    state: State<'_, DesktopState>,
    remote: State<'_, RemoteConnections>,
    bridge: State<'_, CodexDesktopBridge>,
    request: DesktopAttachmentRequest,
    on_event: JavaScriptChannelId,
) -> Result<DesktopAttachmentResult, String> {
    attachments::handle(window, &state, &remote, &bridge, request, on_event).await
}

pub(crate) fn native_drop(
    window: tauri::WebviewWindow,
    paths: Vec<PathBuf>,
    position: tauri::PhysicalPosition<f64>,
) {
    attachments::drop_files(window, paths, position);
}

#[tauri::command(rename = "codex.desktop.open")]
pub async fn open(
    app: tauri::AppHandle,
    state: State<'_, DesktopState>,
    remote: State<'_, RemoteConnections>,
    request: DesktopThreadRequest,
) -> Result<DesktopAck, String> {
    let _guard = remote.begin_local_write(request.revision)?;
    verify_thread(&state, &request.thread_id).await?;
    app.opener()
        .open_url(
            format!("codex://threads/{}", request.thread_id),
            None::<&str>,
        )
        .map_err(|_| "无法打开 Codex 桌面")?;
    Ok(DesktopAck { ok: true })
}

#[tauri::command(rename = "codex.desktop.choose")]
pub async fn choose(
    remote: State<'_, RemoteConnections>,
    bridge: State<'_, CodexDesktopBridge>,
    request: DesktopChooseRequest,
) -> Result<DesktopChoice, String> {
    let _guard = remote.begin_local_write(request.revision)?;
    let directory = match request.kind.as_str() {
        "project" => true,
        "file" => false,
        _ => return Err("选择类型无效".into()),
    };
    let context = if directory {
        None
    } else {
        let runtime = bridge
            .runtime
            .lock()
            .await
            .clone()
            .ok_or("请先连接 Codex 桌面")?;
        let id = request.session_id.as_deref().ok_or("请先连接 Codex 桌面")?;
        let worker = runtime.worker(id).await?;
        Some((runtime, id.to_owned(), worker.current().thread_id))
    };
    let Some(path) = selection::choose(directory).await? else {
        return Ok(DesktopChoice { item: None });
    };
    if let Some((runtime, id, _)) = &context {
        runtime.worker(id).await?;
    }
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("所选目录")
        .to_owned();
    let value = agenthub_core::read_tasks::spawn_blocking(move || {
        if directory {
            selection::project(path)
        } else {
            agenthub_core::codex::desktop::references::NativeReference::selected_file_detailed(path)
                .map(GrantValue::Reference)
                .map_err(|(_, message)| message.to_owned())
        }
    })
    .await
    .map_err(|_| "无法读取所选项目")??;
    if let Some((runtime, id, _)) = &context {
        runtime.worker(id).await?;
    }
    remote.require_local(request.revision)?;
    let kind = match &value {
        GrantValue::Reference(reference) => reference.kind(),
        _ => "project",
    }
    .to_owned();
    let scope = context.map(|(_, _, thread)| thread);
    let handle = bridge
        .grants
        .lock()
        .await
        .insert(request.revision, scope, value);
    Ok(DesktopChoice {
        item: Some(DesktopChosenItem { handle, name, kind }),
    })
}

#[tauri::command(rename = "codex.desktop.create")]
pub async fn create(
    app: tauri::AppHandle,
    state: State<'_, DesktopState>,
    remote: State<'_, RemoteConnections>,
    bridge: State<'_, CodexDesktopBridge>,
    request: DesktopCreateRequest,
) -> Result<DesktopCreatedThread, String> {
    let _guard = remote.begin_local_write(request.revision)?;
    let value =
        bridge
            .grants
            .lock()
            .await
            .resolve(request.revision, None, &request.project_handle)?;
    let cwd = agenthub_core::read_tasks::spawn_blocking(move || selection::project_path(value))
        .await
        .map_err(|_| "无法读取所选目录")??;
    let helper = DesktopHelper::installed(state.codex_paths().home)
        .await
        .map_err(desktop_error)?;
    let thread_id = helper.create_empty(&cwd).await.map_err(desktop_error)?;
    // Creation is already committed by the native helper. Failure to open a
    // window must not turn this into a retryable creation failure.
    let _ = app
        .opener()
        .open_url(format!("codex://threads/{thread_id}"), None::<&str>);
    Ok(DesktopCreatedThread { thread_id })
}

fn require_catalog_context(
    remote: &RemoteConnections,
    bridge: &CodexDesktopBridge,
    revision: u64,
    intent: u64,
) -> Result<(), String> {
    remote.require_local(revision)?;
    if bridge.intent.load(Ordering::SeqCst) != intent {
        return Err("所选线程已变化，目录结果已丢弃".into());
    }
    Ok(())
}

#[tauri::command(rename = "codex.desktop.catalog")]
pub async fn catalog(
    state: State<'_, DesktopState>,
    remote: State<'_, RemoteConnections>,
    bridge: State<'_, CodexDesktopBridge>,
    request: DesktopCatalogRequest,
) -> Result<DesktopCatalog, String> {
    remote.require_local(request.revision)?;
    let intent = bridge.intent.load(Ordering::SeqCst);
    if request.query.len() > 256 {
        return Err("搜索内容超出限制".into());
    }
    let runtime = bridge
        .runtime
        .lock()
        .await
        .clone()
        .ok_or("请先连接 Codex 桌面")?;
    let session_id = request.session_id.as_deref().ok_or("请先连接 Codex 桌面")?;
    let worker = runtime.worker(session_id).await?;
    let cwd = worker
        .project_directory()
        .ok_or("无法确认当前工作目录")?
        .to_owned();
    let thread = worker.current().thread_id;
    let helper = DesktopHelper::installed(state.codex_paths().home)
        .await
        .map_err(desktop_error)?;
    // Cache and grant locks never span the serial native helper operation.
    let page = bridge
        .catalogs
        .read(&helper, &cwd, &thread, &request, || {
            require_catalog_context(&remote, &bridge, request.revision, intent)
        })
        .await?;
    remote.require_local(request.revision)?;
    runtime.worker(session_id).await?;
    Ok(page.project(request.revision, &thread, &mut *bridge.grants.lock().await))
}

#[cfg(test)]
#[path = "codex_desktop/connection_tests.rs"]
mod connection_tests;
