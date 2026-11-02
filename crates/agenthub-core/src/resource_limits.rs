//! Fixed implementation budgets, not user settings or process RSS guarantees.
//! Reader caches are independent: active Arc snapshots and transient parsing /
//! serialization allocations can outlive retained entries. Do not sum these as
//! a hard process limit; the App, helper and WebView are separate processes.
use std::time::Duration;
const MIB: u64 = 1024 * 1024;
pub(crate) const SOURCE_CACHE_ENTRIES: usize = 64;
pub(crate) const CODEX_DETAIL_CACHE_BYTES: u64 = 128 * MIB;
pub(crate) const CODEX_SCAN_CACHE_BYTES: u64 = 8 * MIB;
pub(crate) const CODEX_INDEX_CACHE_BYTES: u64 = 16 * MIB;
pub(crate) const CODEX_QUESTION_CACHE_BYTES: u64 = 4 * MIB;
pub(crate) const SUBAGENT_LIFECYCLE_CACHE_BYTES: u64 = 4 * MIB;
pub(crate) const SUBAGENT_CACHE_BYTES: u64 = MIB;
pub(crate) const CLAUDE_DETAIL_CACHE_BYTES: u64 = 160 * MIB;
pub(crate) const CLAUDE_FILE_BYTES: u64 = 512 * MIB;
pub(crate) const CLAUDE_LINE_BYTES: u64 = 32 * MIB;
pub(crate) const CLAUDE_SUMMARY_CACHE_BYTES: u64 = 8 * MIB;
pub(crate) const CLAUDE_SUMMARY_ENTRIES: usize = 2000;
pub(crate) const GROK_DETAIL_CACHE_BYTES: u64 = 64 * MIB;
pub(crate) const GROK_ACTIVITY_CACHE_BYTES: u64 = 2 * MIB;
pub(crate) const NATIVE_PROBE_CACHE_BYTES: u64 = 32 * MIB;
pub const DETAIL_CACHE_IDLE_TTL: Duration = Duration::from_secs(300);
pub(crate) const HISTORY_REVISIONS: usize = 64;
pub(crate) const HISTORY_CHANGED_IDS: usize = 4096;
pub(crate) const STORAGE_CACHE_ENTRIES: usize = 512;
pub(crate) const STORAGE_CACHE_TTL: Duration = Duration::from_secs(30);
pub(crate) const STORAGE_QUEUE_ENTRIES: usize = 128;
pub(crate) const STORAGE_WORKERS: usize = 2;
pub(crate) const STORAGE_SCAN_ENTRIES: usize = 10_000;
pub(crate) const STORAGE_SCAN_DEPTH: usize = 16;
pub(crate) const STORAGE_SCAN_TIME: Duration = Duration::from_millis(250);
pub(crate) const READ_WORKERS_MIN: usize = 2;
pub(crate) const READ_WORKERS_MAX: usize = 4;
pub(crate) const CREDENTIAL_WORKERS: usize = 1;
// Shared native bridge constraints. A base64-encoded 20 MiB image fits below
// the 32 MiB response cap; neither bound limits all concurrent response memory.
pub const REMOTE_RESPONSE_BYTES: usize = 32 * 1024 * 1024;
// Local loopback (127.0.0.1) Connect-RPC limit for long Antigravity trajectories
// whose embedded executorMetadatas + multi-thousand-step payloads exceed 32 MiB.
pub const ANTIGRAVITY_LOCAL_RPC_BYTES: usize = 256 * 1024 * 1024;
pub const CREDENTIAL_WAIT: Duration = Duration::from_secs(15);
// Native desktop IPC uses one length-prefixed frame at a time. Consumers own
// returned snapshots; this bound is not a cap on retained UI state or RSS.
pub(crate) const CODEX_DESKTOP_FRAME_BYTES: usize = 64 * 1024 * 1024;
pub(crate) const CODEX_DESKTOP_CONNECT_WAIT: Duration = Duration::from_secs(10);
pub(crate) const CODEX_DESKTOP_START_WAIT: Duration = Duration::from_secs(90);
pub(crate) const CODEX_DESKTOP_STEER_WAIT: Duration = Duration::from_secs(30);
pub(crate) const CODEX_DESKTOP_HELPER_WAIT: Duration = Duration::from_secs(20);
pub(crate) const CODEX_DESKTOP_APPS_WAIT: Duration = Duration::from_secs(60);
pub(crate) const CODEX_DESKTOP_HELPER_PERMIT_WAIT: Duration = Duration::from_secs(60);
