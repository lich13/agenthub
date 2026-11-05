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
        "mod rpc_dispatch;",
        "mod system;",
        "mod threads;",
    ] {
        assert!(api.contains(module), "missing current API module: {module}");
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
}
