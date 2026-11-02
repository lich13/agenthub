pub mod antigravity;
pub mod archive;
pub mod claude;
pub mod codex;
pub mod config;
pub mod crypto;
pub mod db;
pub mod dsh;
pub mod grok;
pub mod history_navigation;
pub mod jobs;
pub mod jsonl_cache;
pub mod local;
pub mod platform;
pub mod probe;
pub mod probe_error_monitor;
pub mod providers;
mod read_cache;
pub mod read_tasks;
pub mod resource_limits;
mod scan_work;
pub mod security;
mod selected_codex;
pub mod services;
mod session_delete;
pub mod session_storage;
pub mod shell;
pub mod system;
pub mod update;

pub use config::Config;

pub mod native_probe;
pub mod provider_sessions;

pub mod user_message;

pub fn evict_idle_detail_caches(max_idle: std::time::Duration) {
    codex::evict_idle_detail_cache(max_idle);
    claude::evict_idle_detail_cache(max_idle);
    grok::evict_idle_detail_cache(max_idle);
    native_probe::evict_idle_cache(max_idle);
    dsh::notifications::evict_idle_cache(max_idle);
    dsh::sessions::evict_idle_fold_cache(max_idle);
    services::probe::evict_idle_live_provider_sessions_cache(max_idle);
    antigravity::sessions::evict_idle_endpoint_cache(max_idle);
}
