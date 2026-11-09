# AgentHub

[中文](README.md) | **English**

AgentHub (bundle identifier `com.lich13.agenthub`, daemon binary `agenthub-webd`) is a macOS-native workspace for multi-platform AI Agent session reading, real-time notification probing (**Probe**), and defense-in-depth session cleanup. Built on a ChatGPT macOS Desktop three-tier neutral surface (`--bg-rail` / `--bg-list` / `--bg-stage`), it unifies local sessions across six providers — **Codex**, **Claude Code**, **Grok Build**, **Grok Bot**, **Antigravity**, and **DeepSeek Harness (DSH)** — while managing multiple named remote Linux hosts over zero-port SSH `rpc-stdio` tunnels. Unarchived local Codex main threads can also connect directly to the native Codex desktop app for live collaboration.

v2.1.11 supports fresh installations and configurations already migrated by v2.1.0. Earlier installations must upgrade to v2.1.0 first; databases older than schema 3 must pass through v2.0.10. API protocol remains 2 and database schema remains 3.

## Highlights

| Pillar | Architecture & Distinctive Capabilities |
| --- | --- |
| **Six-Provider Unified Session Hub** | Native session browsing across Codex, Claude Code, Grok Build, Grok Bot (read-only local `.blob`/`.kblob` recent cache), Antigravity, and DeepSeek Harness; `⌘F` in-thread and cross-provider search (capped at 200 matches), one-click Thread ID & optimal handoff file path copy, macOS `Reveal in Finder` (`⌘⇧R`), and Codex/Grok Plan Markdown export. |
| **Native Codex Desktop Collaboration** | Open unarchived local Codex main threads directly in the native Codex desktop app or create new workspace threads; supports `⌘+Enter` send, mid-turn queue/steer/stop, encrypted local send queue, drag-and-drop/clipboard attachments & screenshots, `$` skills and `@` apps/plugins lookup, model/reasoning-effort/Plan mode switching, and live command/diff approvals. |
| **Frameless Streaming & Bounded-Memory Reading** | Preserves exact chronological ordering of prose and tool calls while collapsing consecutive reasoning/command steps into lightweight `<details>` disclosures; seamless reverse pagination at `200px` from the top with strict pixel scroll-anchor locking; shared 32 MiB / 64-entry history cache, 300s idle eviction, and K/M/G session size badges (`< 100 MiB` green, `100–300 MiB` amber, `>= 300 MiB` red). |
| **Console-First Probe & 5-Platform Real-Time Hooks** | Top-level master switch, encrypted Bark queue delivery, one-click per-provider notification toggles, and a unified 5-platform Hooks/Host matrix (one-click install/repair/remove for Codex, Claude Code, Grok Build, and Antigravity hooks, plus zero-intrusion DSH native `session.lock` + `.zstd` streaming), sharing atomic SQLite idempotent deduplication across `<1s` hook callbacks and 15s polling. |
| **GitHub CI Dual-Channel Monitor & Bot Filtering** | Automatically discovers public, private, and fork repositories for the active `gh` account on the selected machine; supports **persisted repository filtering** and **default-enabled Bot workflow run filtering** (`dependabot[bot]`, `renovate[bot]`, `github-actions[bot]`, and bot branches), delivering completion, failure, and timeout alerts to Bark and macOS notifications. |
| **Defense-in-Depth Cleanup & Zero-Port SSH Remote** | Covers 6 cleanup scopes across 5 providers with execution-side target recalculation, preview fingerprint intersection, active/lock verification, verified subagent closure, and symlink isolation; remote Linux hosts communicate exclusively via SSH `rpc-stdio` over a private Unix socket without listening on HTTP ports or requiring API keys. |

## Core Capabilities

