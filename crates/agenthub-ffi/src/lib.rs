pub mod ci_notifications;
pub mod commands;
pub mod gfm_markdown;
pub mod host;
pub mod isolation;
pub mod overview;
pub mod remote;
#[path = "resources_inner.rs"]
pub mod resources;
pub mod services;

pub use host::{AgentHubHostCallbacks, AgentHubRpcCallback};
pub use overview::DesktopState;
pub use services::probe::desktop_probe_status_with_state;
pub use services::updates::desktop_update_status_with_state;

use commands::dispatch::{dispatch, DispatchContext};
use host::{HostBridge, RpcSink};
use std::{
    ffi::{c_char, CStr, CString},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::task::AbortHandle;

#[repr(C)]
pub struct AgentHubCommandAccess {
    pub remote: bool,
    pub read_only: bool,
}

/// # Safety
/// A non-null command must be a readable NUL-terminated string for this call.
#[no_mangle]
pub unsafe extern "C" fn agenthub_ffi_command_access(
    command_utf8: *const c_char,
    hook_status: bool,
) -> AgentHubCommandAccess {
    use agenthub_core::services::commands::{desktop_command_is_read, is_allowed_rpc_command};
    let command = if command_utf8.is_null() {
        ""
    } else {
        // SAFETY: the caller provides a live C string, borrowed only for this query.
        unsafe { std::ffi::CStr::from_ptr(command_utf8) }
            .to_str()
            .unwrap_or("")
    };
    AgentHubCommandAccess {
        remote: is_allowed_rpc_command(command),
        read_only: desktop_command_is_read(command, hook_status),
    }
}

pub struct AgentHubRuntime {
    tokio_runtime: tokio::runtime::Runtime,
    state: DesktopState,
    remote: remote::RemoteConnections,
    host: Arc<HostBridge>,
    ci_task: AbortHandle,
}

impl AgentHubRuntime {
    pub fn build(
        resource_dir: Option<&Path>,
        isolated_root: Option<&Path>,
        callbacks: AgentHubHostCallbacks,
    ) -> Result<Self, String> {
        let tokio_runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .map_err(|err| format!("failed to initialize async runtime: {err}"))?;

        let state = isolation::init_desktop_state(isolated_root).map_err(|err| err.to_string())?;
        state
            .db
            .bind_legacy_panel_records()
            .map_err(|err| format!("无法转换加密记录，原数据已保留: {err}"))?;

        let isolated = state.isolated;
        if !isolated {
            let helper_changed = if let Some(dir) = resource_dir {
                resources::sync_agenthub_webd_helper_from_resource(dir, state.platform())?
            } else {
                false
            };
            if let Err(error) = resources::repair_probe_error_monitor_launch_agent(
                &state.config(),
                state.platform(),
                helper_changed,
            ) {
                eprintln!("Probe monitor repair failed: {error}");
            }
        }

        let remote = remote::RemoteConnections::load(&state.config().paths.data_dir)?;
        let host = HostBridge::new(callbacks);
        let ci_task = {
            let _enter = tokio_runtime.enter();
            ci_notifications::start(state.clone(), remote.clone(), host.clone(), isolated)
        };

        Ok(Self {
            tokio_runtime,
            state,
            remote,
            host,
            ci_task,
        })
    }

    pub fn shutdown_gracefully(self) {
        self.host.begin_shutdown();
        self.ci_task.abort();
        self.remote.disconnect();
        self.tokio_runtime.shutdown_timeout(Duration::from_secs(2));
    }
}

unsafe fn opt_cstr(ptr: *const c_char) -> Result<Option<String>, String> {
    if ptr.is_null() {
        return Ok(None);
    }
    // SAFETY: The ABI supplies a null pointer or a NUL-terminated string valid through this call.
    let s = unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .map_err(|_| "invalid UTF-8 string".to_string())?;
    Ok((!s.is_empty()).then(|| s.to_owned()))
}

fn into_c_string_ptr(text: String) -> *mut c_char {
    let sanitized = if text.as_bytes().contains(&0) {
        text.replace('\0', "")
    } else {
        text
    };
    CString::new(sanitized)
        .expect("interior NULs removed")
        .into_raw()
}

/// # Safety
/// Pointers must be either null or valid null-terminated UTF-8 strings / out-pointers for the duration of the call.
#[no_mangle]
pub unsafe extern "C" fn agenthub_ffi_init(
    resource_dir_utf8: *const c_char,
    isolated_root_utf8: *const c_char,
    callbacks: AgentHubHostCallbacks,
    out_runtime: *mut *mut AgentHubRuntime,
    out_error_utf8: *mut *mut c_char,
) -> i32 {
    if !out_runtime.is_null() {
        // SAFETY: The checked C out-parameter points to a writable pointer slot for this call.
        unsafe { *out_runtime = std::ptr::null_mut() };
    }
    if !out_error_utf8.is_null() {
        // SAFETY: The checked C out-parameter points to a writable pointer slot for this call.
        unsafe { *out_error_utf8 = std::ptr::null_mut() };
    }
    if out_runtime.is_null() {
        if !out_error_utf8.is_null() {
            // SAFETY: The checked C out-parameter points to a writable pointer slot for this call.
            unsafe { *out_error_utf8 = into_c_string_ptr("out_runtime is null".into()) };
        }
        return -1;
    }

    let result = (|| -> Result<AgentHubRuntime, String> {
        // SAFETY: The ABI supplies a null pointer or a NUL-terminated string valid through this call.
        let res_dir = unsafe { opt_cstr(resource_dir_utf8) }?.map(PathBuf::from);
        // SAFETY: The ABI supplies a null pointer or a NUL-terminated string valid through this call.
        let iso_dir = unsafe { opt_cstr(isolated_root_utf8) }?.map(PathBuf::from);
        AgentHubRuntime::build(res_dir.as_deref(), iso_dir.as_deref(), callbacks)
    })();

    match result {
        Ok(runtime) => {
            // SAFETY: The checked C out-parameter points to a writable pointer slot for this call.
            unsafe { *out_runtime = Box::into_raw(Box::new(runtime)) };
            0
        }
        Err(error) => {
            if !out_error_utf8.is_null() {
                // SAFETY: The checked C out-parameter points to a writable pointer slot for this call.
                unsafe { *out_error_utf8 = into_c_string_ptr(error) };
            }
            -1
        }
    }
}

/// # Safety
/// `runtime` must be null or a valid pointer previously returned by `agenthub_ffi_init` and not yet freed.
#[no_mangle]
pub unsafe extern "C" fn agenthub_ffi_shutdown(runtime: *mut AgentHubRuntime) {
    if runtime.is_null() {
        return;
    }
    // SAFETY: Shutdown takes the single handle produced by init after all host borrows have ended.
    let boxed = unsafe { Box::from_raw(runtime) };
    boxed.shutdown_gracefully();
}

/// # Safety
/// `ptr` must be null or a string pointer previously returned by an `agenthub_ffi_*` function.
#[no_mangle]
pub unsafe extern "C" fn agenthub_ffi_string_free(ptr: *mut c_char) {
    if !ptr.is_null() {
        // SAFETY: The caller returns the unique CString allocation handed out by this library.
        unsafe { drop(CString::from_raw(ptr)) };
    }
}

/// # Safety
/// When `len > 0`, `utf8_ptr` must point to `len` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn agenthub_ffi_parse_gfm_markdown(
    utf8_ptr: *const c_char,
    len: usize,
) -> *mut c_char {
    let source = if len == 0 || utf8_ptr.is_null() {
        ""
    } else {
        // SAFETY: The caller keeps the nonempty UTF-8 buffer readable for len bytes during parsing.
        let bytes = unsafe { std::slice::from_raw_parts(utf8_ptr.cast::<u8>(), len) };
        std::str::from_utf8(bytes).unwrap_or("")
    };
    let doc = gfm_markdown::parse_gfm_markdown(source);
    match serde_json::to_string(&doc) {
        Ok(json) => into_c_string_ptr(json),
        Err(_) => std::ptr::null_mut(),
    }
}

