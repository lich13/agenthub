use std::{
    collections::HashMap,
    ffi::c_char,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tokio::sync::{oneshot, Semaphore};

pub type AgentHubRpcCallback =
    Option<unsafe extern "C" fn(u64, bool, *const c_char, usize, *mut std::ffi::c_void)>;
pub type AgentHubStreamCallback =
    Option<unsafe extern "C" fn(u64, *const c_char, usize, bool, *mut std::ffi::c_void)>;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct AgentHubHostCallbacks {
    pub host_ctx: *mut std::ffi::c_void,
    pub open_external_url:
        Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char, usize)>,
    pub deliver_ci_notifications:
        Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char, usize)>,
    pub request_app_restart: Option<unsafe extern "C" fn(*mut std::ffi::c_void)>,
    pub begin_choose_local_path:
        Option<unsafe extern "C" fn(*mut std::ffi::c_void, u64, bool, *const c_char, usize)>,
    pub cancel_choose_local_path: Option<unsafe extern "C" fn(*mut std::ffi::c_void, u64)>,
    pub begin_pasteboard_change_count: Option<unsafe extern "C" fn(*mut std::ffi::c_void, u64)>,
    pub begin_write_pasteboard:
        Option<unsafe extern "C" fn(*mut std::ffi::c_void, u64, *const c_char, usize)>,
}

// SAFETY: Host callback pointers and opaque context are managed by the Swift
// host singleton and invoked without blocking on the caller thread.
unsafe impl Send for AgentHubHostCallbacks {}
unsafe impl Sync for AgentHubHostCallbacks {}

impl Default for AgentHubHostCallbacks {
    fn default() -> Self {
        Self {
            host_ctx: std::ptr::null_mut(),
            open_external_url: None,
            deliver_ci_notifications: None,
            request_app_restart: None,
            begin_choose_local_path: None,
            cancel_choose_local_path: None,
            begin_pasteboard_change_count: None,
            begin_write_pasteboard: None,
        }
    }
}

#[derive(Clone, Copy)]
pub struct RpcSink {
    request_id: u64,
    callback: AgentHubRpcCallback,
    ctx: usize,
}

impl RpcSink {
    pub fn new(request_id: u64, callback: AgentHubRpcCallback, ctx: *mut std::ffi::c_void) -> Self {
        Self {
            request_id,
            callback,
            ctx: ctx as usize,
        }
    }

    pub fn respond(self, result: Result<String, String>) {
        let Some(cb) = self.callback else {
            return;
        };
        let (ok, payload) = match result {
            Ok(json) => (true, json),
            Err(err) => (false, err),
        };
        let bytes = payload.as_bytes();
        let ptr = if bytes.is_empty() {
            std::ptr::null()
        } else {
            bytes.as_ptr().cast::<c_char>()
        };
        // SAFETY: buffer lives for the duration of the synchronous C callback.
        unsafe {
            cb(
                self.request_id,
                ok,
                ptr,
                bytes.len(),
                self.ctx as *mut std::ffi::c_void,
            );
        }
    }
}

struct StreamSinkInner {
    seq: AtomicU64,
    closed: AtomicBool,
    callback: AgentHubStreamCallback,
    ctx: usize,
}

#[derive(Clone)]
pub struct StreamSink(Arc<StreamSinkInner>);

impl StreamSink {
    pub fn new(callback: AgentHubStreamCallback, ctx: *mut std::ffi::c_void) -> Self {
        Self(Arc::new(StreamSinkInner {
            seq: AtomicU64::new(0),
            closed: AtomicBool::new(false),
            callback,
            ctx: ctx as usize,
        }))
    }

    pub fn send_value<T: serde::Serialize>(&self, value: &T) -> bool {
        if self.0.closed.load(Ordering::Acquire) {
            return false;
        }
        let Ok(json) = serde_json::to_string(value) else {
            return false;
        };
        self.send_json(&json)
    }

    pub fn send_json(&self, json: &str) -> bool {
        if self.0.closed.load(Ordering::Acquire) {
            return false;
        }
        let Some(cb) = self.0.callback else {
            return false;
        };
        let seq = self.0.seq.fetch_add(1, Ordering::SeqCst) + 1;
        if self.0.closed.load(Ordering::Acquire) {
            return false;
        }
        let bytes = json.as_bytes();
        let ptr = if bytes.is_empty() {
            std::ptr::null()
        } else {
            bytes.as_ptr().cast::<c_char>()
        };
        // SAFETY: synchronous frame borrow; terminal frame is sent separately once.
        unsafe {
            cb(
                seq,
                ptr,
                bytes.len(),
                false,
                self.0.ctx as *mut std::ffi::c_void,
            );
        }
        true
    }

