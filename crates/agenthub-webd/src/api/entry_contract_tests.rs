use std::{fs, path::PathBuf};

fn src(path: &str) -> String {
    fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join(path),
    )
    .unwrap_or_default()
}

fn production_section(source: &str) -> &str {
    source
        .split("\n#[cfg(test)]\nmod tests {")
        .next()
        .expect("source should have a production section")
}

#[test]
fn api_entry_exposes_only_current_submodules() {
    let api = src("api.rs");
    for module in [
        "mod cleanup;",
        "mod grok;",
        "mod jobs;",
        "mod payload;",
        "mod probe;",
        "mod routes;",
        "mod rpc_dispatch;",
        "mod system;",
        "mod threads;",
    ] {
        assert!(api.contains(module), "missing current API module: {module}");
    }
    for retired in [
        "mod goals;",
        "mod uploads;",
        "mod desktop_webui;",
        "mod security;",
        "mod web_auth;",
    ] {
        assert!(
            !api.contains(retired),
            "retired API module remained: {retired}"
        );
    }
    let production = production_section(&api);
    for handler in [
        "async fn rpc_dispatch",
        "async fn create_thread",
        "async fn send_message",
    ] {
        assert!(
            !production.contains(handler),
            "API entry owns retired handler: {handler}"
        );
    }
}

#[test]
fn rpc_surface_separates_retired_commands_from_current_allowlist() {
    let surface = src("rpc_surface.rs");
    use agenthub_core::services::commands;
    assert!(surface.contains("is_transport_rpc_command"));
    assert!(surface.contains("is_business_rpc_command"));
    for command in ["grok.list", "grok.deleteExecute"] {
        assert!(commands::is_allowed_rpc_command(command));
        assert!(!commands::is_retired_command(command));
    }
    assert!(commands::is_retired_command("pi.list"));
    assert!(!commands::is_allowed_rpc_command(
        "claude.ownershipDiagnostics"
    ));
    assert!(!commands::is_allowed_transport_command(
        "claude.ownershipDiagnostics"
    ));
    assert!(commands::is_retired_command(commands::THREADS_SEND));
    for command in commands::RETIRED_COMMANDS {
        assert!(!commands::is_allowed_rpc_command(command));
        assert!(!commands::is_allowed_transport_command(command));
    }
}

#[test]
fn current_adapters_delegate_thread_and_provider_operations() {
    let threads = production_section(&src("api/threads.rs")).to_string();
    let grok = production_section(&src("api/grok.rs")).to_string();
    let claude = production_section(&src("api/claude.rs")).to_string();
    assert!(threads.contains("list_threads"));
    assert!(threads.contains("thread_detail"));
    for operation in [
        "grok_list",
        "grok_detail",
        "grok_rename",
        "grok_delete_preview",
        "grok_delete_execute",
    ] {
        assert!(
            grok.contains(operation),
            "missing Grok adapter operation: {operation}"
        );
    }
    for operation in [
        "claude_list",
        "claude_detail",
        "claude_rename",
        "claude_delete_preview",
        "claude_delete_execute",
    ] {
        assert!(
            claude.contains(operation),
            "missing Claude adapter operation: {operation}"
        );
    }
    for retired in [
        "create_thread",
        "send_message",
        "enqueue_followup",
        "stop_thread",
        "fork_thread",
    ] {
        assert!(
            !threads.contains(retired),
            "retired thread operation remained: {retired}"
        );
    }
}