/// # Safety
/// `runtime` must be null or live; string pointers must be null or valid null-terminated UTF-8.
#[no_mangle]
pub unsafe extern "C" fn agenthub_ffi_invoke(
    runtime: *const AgentHubRuntime,
    command_utf8: *const c_char,
    args_json_utf8: *const c_char,
    request_id: u64,
    callback: AgentHubRpcCallback,
    callback_ctx: *mut std::ffi::c_void,
) {
    let sink = RpcSink::new(request_id, callback, callback_ctx);
    // SAFETY: The host borrows this handle until this synchronous entry returns; shutdown waits for borrowers.
    let Some(rt) = (unsafe { runtime.as_ref() }) else {
        sink.respond(Err("runtime is null".into()));
        return;
    };
    let tokio_rt = &rt.tokio_runtime;
    // SAFETY: The ABI supplies a null pointer or a NUL-terminated string valid through this call.
    let command = match unsafe { opt_cstr(command_utf8) } {
        Ok(Some(cmd)) => cmd,
        _ => {
            sink.respond(Err("command name is empty or invalid".into()));
            return;
        }
    };
    // SAFETY: The ABI supplies a null pointer or a NUL-terminated string valid through this call.
    let args = match unsafe { opt_cstr(args_json_utf8) } {
        Ok(Some(json)) => json,
        Ok(None) => "{}".to_string(),
        Err(err) => {
            sink.respond(Err(err));
            return;
        }
    };
    let state = rt.state.clone();
    let remote = rt.remote.clone();
    let host = rt.host.clone();
    let isolated = rt.state.isolated;
    tokio_rt.spawn(async move {
        let ctx = DispatchContext {
            state: &state,
            remote: &remote,
            host: &host,
            isolated,
        };
        let result = dispatch(ctx, &command, &args).await;
        sink.respond(result);
    });
}

#[cfg(test)]
mod contract_guard_tests;

#[cfg(test)]
mod lifecycle_tests;