- **Unified Reading & Search**: Browse Codex, Claude Code, Grok Build, Grok Bot, Antigravity, and DeepSeek Harness; press `⌘F` to search the active thread or current provider with historical pagination and native message jumping. Search stops after 200 matches, and active thread lookup is not restricted by recent list limits.
- **Grok Bot Recent Cache**: Dedicated local read-only entry for the active account's `.blob`/`.kblob` cache; copies verified local cache paths while keeping uncached threads visible with path actions disabled. Renaming, archiving, deletion, and message sending are not offered, and full cloud history is not assumed.
- **Frameless Streaming Typography & Process Folding**: Preserves the original sequence of assistant responses and tool invocations; consecutive reasoning, transition notes, and command/tool executions within a turn fold automatically into single-line `<details>` disclosures while final answers stand out clearly. `AGENTS.md`, environment metadata, and automatic context merge into a single "Context" entry, expanding automatically on search hits while respecting manual toggle state.
- **Optimal Handoff Paths & One-Click Reveal**: Copy thread IDs, copy the optimal handoff path per provider (Codex plaintext/archived/`.jsonl.zst` rollout, Claude primary JSONL, Grok `chat_history.jsonl` preferred, Antigravity plaintext transcript preferred, DSH plaintext/`.zstd` log), `Reveal in Finder` (`⌘⇧R`) on macOS, copy clean assistant Markdown replies, and export Codex/Grok Plans as Markdown.
- **Finder-Style Multi-Select & Archived Drawer**: Shared `⌘`-click toggle, `Shift`-click range selection, `⌘A` select all, `Escape` clear, `⌘⌫` batch delete, and context menus; double-click or press `F2` for inline rename. Codex, Antigravity, and DSH provide an on-demand collapsible "Archived" drawer at the bottom of the list supporting single/batch archive, restore, and permanent deletion including verified subagent closures.
- **Console-First Probe & 5-Platform Hooks/Host Matrix**: Presents the global master switch, Bark channel configuration and test push, one-click notification switches for all six providers, and consistent management cards for Codex (`~/.codex/hooks.json`), Claude Code (`~/.claude/settings.json`), Grok Build (`~/.grok/hooks/agenthub.json`), Antigravity (`~/.gemini/config/hooks.json`), and DSH (`session.lock` + `.zstd` stream).
- **GitHub CI Monitoring, Repository Filter & Bot Filter**: Displays GitHub Actions workflow runs for the active `gh` account on the selected machine; polls the latest 10 runs per repository every 60 seconds, refreshes owned public, private, and fork repositories every 10 minutes, and displays up to 20 runs after filtering. Supports persisted repository filtering (`github_ci_repositories`) and defaults to filtering out bot-triggered runs (`github_ci_filter_bots = true`, excluding `dependabot[bot]`, `renovate[bot]`, `github-actions[bot]`, and bot branches). When enabled, success, failure, and timeout events notify Bark and authorized macOS system notifications with direct links to the GitHub run. Initial observation, re-enabling, account switching, and filter toggling never replay historical runs.
- **Cross-Provider Cleanup with Execution-Side Guards**: "Settings → Updates & Maintenance → Cross-Platform Thread Cleanup" covers six scopes across five providers (Codex Archived, Codex Hidden, Antigravity Archived, DSH Archived, Claude Code Empty/Idle, Grok Empty/Idle) with streaming batch scans, 100-item paginated previews, and per-scope selection.
- **Named Remote Linux Host Management**: "Settings → Remote Connections" manages multiple named SSH hosts, reusing local OpenSSH config (`Include`, keys, `ssh-agent`, jump hosts) and supporting one-click "Install & Connect" for the matching `agenthub-webd` version. Switching machines isolates caches, pagination, and late responses without interrupting accepted background tasks.

All model inference and tool execution remain inside each agent's native runtime; AgentHub never bundles model runtimes or mutates unrecognized native session files.

## Native Codex Desktop Collaboration

Opening an unarchived local Codex main thread and clicking "Open in Codex" in the bottom floating collaboration bar launches the native Codex desktop app on that thread and establishes a local IPC connection; you can also create a new workspace thread via "New Thread". Historical browsing remains strictly read-only until collaboration is requested. Compatibility is determined by live handshake protocol and thread ownership rather than hardcoded build numbers, falling back cleanly to read-only mode if the protocol or owner changes.

The collaboration composer supports:

