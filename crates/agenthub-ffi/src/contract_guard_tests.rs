use super::*;
use std::collections::BTreeSet;

fn contract_actions() -> Vec<serde_json::Value> {
    let raw = include_str!("../../../contracts/agenthub-contract.json");
    let value: serde_json::Value =
        serde_json::from_str(raw).expect("contract registry must be valid JSON");
    value
        .get("actions")
        .and_then(serde_json::Value::as_array)
        .expect("contract actions must be an array")
        .clone()
}

#[test]
fn ffi_dispatch_handler_matches_contract_registry() {
    let registered_ffi_paths: BTreeSet<String> = commands::dispatch::REGISTERED_COMMAND_ENTRIES
        .iter()
        .map(|(_, handler)| (*handler).to_owned())
        .collect();
    let registered_action_ids: BTreeSet<String> = commands::dispatch::REGISTERED_COMMAND_ENTRIES
        .iter()
        .map(|(action_id, _)| (*action_id).to_owned())
        .collect();

    let expected_ffi_paths: BTreeSet<String> = contract_actions()
        .into_iter()
        .filter_map(|action| {
            action
                .get("ffiHandler")
                .and_then(serde_json::Value::as_str)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
        })
        .collect();

    let expected_action_ids: BTreeSet<String> = contract_actions()
        .into_iter()
        .filter(|action| {
            action
                .get("ffiHandler")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|value| !value.is_empty())
        })
        .filter_map(|action| {
            action
                .get("id")
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned)
        })
        .collect();

    assert_eq!(registered_ffi_paths.len(), 64);
    assert_eq!(registered_ffi_paths, expected_ffi_paths);
    assert_eq!(registered_action_ids, expected_action_ids);
}

#[test]
fn c_abi_isolated_init_invoke_capabilities_rejects_unknown_command_and_shutdown_roundtrip() {
    let temp = tempfile::tempdir().unwrap();
    let iso_c = CString::new(temp.path().to_str().unwrap()).unwrap();
    let mut rt_ptr: *mut AgentHubRuntime = std::ptr::null_mut();
    let mut err_ptr: *mut c_char = std::ptr::null_mut();

    let rc = unsafe {
        agenthub_ffi_init(
            std::ptr::null(),
            iso_c.as_ptr(),
            AgentHubHostCallbacks::default(),
            &mut rt_ptr,
            &mut err_ptr,
        )
    };
    if rc != 0 {
        let msg = if err_ptr.is_null() {
            "unknown init error".to_owned()
        } else {
            let s = unsafe { CStr::from_ptr(err_ptr) }
                .to_string_lossy()
                .into_owned();
            unsafe { agenthub_ffi_string_free(err_ptr) };
            s
        };
        panic!("agenthub_ffi_init failed (rc={rc}): {msg}");
    }
    assert!(!rt_ptr.is_null());
    assert!(err_ptr.is_null());
    assert!(temp.path().join("config.toml").is_file());
    assert!(
        !temp.path().join("workspace").exists(),
        "isolated desktop init must not materialize retired workspace directories on disk"
    );

    let (tx, rx) = std::sync::mpsc::channel::<(u64, bool, String)>();
    unsafe extern "C" fn on_rpc(
        req_id: u64,
        ok: bool,
        ptr: *const c_char,
        len: usize,
        ctx: *mut std::ffi::c_void,
    ) {
        let sender = unsafe { &*(ctx as *const std::sync::mpsc::Sender<(u64, bool, String)>) };
        let body = if len == 0 || ptr.is_null() {
            String::new()
        } else {
            let bytes = unsafe { std::slice::from_raw_parts(ptr.cast::<u8>(), len) };
            String::from_utf8_lossy(bytes).into_owned()
        };
        let _ = sender.send((req_id, ok, body));
    }

    let cmd = CString::new("system.capabilities").unwrap();
    let args = CString::new("{}").unwrap();
    unsafe {
        agenthub_ffi_invoke(
            rt_ptr,
            cmd.as_ptr(),
            args.as_ptr(),
            42,
            Some(on_rpc),
            &tx as *const _ as *mut std::ffi::c_void,
        );
    }
    let (id, ok, body) = rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(id, 42);
    assert!(ok, "{body}");
    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(json["api_version"], 2);
    assert_eq!(json["host_surface"], "desktop_embedded_tauri");
    assert!(json["capabilities"].is_object());

    // Zero-length GFM parse with null pointer must never UB.
    let mut empty_len: usize = usize::MAX;
    let empty_gfm = unsafe { agenthub_ffi_parse_gfm_markdown(std::ptr::null(), 0, &mut empty_len) };
    assert!(!empty_gfm.is_null());
    assert!(empty_len > 0);
    unsafe { agenthub_ffi_string_free(empty_gfm) };

    // Unknown desktop commands must fail closed via the positive allowlist.
    let unknown_cmd = CString::new("unknown.command").unwrap();
    let unknown_args = CString::new("{}").unwrap();
    unsafe {
        agenthub_ffi_invoke(
            rt_ptr,
            unknown_cmd.as_ptr(),
            unknown_args.as_ptr(),
            43,
            Some(on_rpc),
            &tx as *const _ as *mut std::ffi::c_void,
        );
    }
    let (ret_id, ret_ok, ret_err) = rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(ret_id, 43);
    assert!(!ret_ok);
    assert!(
        ret_err.contains("unsupported desktop command: unknown.command"),
        "Expected unknown command rejection, got: {ret_err}"
    );

    unsafe { agenthub_ffi_shutdown(rt_ptr) };
}