    pub fn close(&self) {
        self.0.emit_close_once();
    }
}

impl StreamSinkInner {
    fn emit_close_once(&self) {
        if self.closed.swap(true, Ordering::AcqRel) {
            return;
        }
        if let Some(cb) = self.callback {
            let seq = self.seq.fetch_add(1, Ordering::SeqCst) + 1;
            // SAFETY: guaranteed single terminal invocation with null/0 payload.
            unsafe {
                cb(
                    seq,
                    std::ptr::null(),
                    0,
                    true,
                    self.ctx as *mut std::ffi::c_void,
                );
            }
        }
    }
}

impl Drop for StreamSinkInner {
    fn drop(&mut self) {
        self.emit_close_once();
    }
}

#[derive(Default)]
struct PendingOps {
    choose_path: HashMap<u64, oneshot::Sender<Option<String>>>,
    pasteboard_count: HashMap<u64, oneshot::Sender<i64>>,
    write_pasteboard: HashMap<u64, oneshot::Sender<bool>>,
}

pub struct HostBridge {
    callbacks: AgentHubHostCallbacks,
    closing: AtomicBool,
    visible: AtomicBool,
    focused: AtomicBool,
    next_op_id: AtomicU64,
    chooser_semaphore: Semaphore,
    pending: Mutex<PendingOps>,
}

impl HostBridge {
    pub fn new(callbacks: AgentHubHostCallbacks) -> Arc<Self> {
        Arc::new(Self {
            callbacks,
            closing: AtomicBool::new(false),
            visible: AtomicBool::new(false),
            focused: AtomicBool::new(false),
            next_op_id: AtomicU64::new(1),
            chooser_semaphore: Semaphore::new(1),
            pending: Mutex::new(PendingOps::default()),
        })
    }

    pub fn is_closing(&self) -> bool {
        self.closing.load(Ordering::Acquire)
    }

    pub fn set_window_state(&self, visible: bool, focused: bool) {
        self.visible.store(visible, Ordering::Release);
        self.focused.store(focused, Ordering::Release);
    }

    pub fn is_main_window_visible_and_focused(&self) -> bool {
        self.visible.load(Ordering::Acquire) && self.focused.load(Ordering::Acquire)
    }

    pub fn is_main_window_focused(&self) -> bool {
        self.focused.load(Ordering::Acquire)
    }

    pub fn begin_shutdown(&self) {
        self.closing.store(true, Ordering::Release);
        let active_choosers: Vec<u64> = {
            let mut pending = self.pending.lock().expect("host pending mutex");
            let ids = pending.choose_path.keys().copied().collect();
            pending.choose_path.clear();
            pending.pasteboard_count.clear();
            pending.write_pasteboard.clear();
            ids
        };
        if let Some(cancel) = self.callbacks.cancel_choose_local_path {
            for op_id in active_choosers {
                unsafe { cancel(self.callbacks.host_ctx, op_id) };
            }
        }
    }