- **Send, Queue, Steer & Stop**: Press `⌘+Enter` to send; while a turn is running, queue messages for the next turn, steer the active turn, or stop the current turn and pause the local queue. Pending items are persisted with encryption in SQLite and require explicit confirmation after disconnect or restart; items with uncertain delivery status are never resent automatically.
- **Attachments & Clipboard Integration**: Native file picker, drag-and-drop upload, Finder file copy-paste, newline-separated absolute/`~/`/`file://` paths, and direct screenshot paste (PNG/TIFF/JPEG).
- **Skills, Apps, Plugins & Model Switching**: Type `$` to search skills, `@` to search apps and enabled plugins; click the model summary to switch native models, reasoning effort, and Default/Plan modes while synchronizing native Goal progress and command/file diff approvals.

See [Architecture](docs/ARCHITECTURE.md#本机-codex-桌面协作) for detailed protocol boundaries.

## Reading, Pagination & Storage Accounting

- **Seamless Bidirectional Pagination & Scroll Anchoring**: Main threads and Codex subagent views automatically load older pages when scrolling within `200px` of the top, preserving the exact pixel offset of the currently visible message across polling, message appends, and tool group expansions.
- **Bounded Memory & Read Reuse**: Unchanged session lists and histories are reused by file identity and modification metadata. Grok Bot, Antigravity, and DSH share a bounded **32 MiB / 64-entry** history cache, while DSH summaries evict beyond **8 MiB / 2,000 entries**. Detail views, Claude summaries, and Codex question caches share a **300-second (5-minute)** idle eviction window. Frontend polling pauses when the window is hidden and refreshes immediately upon visibility.
- **Session Size Badges (K / M / G)**: Session cards display byte sizes in monospace type with unified color thresholds: **`< 100 MiB` green**, **`100 MiB – 300 MiB` amber**, **`>= 300 MiB` red** (`—` while pending, `≥` for lower-bound estimates). Accounting covers only verified session rollouts, JSONL logs, dedicated subdirectories, and compressed streams — never project workspaces or symlink targets.

## Session Deletion & Cleanup Safety Boundaries

Direct deletion from session lists and "Updates & Maintenance → Cross-Platform Thread Cleanup" enforce strict execution-side verification:

1. **Fixed Confirmation Scope**: Codex, Antigravity, and DSH delete only the confirmed root IDs and verified descendants at execution time, excluding sub-sessions created after preview. Unified cleanup intersects preview fingerprints with the freshly computed allowed set.
2. **Provider Write Boundaries**: Antigravity checks active running state and offline locks; DSH archive, restore, and delete require the host app to be closed and verify per-session locks.
3. **Per-Item Reporting & Stop-on-Failure**: Direct batch operations report per-item successes, failures, and residuals; Antigravity/DSH subtree failures halt further deletion in that subtree, and unified cleanup stops on the first failure without rolling back already deleted items.
4. **Dedicated Scope & Symlink Isolation**: Only verified session files and dedicated sidecars are removed while updating shared indices; symlinks remove only the link itself and never traverse into external targets.

## Probe Notification Engine & 5-Platform Hooks

Probe exposes controls, status indicators, error diagnostics, event records, and confirmation objects; "Live Cross-Platform Posture" displays the aggregated overview capped at 12 recent items in the frontend.

Bark notifications follow a unified `Provider｜Status｜Thread Title` format (`Completed`, `Reply Needed`, `Authorization Needed`, `Failed`), with bodies distilled to at most 4 non-empty lines and 300 Unicode characters stripped of Markdown, code blocks, tool outputs, and internal metadata.

- **Per-Provider Notification Switches**: Toggle each provider independently on the Probe home view. Grok Bot is disabled by default and notifies locally for `send-message` replies as well as pending reply/authorization prompts with cursor and upgrade-baseline deduplication; cache eviction, replacement, re-enabling, and restarts never replay stale alerts. Antigravity and DSH notifications also operate locally.
- **4-Platform Real-Time Hooks + DSH Native Stream**:
  - **Codex**: Manages the Stop Hook in `~/.codex/hooks.json` with per-question identification and deduplication;
  - **Claude Code**: Manages `Stop`, `Notification`, and `StopFailure` hooks in `~/.claude/settings.json`;
  - **Grok Build**: Manages `~/.grok/hooks/agenthub.json`, reducing completion, failure, and reply-needed latency from 15s polling to `<1s` callbacks while retaining 15s polling as a fallback;
  - **Antigravity**: Manages the `"agenthub"` namespaced `Stop` hook in `~/.gemini/config/hooks.json` (returning `{"decision":"allow"}` immediately without blocking the IDE) for sub-second turn-end delivery deduplicated with 15s Language Server polling;
  - **DeepSeek Harness**: Monitors native `session.lock` and `.zstd`/v4 JSONL incremental streams plus `turn/end` settlement directly without external hook files.
- **Transport Safety & Bounded Retries**: Bark accepts only HTTPS endpoints or HTTP loopback addresses; transient network failures retry with bounded backoff, preserving queued items and unconfirmed chunks across upgrades and restarts. Initial enablement or re-enablement establishes a fresh baseline without replaying historical events.
- **Encrypted Payloads & Deduplication Retention**: Pending notification bodies are encrypted at rest and scrubbed upon terminal delivery, while retention pruning preserves deduplication identities needed by active cursors.

## Running & Deployment

### Local macOS App

Install `AgentHub-<version>-darwin-arm64.dmg` (`AgentHub.app`, bundle ID `com.lich13.agenthub`). The local installer quits any running instance, replaces `/Applications/AgentHub.app` in place, and verifies code signatures and version metadata.

### Remote Linux SSH Service

In "Settings → Remote Connections", select a saved SSH Host from your local OpenSSH config, toggle `sudo` if required, and click "Save & Connect" or "Install & Connect". Complete any interactive password, host key, or hardware key prompts in your terminal first; AgentHub never copies private keys, stores SSH passwords, or modifies `sudoers`.

The desktop app communicates over a persistent SSH connection running `agenthub-webd rpc-stdio` forwarded to the server's private Unix socket. Command-line deployment is also supported:

```bash
bash scripts/deploy-cloud.sh SSH_HOST ./dist/agenthub-webd-linux-x86_64.tar.gz
```

Always upgrade the remote Linux service before upgrading the macOS app. Full instructions are available in the [Cloud Deployment Runbook](docs/cloud-deploy-runbook.md).

## Release Assets

Current source version is **2.1.11**; see [GitHub Releases](https://github.com/lich13/agenthub/releases). Every official release publishes eight assets:

- `AgentHub-<version>-darwin-arm64.dmg` and `.dmg.sha256`;
- `agenthub-darwin-arm64.tar.gz` and `.tar.gz.sig` (Tauri updater archive and Minisign signature);
- `latest.json` containing the `darwin-aarch64` updater mapping;
- `agenthub-webd-linux-x86_64.tar.gz`, `.tar.gz.sha256`, and `.tar.gz.sig`.

## Development

```bash
pnpm --dir webui install --frozen-lockfile
node scripts/check-wire-types.mjs
node --test scripts/test-wire-types.mjs
node --test scripts/test-playwright-browser.mjs
cargo fmt --all -- --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo test --manifest-path src-tauri/Cargo.toml --locked
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --locked -- -D warnings
pnpm --dir webui typecheck
pnpm --dir webui test
pnpm --dir webui build:tauri
bash scripts/test-install-script.sh
python3 scripts/check-versions.py
python3 scripts/check-workspaces.py
python3 scripts/test-install-safety.py
python3 scripts/test-install-signatures.py
python3 scripts/test-privacy-check.py
python3 scripts/test-check-versions.py
python3 scripts/test-check-workspaces.py
python3 scripts/test-tauri-build-config.py
node --test scripts/test-security-audit.mjs
python3 scripts/privacy-check.py --git-objects
git diff --check
```

All unit and integration tests run inside isolated temporary sandboxes (`TempDir`) and never touch real user sessions on the host. Cross-engine Playwright (Chromium/WebKit) end-to-end regression and macOS headed frame-budget checks run in GitHub Actions CI.

## Documentation

- [README.md](README.md): Chinese documentation (中文说明).
- [AGENTS.md](AGENTS.md): Collaboration, safety, and delivery rules.
- [DESIGN.md](DESIGN.md): Three-tier surface, component, and interaction design specification.
- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md): Module boundaries, cache budgets, and wire contract.
- [docs/cloud-deploy-runbook.md](docs/cloud-deploy-runbook.md): Linux SSH service deployment, upgrade, and rollback runbook.
- [docs/progress/MASTER.md](docs/progress/MASTER.md): Maintainer release history and verification records.
