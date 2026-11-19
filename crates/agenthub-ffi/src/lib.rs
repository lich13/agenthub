pub mod ci_notifications;
pub mod codex_desktop;
pub mod commands;
pub mod gfm_markdown;
pub mod host;
pub mod isolation;
pub mod legacy_prefs;
#[path = "../../../src-tauri/src/overview.rs"]
pub mod overview;
pub mod remote;
#[path = "resources_wrapper.rs"]
pub mod resources;
pub mod services;

pub use codex_desktop::clipboard::{
    AgentHubRawFileUrl, AgentHubRawImageKind, AgentHubRawPasteboard,
};
pub use host::{AgentHubHostCallbacks, AgentHubRpcCallback, AgentHubStreamCallback};
pub use overview::{agenthub_paths_for_home, AgentHubPaths, DesktopState};
pub use services::probe::desktop_probe_status_with_state;
pub use services::updates::desktop_update_status_with_state;

use commands::dispatch::{dispatch, DispatchContext};
use host::{HostBridge, RpcSink, StreamSink};
use std::{
    ffi::{c_char, CStr, CString},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::task::AbortHandle;

pub struct AgentHubRuntime {
    tokio_runtime: Option<tokio::runtime::Runtime>,
    state: DesktopState,
    remote: remote::RemoteConnections,
    bridge: Arc<codex_desktop::CodexDesktopBridge>,
    host: Arc<HostBridge>,
    ci_task: AbortHandle,
    isolated: bool,
}

impl AgentHubRuntime {
    pub fn build(
        resource_dir: Option<&Path>,
        isolated_root: Option<&Path>,
        callbacks: AgentHubHostCallbacks,
    ) -> Result<Self, String> {
        let tokio_runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|err| format!("failed to initialize async runtime: {err}"))?;

        let (state, isolated) =
            isolation::init_desktop_state(isolated_root).map_err(|err| err.to_string())?;

        if !isolated {
            let helper_changed = if let Some(dir) = resource_dir {
                resources::sync_agenthub_webd_helper_from_resource(dir)?
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
        let bridge = Arc::new(codex_desktop::CodexDesktopBridge::new(
            agenthub_core::services::credentials::CredentialAccess::default(),
            isolated,
        ));
        let host = HostBridge::new(callbacks);
        let ci_task = {
            let _enter = tokio_runtime.enter();
            ci_notifications::start(state.clone(), remote.clone(), host.clone(), isolated)
        };

        Ok(Self {
            tokio_runtime: Some(tokio_runtime),
            state,
            remote,
            bridge,
            host,
            ci_task,
            isolated,
        })
    }

    pub fn shutdown_gracefully(mut self) {
        // Step 1 & 2: Reject new calls, wake pending oneshots, and cancel active NSOpenPanel.
        self.host.begin_shutdown();
        // Step 3: Abort CI observer task.
        self.ci_task.abort();
        // Step 4: Close owned SSH transport.
        self.remote.disconnect();
        // Step 5 & 6: Unbind attachment receiver, close stream listeners, release DesktopWorkerLease,
        // and shut down Tokio runtime within a bounded 2s deadline.
        if let Some(rt) = self.tokio_runtime.take() {
            let bridge = self.bridge.clone();
            rt.block_on(async move {
                bridge.shutdown().await;
            });
            drop(self.bridge);
            rt.shutdown_timeout(Duration::from_secs(2));
        }
        if self.isolated {
            isolation::set_isolated_mode(false);
        }
    }
}

unsafe fn opt_cstr(ptr: *const c_char) -> Result<Option<String>, String> {
    if ptr.is_null() {
        return Ok(None);
    }
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
        .map(CString::into_raw)
        .unwrap_or(std::ptr::null_mut())
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
        unsafe { *out_runtime = std::ptr::null_mut() };
    }
    if !out_error_utf8.is_null() {
        unsafe { *out_error_utf8 = std::ptr::null_mut() };
    }
    if out_runtime.is_null() {
        if !out_error_utf8.is_null() {
            unsafe { *out_error_utf8 = into_c_string_ptr("out_runtime is null".into()) };
        }
        return -1;
    }

    let result = (|| -> Result<AgentHubRuntime, String> {
        let res_dir = unsafe { opt_cstr(resource_dir_utf8) }?.map(PathBuf::from);
        let iso_dir = unsafe { opt_cstr(isolated_root_utf8) }?.map(PathBuf::from);
        AgentHubRuntime::build(res_dir.as_deref(), iso_dir.as_deref(), callbacks)
    })();

    match result {
        Ok(runtime) => {
            unsafe { *out_runtime = Box::into_raw(Box::new(runtime)) };
            0
        }
        Err(error) => {
            if !out_error_utf8.is_null() {
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
    let boxed = unsafe { Box::from_raw(runtime) };
    boxed.shutdown_gracefully();
}

/// # Safety
/// `ptr` must be null or a string pointer previously returned by an `agenthub_ffi_*` function.
#[no_mangle]
pub unsafe extern "C" fn agenthub_ffi_string_free(ptr: *mut c_char) {
    if !ptr.is_null() {
        unsafe { drop(CString::from_raw(ptr)) };
    }
}

/// # Safety
/// `runtime` must be null or a live `AgentHubRuntime` pointer.
#[no_mangle]
pub unsafe extern "C" fn agenthub_ffi_set_main_window_state(
    runtime: *const AgentHubRuntime,
    visible: bool,
    focused: bool,
) {
    let Some(rt) = (unsafe { runtime.as_ref() }) else {
        return;
    };
    rt.host.set_window_state(visible, focused);
}

/// # Safety
/// `runtime` must be null or live; `path_or_null` must be null or a valid C string.
#[no_mangle]
pub unsafe extern "C" fn agenthub_ffi_complete_choose_local_path(
    runtime: *const AgentHubRuntime,
    op_id: u64,
    path_or_null: *const c_char,
) {
    let Some(rt) = (unsafe { runtime.as_ref() }) else {
        return;
    };
    let path = unsafe { opt_cstr(path_or_null) }.ok().flatten();
    rt.host.complete_choose_local_path(op_id, path);
}

/// # Safety
/// `runtime` must be null or live.
#[no_mangle]
pub unsafe extern "C" fn agenthub_ffi_complete_pasteboard_change_count(
    runtime: *const AgentHubRuntime,
    op_id: u64,
    count: i64,
) {
    let Some(rt) = (unsafe { runtime.as_ref() }) else {
        return;
    };
    rt.host.complete_pasteboard_change_count(op_id, count);
}

/// # Safety
/// `runtime` must be null or live.
#[no_mangle]
pub unsafe extern "C" fn agenthub_ffi_complete_write_pasteboard(
    runtime: *const AgentHubRuntime,
    op_id: u64,
    ok: bool,
) {
    let Some(rt) = (unsafe { runtime.as_ref() }) else {
        return;
    };
    rt.host.complete_write_pasteboard(op_id, ok);
}

/// # Safety
/// `home_dir_utf8` must be null or a valid null-terminated UTF-8 path.
#[no_mangle]
pub unsafe extern "C" fn agenthub_ffi_read_legacy_webview_prefs(
    home_dir_utf8: *const c_char,
) -> *mut c_char {
    let home = unsafe { opt_cstr(home_dir_utf8) }
        .ok()
        .flatten()
        .map(PathBuf::from)
        .or_else(dirs::home_dir);
    let prefs = match home {
        Some(dir) => legacy_prefs::read_legacy_webview_prefs(&dir),
        None => legacy_prefs::LegacyWebviewPrefs::default(),
    };
    let json = serde_json::to_string(&prefs).unwrap_or_else(|_| "{}".to_string());
    into_c_string_ptr(json)
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
        let bytes = unsafe { std::slice::from_raw_parts(utf8_ptr.cast::<u8>(), len) };
        std::str::from_utf8(bytes).unwrap_or("")
    };
    let doc = gfm_markdown::parse_gfm_markdown(source);
    let json = serde_json::to_string(&doc).unwrap_or_else(|_| "{\"blocks\":[]}".to_string());
    into_c_string_ptr(json)
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
    let Some(rt) = (unsafe { runtime.as_ref() }) else {
        sink.respond(Err("runtime is null".into()));
        return;
    };
    if rt.host.is_closing() {
        sink.respond(Err("应用正在关闭".into()));
        return;
    }
    let Some(tokio_rt) = rt.tokio_runtime.as_ref() else {
        sink.respond(Err("应用正在关闭".into()));
        return;
    };
    let command = match unsafe { opt_cstr(command_utf8) } {
        Ok(Some(cmd)) => cmd,
        _ => {
            sink.respond(Err("command name is empty or invalid".into()));
            return;
        }
    };
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
    let bridge = rt.bridge.clone();
    let host = rt.host.clone();
    let isolated = rt.isolated;
    tokio_rt.spawn(async move {
        let ctx = DispatchContext {
            state: &state,
            remote: &remote,
            bridge: &bridge,
            host: &host,
            isolated,
        };
        let result = dispatch(ctx, &command, &args).await;
        sink.respond(result);
    });
}

/// # Safety
/// `runtime` must be null or live; `request_json_utf8` must be null or valid null-terminated UTF-8.
#[no_mangle]
pub unsafe extern "C" fn agenthub_ffi_desktop_attach(
    runtime: *const AgentHubRuntime,
    request_json_utf8: *const c_char,
    request_id: u64,
    initial_cb: AgentHubRpcCallback,
    initial_ctx: *mut std::ffi::c_void,
    stream_cb: AgentHubStreamCallback,
    stream_ctx: *mut std::ffi::c_void,
) {
    let initial_sink = RpcSink::new(request_id, initial_cb, initial_ctx);
    let stream_sink = StreamSink::new(stream_cb, stream_ctx);
    let Some(rt) = (unsafe { runtime.as_ref() }) else {
        stream_sink.close();
        initial_sink.respond(Err("runtime is null".into()));
        return;
    };
    if rt.host.is_closing() {
        stream_sink.close();
        initial_sink.respond(Err("应用正在关闭".into()));
        return;
    }
    let Some(tokio_rt) = rt.tokio_runtime.as_ref() else {
        stream_sink.close();
        initial_sink.respond(Err("应用正在关闭".into()));
        return;
    };
    let raw_req = match unsafe { opt_cstr(request_json_utf8) } {
        Ok(Some(s)) => s,
        _ => {
            stream_sink.close();
            initial_sink.respond(Err("invalid attach request".into()));
            return;
        }
    };
    let state = rt.state.clone();
    let remote = rt.remote.clone();
    let bridge = rt.bridge.clone();
    tokio_rt.spawn(async move {
        let parsed: Result<
            agenthub_core::services::codex_desktop::DesktopThreadRequest,
            serde_json::Error,
        > = serde_json::from_str(&raw_req);
        let req = match parsed {
            Ok(req) => req,
            Err(err) => {
                stream_sink.close();
                initial_sink.respond(Err(format!("invalid attach request: {err}")));
                return;
            }
        };
        match codex_desktop::attach(&state, &remote, &bridge, req, stream_sink.clone()).await {
            Ok(view) => {
                initial_sink.respond(serde_json::to_string(&view).map_err(|e| e.to_string()));
            }
            Err(err) => {
                stream_sink.close();
                initial_sink.respond(Err(err));
            }
        }
    });
}

/// # Safety
/// `runtime` must be null or live; `request_json_utf8` must be null or valid null-terminated UTF-8.
#[no_mangle]
pub unsafe extern "C" fn agenthub_ffi_desktop_attachments(
    runtime: *const AgentHubRuntime,
    request_json_utf8: *const c_char,
    request_id: u64,
    result_cb: AgentHubRpcCallback,
    result_ctx: *mut std::ffi::c_void,
    event_cb: AgentHubStreamCallback,
    event_ctx: *mut std::ffi::c_void,
) {
    let result_sink = RpcSink::new(request_id, result_cb, result_ctx);
    let event_stream = event_cb.map(|_| StreamSink::new(event_cb, event_ctx));
    let Some(rt) = (unsafe { runtime.as_ref() }) else {
        if let Some(s) = &event_stream {
            s.close();
        }
        result_sink.respond(Err("runtime is null".into()));
        return;
    };
    if rt.host.is_closing() {
        if let Some(s) = &event_stream {
            s.close();
        }
        result_sink.respond(Err("应用正在关闭".into()));
        return;
    };
    let Some(tokio_rt) = rt.tokio_runtime.as_ref() else {
        if let Some(s) = &event_stream {
            s.close();
        }
        result_sink.respond(Err("应用正在关闭".into()));
        return;
    };
    let raw_req = match unsafe { opt_cstr(request_json_utf8) } {
        Ok(Some(s)) => s,
        _ => {
            if let Some(s) = &event_stream {
                s.close();
            }
            result_sink.respond(Err("invalid attachments request".into()));
            return;
        }
    };
    let state = rt.state.clone();
    let remote = rt.remote.clone();
    let bridge = rt.bridge.clone();
    let host = rt.host.clone();
    tokio_rt.spawn(async move {
        let parsed: Result<
            agenthub_core::services::codex_desktop::DesktopAttachmentRequest,
            serde_json::Error,
        > = serde_json::from_str(&raw_req);
        let req = match parsed {
            Ok(req) => req,
            Err(err) => {
                if let Some(s) = &event_stream {
                    s.close();
                }
                result_sink.respond(Err(format!("invalid attachments request: {err}")));
                return;
            }
        };
        let is_bind = matches!(
            &req.action,
            agenthub_core::services::codex_desktop::DesktopAttachmentAction::Bind { .. }
        );
        if !is_bind {
            if let Some(s) = &event_stream {
                s.close();
            }
        }
        match codex_desktop::attachments::handle(
            &host,
            &state,
            &remote,
            &bridge,
            req,
            if is_bind { event_stream.clone() } else { None },
        )
        .await
        {
            Ok(res) => {
                result_sink.respond(serde_json::to_string(&res).map_err(|e| e.to_string()));
            }
            Err(err) => {
                if let Some(s) = &event_stream {
                    s.close();
                }
                result_sink.respond(Err(err));
            }
        }
    });
}

/// # Safety
/// `runtime` must be null or live; `raw_pasteboard` and non-empty buffers must remain valid during this call.
#[no_mangle]
pub unsafe extern "C" fn agenthub_ffi_desktop_paste(
    runtime: *const AgentHubRuntime,
    request_json_utf8: *const c_char,
    raw_pasteboard: *const AgentHubRawPasteboard,
    request_id: u64,
    result_cb: AgentHubRpcCallback,
    result_ctx: *mut std::ffi::c_void,
) {
    let result_sink = RpcSink::new(request_id, result_cb, result_ctx);
    let Some(rt) = (unsafe { runtime.as_ref() }) else {
        result_sink.respond(Err("runtime is null".into()));
        return;
    };
    if rt.host.is_closing() {
        result_sink.respond(Err("应用正在关闭".into()));
        return;
    }
    let Some(tokio_rt) = rt.tokio_runtime.as_ref() else {
        result_sink.respond(Err("应用正在关闭".into()));
        return;
    };
    let Some(raw_pb) = (unsafe { raw_pasteboard.as_ref() }) else {
        result_sink.respond(Err("无法读取剪贴板附件".into()));
        return;
    };
    let snapshot =
        match unsafe { codex_desktop::clipboard::RawPasteboardSnapshot::from_raw(raw_pb) } {
            Ok(snap) => snap,
            Err(err) => {
                result_sink.respond(Err(err.to_owned()));
                return;
            }
        };
    let raw_req = match unsafe { opt_cstr(request_json_utf8) } {
        Ok(Some(s)) => s,
        _ => {
            result_sink.respond(Err("invalid paste request".into()));
            return;
        }
    };
    let state = rt.state.clone();
    let remote = rt.remote.clone();
    let bridge = rt.bridge.clone();
    let host = rt.host.clone();
    tokio_rt.spawn(async move {
        let req: agenthub_core::services::codex_desktop::DesktopAttachmentRequest =
            match serde_json::from_str(&raw_req) {
                Ok(r) => r,
                Err(err) => {
                    result_sink.respond(Err(format!("invalid paste request: {err}")));
                    return;
                }
            };
        let res =
            codex_desktop::attachments::paste(&host, &state, &remote, &bridge, req, snapshot).await;
        result_sink.respond(res.and_then(|v| serde_json::to_string(&v).map_err(|e| e.to_string())));
    });
}

/// # Safety
/// `runtime` must be null or live; `paths_json_utf8` must be null or valid null-terminated UTF-8.
#[no_mangle]
pub unsafe extern "C" fn agenthub_ffi_desktop_drop_files(
    runtime: *const AgentHubRuntime,
    paths_json_utf8: *const c_char,
    logical_x: f64,
    logical_y: f64,
) {
    let Some(rt) = (unsafe { runtime.as_ref() }) else {
        return;
    };
    if rt.host.is_closing() {
        return;
    }
    let Some(tokio_rt) = rt.tokio_runtime.as_ref() else {
        return;
    };
    let Some(raw) = (unsafe { opt_cstr(paths_json_utf8) }).ok().flatten() else {
        return;
    };
    let Ok(paths) = serde_json::from_str::<Vec<PathBuf>>(&raw) else {
        return;
    };
    let host = rt.host.clone();
    let state = rt.state.clone();
    let remote = rt.remote.clone();
    let bridge = rt.bridge.clone();
    tokio_rt.spawn(async move {
        codex_desktop::attachments::drop_files(
            host, state, remote, bridge, paths, logical_x, logical_y,
        )
        .await;
    });
}

#[cfg(test)]
mod contract_guard_tests;
