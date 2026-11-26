use std::{
    ffi::c_char,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

pub type AgentHubRpcCallback =
    Option<unsafe extern "C" fn(u64, bool, *const c_char, usize, *mut std::ffi::c_void)>;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct AgentHubHostCallbacks {
    pub host_ctx: *mut std::ffi::c_void,
    pub retain_host_ctx: Option<unsafe extern "C" fn(*mut std::ffi::c_void)>,
    pub release_host_ctx: Option<unsafe extern "C" fn(*mut std::ffi::c_void)>,
    pub deliver_ci_notifications:
        Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char, usize)>,
    pub request_app_restart: Option<unsafe extern "C" fn(*mut std::ffi::c_void)>,
}

// SAFETY: Host callbacks may be called on runtime workers. HostBridge retains the context across them.
unsafe impl Send for AgentHubHostCallbacks {}
// SAFETY: Callbacks accept concurrent invocation. The host context synchronizes access to its runtime.
unsafe impl Sync for AgentHubHostCallbacks {}

impl Default for AgentHubHostCallbacks {
    fn default() -> Self {
        Self {
            host_ctx: std::ptr::null_mut(),
            retain_host_ctx: None,
            release_host_ctx: None,
            deliver_ci_notifications: None,
            request_app_restart: None,
        }
    }
}

pub(crate) struct RpcSink {
    request_id: u64,
    callback: AgentHubRpcCallback,
    ctx: usize,
}

impl RpcSink {
    pub(crate) fn new(
        request_id: u64,
        callback: AgentHubRpcCallback,
        ctx: *mut std::ffi::c_void,
    ) -> Self {
        Self {
            request_id,
            callback,
            ctx: ctx as usize,
        }
    }

    pub(crate) fn respond(mut self, result: Result<String, String>) {
        match result {
            Ok(json) => self.send(true, &json),
            Err(error) => self.send(false, &error),
        }
    }

    fn send(&mut self, ok: bool, payload: &str) {
        let Some(cb) = self.callback.take() else {
            return;
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

impl Drop for RpcSink {
    fn drop(&mut self) {
        self.send(false, "请求已中断，结果未确认");
    }
}

pub(crate) struct HostBridge {
    callbacks: AgentHubHostCallbacks,
    closing: AtomicBool,
}

impl Drop for HostBridge {
    fn drop(&mut self) {
        if let Some(release) = self.callbacks.release_host_ctx {
            // SAFETY: paired with construction, after the final Arc borrow.
            unsafe { release(self.callbacks.host_ctx) };
        }
    }
}

impl HostBridge {
    pub(crate) fn new(callbacks: AgentHubHostCallbacks) -> Arc<Self> {
        if let Some(retain) = callbacks.retain_host_ctx {
            // SAFETY: the caller owns a live context throughout initialization.
            unsafe { retain(callbacks.host_ctx) };
        }
        Arc::new(Self {
            callbacks,
            closing: AtomicBool::new(false),
        })
    }

    pub(crate) fn is_closing(&self) -> bool {
        self.closing.load(Ordering::Acquire)
    }

    pub(crate) fn begin_shutdown(&self) {
        self.closing.store(true, Ordering::Release);
    }

    pub(crate) fn deliver_ci_notifications(&self, payload_json: &str) {
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
            // SAFETY: HostBridge retains the context and the borrowed bytes remain live until this callback returns.
            unsafe { deliver(self.callbacks.host_ctx, ptr, bytes.len()) };
        }
    }

    pub(crate) fn request_app_restart(&self) {
        if self.is_closing() {
            return;
        }
        if let Some(restart) = self.callbacks.request_app_restart {
            // SAFETY: HostBridge retains the context through this call.
            unsafe { restart(self.callbacks.host_ctx) };
        }
    }
}