    pub fn open_external_url(&self, url: &str) -> Result<(), &'static str> {
        if self.is_closing() {
            return Err("应用正在关闭");
        }
        let Some(open) = self.callbacks.open_external_url else {
            return Err("外部链接处理未就绪");
        };
        let bytes = url.as_bytes();
        let ptr = if bytes.is_empty() {
            std::ptr::null()
        } else {
            bytes.as_ptr().cast::<c_char>()
        };
        unsafe { open(self.callbacks.host_ctx, ptr, bytes.len()) };
        Ok(())
    }

    pub fn deliver_ci_notifications(&self, payload_json: &str) {
        if self.is_closing() {
            return;
        }
        if let Some(deliver) = self.callbacks.deliver_ci_notifications {
            let bytes = payload_json.as_bytes();
            let ptr = if bytes.is_empty() {
                std::ptr::null()
            } else {
                bytes.as_ptr().cast::<c_char>()
            };
            unsafe { deliver(self.callbacks.host_ctx, ptr, bytes.len()) };
        }
    }

    pub fn request_app_restart(&self) {
        if let Some(restart) = self.callbacks.request_app_restart {
            unsafe { restart(self.callbacks.host_ctx) };
        }
    }

    pub async fn choose_local_path(
        &self,
        directory: bool,
        prompt: &str,
    ) -> Result<Option<PathBuf>, String> {
        let _permit = self
            .chooser_semaphore
            .try_acquire()
            .map_err(|_| "文件选择窗口已打开".to_string())?;
        if self.is_closing() {
            return Err("应用正在关闭".into());
        }
        let Some(begin) = self.callbacks.begin_choose_local_path else {
            return Err("无法打开文件选择窗口".into());
        };
        let op_id = self.next_op_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        {
            let mut pending = self.pending.lock().expect("host pending mutex");
            if self.is_closing() {
                return Err("应用正在关闭".into());
            }
            pending.choose_path.insert(op_id, tx);
        }
        let bytes = prompt.as_bytes();
        let ptr = if bytes.is_empty() {
            std::ptr::null()
        } else {
            bytes.as_ptr().cast::<c_char>()
        };
        unsafe { begin(self.callbacks.host_ctx, op_id, directory, ptr, bytes.len()) };
        match tokio::time::timeout(Duration::from_secs(300), rx).await {
            Ok(Ok(None)) => Ok(None),
            Ok(Ok(Some(raw))) => {
                let path = raw.strip_suffix('\n').unwrap_or(&raw);
                if path.len() > 8192 {
                    return Err("所选路径超出限制".into());
                }
                if path.is_empty()
                    || path.chars().any(char::is_control)
                    || !std::path::Path::new(path).is_absolute()
                {
                    return Err("所选路径无法处理".into());
                }
                Ok(Some(PathBuf::from(path)))
            }
            Ok(Err(_)) => Ok(None),
            Err(_) => {
                let removed = self
                    .pending
                    .lock()
                    .expect("host pending mutex")
                    .choose_path
                    .remove(&op_id)
                    .is_some();
                if removed {
                    if let Some(cancel) = self.callbacks.cancel_choose_local_path {
                        unsafe { cancel(self.callbacks.host_ctx, op_id) };
                    }
                }
                Err(crate::codex_desktop::desktop_error(
                    agenthub_core::codex::desktop::DesktopError::Timeout,
                ))
            }
        }
    }

    pub fn complete_choose_local_path(&self, op_id: u64, path: Option<String>) {
        if let Some(tx) = self
            .pending
            .lock()
            .expect("host pending mutex")
            .choose_path
            .remove(&op_id)
        {
            let _ = tx.send(path);
        }
    }

    pub async fn pasteboard_change_count(&self) -> Result<i64, &'static str> {
        if self.is_closing() {
            return Err("剪贴板已变化，请重新粘贴");
        }
        let Some(begin) = self.callbacks.begin_pasteboard_change_count else {
            return Err("无法读取剪贴板附件");
        };
        let op_id = self.next_op_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        {
            let mut pending = self.pending.lock().expect("host pending mutex");
            if self.is_closing() {
                return Err("剪贴板已变化，请重新粘贴");
            }
            pending.pasteboard_count.insert(op_id, tx);
        }
        unsafe { begin(self.callbacks.host_ctx, op_id) };
        match tokio::time::timeout(Duration::from_secs(5), rx).await {
            Ok(Ok(count)) => Ok(count),
            _ => {
                self.pending
                    .lock()
                    .expect("host pending mutex")
                    .pasteboard_count
                    .remove(&op_id);
                Err("剪贴板已变化，请重新粘贴")
            }
        }
    }

    pub fn complete_pasteboard_change_count(&self, op_id: u64, count: i64) {
        if let Some(tx) = self
            .pending
            .lock()
            .expect("host pending mutex")
            .pasteboard_count
            .remove(&op_id)
        {
            let _ = tx.send(count);
        }
    }

    pub async fn write_pasteboard(&self, text: &str) -> Result<(), &'static str> {
        if text.len() > 64 * 1024 {
            return Err("诊断超出复制容量");
        }
        if self.is_closing() {
            return Err("无法复制诊断，请重试");
        }
        let Some(begin) = self.callbacks.begin_write_pasteboard else {
            return Err("无法复制诊断，请重试");
        };
        let op_id = self.next_op_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        {
            let mut pending = self.pending.lock().expect("host pending mutex");
            if self.is_closing() {
                return Err("无法复制诊断，请重试");
            }
            pending.write_pasteboard.insert(op_id, tx);
        }
        let bytes = text.as_bytes();
        let ptr = if bytes.is_empty() {
            std::ptr::null()
        } else {
            bytes.as_ptr().cast::<c_char>()
        };
        unsafe { begin(self.callbacks.host_ctx, op_id, ptr, bytes.len()) };
        match tokio::time::timeout(Duration::from_secs(5), rx).await {
            Ok(Ok(true)) => Ok(()),
            _ => {
                self.pending
                    .lock()
                    .expect("host pending mutex")
                    .write_pasteboard
                    .remove(&op_id);
                Err("无法复制诊断，请重试")
            }
        }
    }

    pub fn complete_write_pasteboard(&self, op_id: u64, ok: bool) {
        if let Some(tx) = self
            .pending
            .lock()
            .expect("host pending mutex")
            .write_pasteboard
            .remove(&op_id)
        {
            let _ = tx.send(ok);
        }
    }
}
