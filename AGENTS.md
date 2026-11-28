# Agent Instructions

## Delivery

- Work from the six current Markdown documents and the checked-in contract registry. Keep implementation, tests and acceptance evidence in source or the current status document; do not add release-specific Markdown or backups.
- Preserve user sessions, credentials, configuration and provider-specific behavior. Deployment hosts, domains, private paths and acceptance evidence are explicit runtime inputs and never repository data.
- For code delivery, select only the build, type, static, contract and privacy checks and targeted automated tests needed for affected code and concrete hard failures; keep full regression in existing CI. Complete authorized deployment/install with only necessary success or startup checks. Functional, UI, native-app and end-to-end acceptance defaults to the user from concise entrypoint, steps, expected results and coverage notes, unless explicitly requested or needed to reproduce/verify a concrete code failure. Mark it pending user acceptance without blocking assistant delivery, and report automated evidence separately. Routine rule maintenance does not trigger a release.

## Architecture

- Shared actions flow through `AgentHubUseCases` and the contract registry, then typed Swift 6 models/stores (`macos/Sources/AgentHubCore/`) and thin Linux (`agenthub-webd`)/FFI (`agenthub-ffi`) adapters.
- Codex, Claude Code, Grok, Antigravity and DSH provide session reading and explicit management actions. Claude rename retains native identity, version, ownership and fingerprint validation. Archive/restore retain their existing rules. Codex, Claude and Grok direct deletion is independent of content reading and activity; Antigravity and DSH retain their native running/lock checks. Unified cleanup retains its preview fingerprint and allowlist checks. Resolve targets from the selected machine, Provider, native ID or project-relative filename key; unlink directory symlinks themselves and only remove matching records from shared files. Never install Claude or infer permission waits from generic pending tools.
- The AgentHub Goal feature, automatic recovery chain and local Codex desktop collaboration (`codex.desktop.*` / `codex_desktop`) are retired. Unknown RPC names are rejected by the current positive allowlist; no retired-name table remains. Codex native logs are retained as read-only error-monitor input; no Goal scheduler, recovery retry or Goal database table remains. Existing SQLite `codex_desktop_submissions` tables, `<db>.desktop-attachments/` directories and Keychain entries on upgraded machines are left untouched without `DROP TABLE` so binary rollback to v2.1.30 remains lossless; fresh databases do not create `codex_desktop_submissions`.
- Probe keeps error detection, persistent cursors, dedupe, retention and encrypted Bark delivery. Provider identities and terminal evidence are validated independently. Codex ordinary feedback requests are classified locally only after a verified terminal turn. Native asynchronous questions remain pending across tool acknowledgements and normal completion; all notification entrances use the shared question tracker, identity recheck and persistent per-call delivery claim. Claude completion requires terminal evidence and no pending tools; an empty result falls back to the current turn's last assistant text. Compatibility warnings remain readable without blocking management; fatal parse, identity, path, permission or uncertain ownership errors still guard rename. Only fatal parse or identity errors suppress notification scanning. Claude hooks are explicit local-only settings operations; App installation never enables them. Hooks and local polling share verified native prompt identities and one claim per prompt/category; this rule never changes Codex per-question claims. Retain completed Claude turns on the current branch across later prompts. Check Claude failure freshness before first delivery and every retry; a new prompt, resumed execution, success or cancellation obsoletes the old failure and clears its queued payload. Main-turn completion does not wait for independent background shells or monitors, but a turn that ends while native agents or workflows are still pending (`turn_duration` pending counts, the Stop hook `background_tasks` list, or a verified owner reporting `busy`) is not the thread's completion. A native wake-up record with its own `origin` and `promptId` starts a separate notification turn without becoming a user message, title or timeline marker. Stop and StopFailure hooks carrying `agent_id` belong to a subagent and never notify for the session. A permission notice for a pending native question is claimed as that question. Claude running state comes from the verified session registration (`busy`/`waiting`/`idle`); native installations run under a version-named process that is admitted only after executable verification, and a session keeps its owner across recorded working-directory changes. New Bark events are one plain-text summary (four lines, 300 Unicode characters); preserve encrypted legacy chunks and their acknowledgements. Redact the full input before formatting; stop once truncation is established and remove only complete valid Markdown images, preserving ordinary exclamation/bracket text. Partial answers never change the original dedupe identity. Never replay questions older than the asynchronous enablement baseline.
- Settings use one `更新与维护` surface and the retired `system.status` action cannot execute; capability bootstrap uses `system.capabilities`. File-path Markdown actions must preserve the source path and never invent a web origin.

- The macOS App is the sole UI. Remote management uses system SSH with saved OpenSSH Host/Include aliases and a fixed `agenthub-webd rpc-stdio` command forwarding to the service-owned private Unix socket. Reuse native SSH identity, agent and jump settings without copying private keys or storing passwords. Keep protocol 2, command allowlists and connection revisions. Switching machines releases the owned channel, isolates previews, selection and cached responses; mutations are never replayed automatically. Only pending write requests block switching; accepted jobs continue in the service.
- HTTP management, API Key authentication, static assets and EventSource are retired. Their completed migrations live in v2.1.0; support fresh installs and already migrated configurations. Preserve business audit, Bark encryption and notification state.

Grok activity display uses one native-process snapshot per list and bounded incremental event reads. Deletion locates every matching native ID directory without parsing summaries or checking ownership, and removes corresponding active_sessions registrations. Prefer updates.jsonl for reading; when absent, map native chat_history.jsonl user, assistant and tool records into the existing history, search and timeline. System prompts, system_reminder and encrypted reasoning are not conversation content. Claude empty and metadata-only records remain readable without cwd or version. Main-list scanning, cached/fallback lookup and deletion indexing share the same dedicated-directory exclusion; a sibling JSONL or symlink owns that session directory.

## UI and safety

- Use the shared `RunningIndicator` for running Codex, Claude Code and Grok sessions. Codex, Claude Code and Grok group adjacent native tool activity at its original position between messages. Never discard historical tool rows; keep chronological call/result pairing and bounded pagination. Both levels use native `<details>` controls. Completed groups are closed by default; active and failed groups remain open. AGENTS.md rows and instruction-only groups always start closed; retain explicit user choices by session/activity identity. Plan copy and Markdown download use the same cleaned source as rendering, with safe title-derived filenames. Hide structured memory metadata before parsing, preserve code examples and ordinary prose, and never rewrite native files.
- Timeline rail markers represent only recognized user messages for each Provider. Build the full index independently of loaded pages and folded history; use stable native identities and bounded jump windows, preserving both pagination directions. Tools, plans, assistant replies and activity groups keep their original anchors for search and focus without becoming rail markers. Keep the rail and single hover preview narrow, with bounded mounted markers and keyboard access to every entry.
- Deletion accepts any number of explicit keys; archive/restore requests are split into batches of 100. A changed filter clears selection; polling never selects new rows. Active and archived selections are exclusive across click, keyboard, native menu and toolbar entrypoints; archive-toggle Select All selects its loaded rows only when expanded. One confirmation lists targets and native descendants; execution uses that fixed set. Per-item failures and residuals remain visible, missing files complete successfully, and a failed item never rolls back other targets.
- Never expose arbitrary shell, public Codex sockets, private deployment values or real session content in tests and packages. The management socket is private to the systemd service user; managed update and Probe jobs execute fixed programs with literal argument arrays. Reject unsafe Bark destinations before sending without substituting a default destination. Use reserved example values and a GitHub noreply commit identity.
- Keep systemd hardening (`ProtectSystem=full`, `ProtectHome=read-only`, `NoNewPrivileges=true`, `PrivateTmp=true`). Allow exact provider data roots in `ReadWritePaths` so shared indexes can be atomically replaced; protect Grok and Claude settings and credentials with optional `ReadOnlyPaths`. Resolve custom roots from the provider environment and Codex configuration. Missing provider storage must not block service startup.

- User-message presentation is separate from assistant Markdown. Preserve user whitespace and literal Markdown. Read native attachment bytes only through `sessions.attachmentRead`, using provider/session/message/attachment identity; never accept client paths or include image bytes in polling responses.
- Search is the read-only `sessions.search` action scoped to the selected machine and either the current thread or current Provider. Use cleaned visible content, omit memory metadata, preserve searchable AGENTS.md text, and return stable event/block positions rather than paths. The timeline rail uses the same anchors for messages, plans, attachments and activity groups; history loading preserves scroll height and disclosure choices.
- Shared user-message projection separates literal requests, attachments and optional contextBlocks (instructions, environment, runtime, files). Recognize only complete allowlisted envelopes outside code/quoted examples; support both native attachment request headings and preserve request whitespace. Render attachments, at most one closed “上下文” row, then the request; mount context text only when expanded. Search uses actual content, questions/answers, attachment names and AGENTS.md instructions, never raw wrappers. Context-only messages have no empty bubble or rail marker. Native question replies, shell operations and interruptions retain their dedicated handling. Stable machine/session/message identities preserve disclosure choices; no presentation cleanup writes back to native sessions.

## Release matrix

- CI keeps `Backend & contract checks` (`ubuntu-24.04`) and `macOS Swift 6 checks` (`macos-15`). On non-tag runs, `macOS Swift 6 checks` skips native toolchain, build and test steps when a `macos-verified-<hash>` cache marker from `main` matches the code tree hashed by `scripts/ci-code-tree-hash.sh` (excluding root `*.md` and `docs/**/*.md`); tag runs and `Backend & contract checks` always run in full. Tauri, WebView, Node/pnpm frontend builds, Playwright browser smoke, and Linux desktop packages are retired.
- Release keeps the macOS ARM64 app/updater (`scripts/package-macos-swift-arm64.sh`) and Linux webd (`scripts/package-webd-linux-x86_64.sh`) stages. The eight assets are the DMG/checksum, updater archive/signature, `latest.json` for `darwin-aarch64`, and webd tarball/checksum/signature. Verify archives with the preinstalled Minisign validator and trusted public key before extraction; never fall back to checksum-only updates.
- Cloud deployment remains manual and explicit through `scripts/deploy-cloud.sh`; production configuration is never committed.

## Required gates

Use the commands in README, plus `python3 scripts/privacy-check.py --git-objects`, `git diff --check`, contract checks and `bash scripts/test-install-script.sh`. Use normal incremental commits for subsequent changes.

## 管理边界

- 线程右键、省略号和键盘菜单共用动作模型；右键目标独立于当前详情。保留焦点返回、边缘避让、双击改名和批量模式。
- 单条、批量、归档清理和隐藏清理共用直接删除实现。Codex 覆盖普通／归档明文与压缩 rollout、writer 锁、shell snapshots、visualizations、原生状态和分页历史数据库、名称索引。父子关系去重遍历，循环不阻断；不反向扩大到引用方。执行仅处理确认时提交的 ID 集合，不加入后来出现的后代。
- Claude 定位键取项目相对路径及文件名身份；覆盖主 JSONL、同名目录、file-history、tasks、session-env、debug、todos，以及项目索引、共享历史和会话登记中的对应记录。Grok 按各工作区的 ID 目录定位，摘要缺失或损坏不影响删除。
- 删除不设批量数量、文件大小、目录深度和文件数量上限，目录流式遍历；文件、权限和数据库失败逐项报告，继续其他目标。fingerprint 是可省略的旧字段，不参与删除；归档及隐藏清理的 candidates 是固定执行范围。删除是一次性操作，原生程序重新生成的数据在刷新后呈现。
- Antigravity 与 DSH 提供独立会话阅读、搜索、附件、改名、归档／恢复和删除入口，并保留独立归档清理；左侧顶栏「工作区」跨平台总览页及 5 个代码 Agent 页内分组按 `.git` worktree 主仓库与规范化工作目录混排或筛选会话。Antigravity 同时纳管桌面端（`ANTIGRAVITY_HOME`／`~/.gemini/antigravity`，兼容 `--csrf_token` 与 `daemon/hub_*.json` 多实例并发端点枚举及按会话归属端点路由）与 `agy` CLI（`ANTIGRAVITY_CLI_HOME`／`~/.gemini/antigravity-cli`，离线根锁树级继承、5 源父链防漏及本机／远端 Linux 双端 Bark 通知），按主存储文件校验归属、排除跨端幽灵摘要与 `implicit/*.pb`，并支持同源无环父链核验的多级子智能体阅读与附件读取（`antigravity_subagents`）；离线改名／归档校验 `presence` 锁并同步 `raw_summary`、15 列整型纳秒 FTS5 `archived`、proto 缓存及对象安全的 `conversation_metadata.json`，永久删除按叶节点优先拓扑逆序覆盖 `conversation_parents`、未水合后代 `404` 磁盘残余与 CLI 共享文件，本机与远端 Linux 同步开放 `antigravity_archived` 已归档与无锁孤立子会话统一清理及 `settings.json` 只读保护。DSH 以标准 DSH_HOME 的 workspace v2 归档名单和 session 头的 origin=subagent/parentSession 核验归属，独立 fork 不扩入；只删除确认请求中的后代。归档、恢复和删除前要求对应机器的 DSH 退出，保留会话锁校验；改名遇到不完整 JSON 记录或 Zstandard 帧直接报错且不写原文件。请求不接受路径，共享索引只移除成功目标；旧服务缺少对应能力时禁用入口。
- 隐藏扫描只定位元数据候选及其原生后代，不读取正文或计算指纹。分批接口继续复用 cleanup.hiddenDryRun，每页最多 100 项；保留同源扫描协调、取消及空闲释放，候选总数不设业务上限。停止后的不完整结果不执行清理，机器切换隔离迟到响应。
- Pi 活动契约、读取、管理、界面和监测全部退役，未知命令统一拒绝，不维护退役命令墓碑表。旧数据库的 Pi 退役迁移由 v2.0.10 完成；保留其原生程序与用户数据。
- API 协议为 2；先升级服务再安装 App。主窗口隐藏创建，仅初始化时适配一次工作区，Ready 和 Reopen 只显示、恢复及聚焦。
- 发布前检查当前树、索引、可达历史和资产。仅发现可达历史敏感值时才按授权重写；平台保留的不可达对象另行记录，不声称已完全擦除。

## Codex 账号隔离

- `threads.accountScopeStatus`／`threads.accountScopeClear` 属于可缺省能力 `codex_account_scope`，协议保持 2；旧服务不显示入口。响应只含数量、线程 ID 与原因，不含路径或会话内容，业务审计只记数量。不读取 Codex 凭据文件，不修改原生数据库、索引或创建者身份。
- 只处理 `sessions/`、`archived_sessions/` 下文件名、`session_meta` 类型与 `payload.id` 一致的 rollout 首行：把 Codex 写出的精确标记片段替换为等长空白，替换后的 JSON 必须等于原值去掉该字段，否则记为格式不支持并保持原文件。明文文件原地写入后恢复 mtime，inode、长度和偏移不变；压缩文件在同目录重编码、核对内容摘要后原子替换，保留属主、权限和 mtime；临时副本存续期间持有文件锁，被终止的遗留副本由下一次解除清理。配置的 Codex 数据目录即根目录，手动与自动入口都不进入符号链接的 `sessions/`、`archived_sessions/`，也不跟随其下的符号链接。
- 只以只读方式打开 Codex 已有的锁文件，绝不创建锁文件或目录。原生压缩维护进行中整次返回错误并提示稍后重试，自动模式直接跳过；压缩文件替换前取得协调锁并确认线程未被写入，线程锁只在协调锁内探测，缺少协调锁或线程正在使用记为忙碌。回退副本只在同线程原始文件可读、`payload.id` 一致且已无标记时处理，否则保留并报告缺少原始会话。
- 单次调用在扫描后约 20 秒返回剩余数量，由界面在仍有进展时继续调用，写请求不自动重放。`probe.hooks.clear_account_scope` 默认关闭，只在已安装的 Codex Stop Hook 内、建立通知事件之前处理本回合线程及已登记后代的明文原始文件，失败不影响 Hook 输出；压缩文件、回退副本和未安装 Hook 的机器由手动动作处理。
- 清除改变 rollout 首行，该线程正在等待重试的 Codex 通知按来源已变处理、不再发送。Codex 若启用恢复时的账号校验，本功能不能使其通过。

## 阅读与子智能体边界

- 各 Provider 共用紧凑阅读基线；复制动作不增加正文行高，用户气泡、工具两层折叠、Plan、搜索与用户指令时间线保持原规则。
- Codex 子智能体只通过原生调用身份、精确任务路径和父子关联读取，不按显示名称猜测。只使用子线程自己的回合证据判断运行状态。
- `threads.subagentDetail` 和带主线程上下文的子线程附件必须校验可见主线程、唯一父链、深度、循环、文件身份与指纹。无任意路径、执行控制或子线程管理能力。
- 子智能体能力缺失时按不支持处理，API 协议保持 2；机器切换关闭面板、隔离缓存并丢弃迟到响应。
- Hook 集成测试必须启动本次 Cargo 构建的 CLI；不得跳过测试或借用已安装的正式服务。自动化离屏渲染与真实 App 入口验收分别记录。

## SSH 恢复与配置管理

- 现有刷新／请求发现断线才恢复连接，同轮共享握手；读取最多自动重试一次，前端查询不叠加远程重试，写操作不重放。传输失败或超时立即关闭通道，业务错误保留通道。恢复不增加连接修订号，不重置页面；切机或退出丢弃旧握手。启动时已保存的远程目标立即返回连接视图，首次握手在后台进行，首个远程请求加入同一握手。
- remote.sshDelete 仅接受 revision 和 alias。删除用户及 Include 中的全部显式定义，多别名仅去掉所选项，系统配置拒绝删除。删除指令时同时移除其行尾注释，保留独立注释、文件权限和其他配置；同目录唯一临时文件原子替换并在保存前核对变化，部分失败报告已完成项，不跨文件反向回滚；删除当前目标释放 SSH 并回本机。
- 日志按 CR／LF／CRLF 整条解码脱敏后，在原读取任务中累计 16KiB 或 200ms 写库，结束前排空；不增加日志后台任务、状态文件或任意单行硬上限。

## 凭据边界

- 保存与移除连接使用现有变更保护与原子配置写入；切换或退出时释放自有 SSH 进程。旧远程钥匙串迁移由 v2.1.0 完成。
- SSH 能力验证保持连接变更保护；请求校验修订号及目标，丢弃迟到响应。固定错误消息不含原生异常、Key 或地址。测试不得访问真实钥匙串。
- 桌面与 monitor 共用运行配置密钥入口；新环境在既有受限 env 文件生成随机密钥，有效密钥保留，配置损坏或密钥异常明确报错。支持空库和 schema 3，旧库须先经过 v2.0.10；不再预开数据库或抽样解密，密文在实际读取时校验，不触碰既有 Keychain 条目或遗留桌面表。

## 子智能体活动与汇总

- 原生 `item_completed` 中的 `SubAgentActivity.kind` 表示历史活动，外层完成不能当作子线程终态。保留开始、完成、中断及交互时点，并与同次创建调用/结果去重。
- 活动、列表和详情共用任务名解析，优先 `task_name`，再使用已验证路径末段；不让昵称或角色覆盖任务名。
- 直属集合在分页前按原生身份去重，当前状态只依据子线程自身回合。父线程未变时也刷新，保留内部完整性标记及各条目的状态与不可用原因；主线程列表不显示子代理数量。
- 汇总和嵌套列表复用只读详情边界、机器隔离及焦点/滚动恢复。API 协议保持 2，不新增 RPC、数据库表或原生写入。
- 完整内部 `external_codex_apps_open_page` JSON 包装从显示、复制、导出和搜索排除；代码与引用示例保留，不产生空用户气泡或时间线标记。

## 界面说明

Probe 不显示说明小字、副标题、平台事件或 Hook 机制解释，保留状态、错误及确认对象；“全平台实时态势”沿用原聚合和数量限制。执行记录列表与后台轮询（包括终态）只返回摘要；jobs.detail 的 includeOutput 默认 true，后台传 false，仅展开读取全文，终态继续刷新相关状态。

子智能体汇总和列表不显示通用的统计不完整说明。归档与隐藏线程清理不显示重复描述，保留状态、预览、确认及逐项结果；保留一次确认及固定目标范围，沿用各平台已有写入校验。

## 指令与通知边界

- 完整原生 INSTRUCTIONS 包装允许结束标签与后续环境标签同行；内部标题不能截断折叠。代码、引用示例与不完整结构保持保守处理，原生会话不写回。
- Gotify 已退役；配置迁移由 v2.1.0 完成，不维护旧标识扫描门禁或不可执行契约墓碑。移除其配置、Token、队列、测试任务和投递字段时，必须保留 Bark 凭据、基线、成功/未知记录及待投递状态。
- Bark 使用下述有限重试规则。接口协议保持 2，不改变 Provider 或事件识别规则。
- 外部通知服务卸载归运维仓库，须明确配置与数据归属；不改共用 Nginx/TLS、其他服务或用户原生会话。

## Bark 投递边界

- Antigravity 通知在本机 macOS（受管 `~/.gemini/config/hooks.json` Stop Hook + 15 秒轮询）与远端 Linux `agenthub-webd`（15 秒只读轮询，Hook 写入仍仅限本机）同步开放 `antigravity_notifications`；DSH 仅在本机运行通知，两者均通过可选能力展示总开关和四类开关，默认关闭，协议保持 2。沿用 Probe 轮询、数据库和加密队列，首次启用／重新启用及升级首次扫描 CLI 存储时建立基线（`antigravity_cli_resume_cutoff_ms`）。Antigravity 只读原生在线服务与离线 `.db`／转录快照，明确执行终态、正文／工具提问（`ask_question`／`RequestFeedback`）与权限等待才产生对应事件；DSH 只读标准 v4 日志及多帧 Zstandard，按主回合、问题和审批身份配对。子代理和 fork 继承记录不成为主线程事件；读取或身份不明显示异常，不推断终态。首发与重试均复核，恢复执行淘汰旧失败，问题和审批解除后终结重试；异步问题超时仍可回答时继续待回复。

- 每分段最多 3 次，重试等待 15/60 秒并加入 ±20% 抖动；首次投递起 10 分钟内有效。尊重 Retry-After，不提前发送或无限保留。
- 网络失败、结果未知和明确临时拒绝可重试；凭据、证书验证、重定向、非法请求及已知永久 APNs 拒绝不能重试。日志仅含安全分类、状态码、耗时、次数和匿名身份，不含原始异常、地址、Key 或正文。
- 使用加密不可变队列、持久化分段确认与带归属校验的租约。成功分段不重发，Hook 不等待后台重试，monitor 每轮最多处理 10 项。每次重试及来源复核后重新读取配置、核对当前状态和目标；正常追加不等于来源变化。
- 原生待发正文使用已有加密组件；交接给 Bark 队列及终态后清除原生正文副本，只保留去重身份、结果及界面短摘要。升级保留基线、认领、重试与成功分段；保留期清理包含 unknown 终态，但保留当前游标需要的最小去重身份，游标推进后再清理。空闲轮询不重写未变游标，不执行无变化的全历史同步。稳定 Bark ID 不能被描述为绝对去重保证。
- 测试通知使用“AgentHub 推送测试”与机器来源正文，作业等待真实终态，HTTP 成功且 Bark code=200 才算受理。不提供旧配置导入入口。

## 历史与大小边界

- Codex、Claude、Grok 和子智能体共用自动上滚分页；初次定位最新内容，不自动读完整条历史。搜索与滚动按机器、线程及游标协调，错误或游标不推进时停止；可见锚点与像素偏移优先于旧滚动高度。
- 轮询不能丢失已加载历史和最早游标；机器、线程或文件身份改变时丢弃旧响应。保留独立子面板、工具折叠与用户指令时间线。
- 大小只按已验证的 rollout 或会话专属目录统计逻辑字节，不读正文、不计算全内容哈希、不跟随符号链接、不扫描项目或共享存储。目录后台扫描和缓存必须有界，身份变化立即失效。
- `storageSize` 为可选摘要字段，未知、待统计和部分结果不能冒充完整大小；旧服务仍可读取。API 协议保持 2，不新增命令、表或客户端路径参数。

## 读取性能

- 读取使用有界后台执行与不可变快照；Codex／Claude／Grok 沿用增量缓存。Codex 活动扫描与 Grok 摘要缓存的条目上限为 2,000，字节预算保持 8MiB／2MiB，先到者生效。Antigravity／DSH 复用未变化历史及时间线，正文缓存合计最多 32MiB／64 项，DSH 摘要最多 8MiB／2000 项，超预算正常读取但不缓存。Antigravity 在线列表合并磁盘索引中原生服务尚未加载的根会话（在线条目优先，子代理与原生存储已不存在的索引行不列出）；在线复用仅限原生身份、摘要与终态未变化且完整的会话，运行中继续读取；文件替换、截断或修改立即失效，附件按请求刷新。不得把历史快照复用写成完整增量解析。
- 历史变更元数据和 Grok 分页是协议 2 的可选扩展。旧客户端保持原响应行为；新客户端只在来源支持时使用稳定块游标。Antigravity／DSH 快照依据实际内容计算 revision，时间线随快照生成并按实际容量计重；继续保守重读旧页，历史工具、问题与子智能体状态仍需更新。
- K／M／G 只表示单位，颜色按实际字节在 100MiB、300MiB 分界；未知或待统计为灰色，部分结果按已确认下限分色并保留 ≥ 与不完整提示。
- 常规受影响检查在本机运行，完整回归和跨平台构建交由现有两项 CI（`Backend & contract checks` 与 `macOS Swift 6 checks`）。性能比较使用运行时合成数据及相同样本，区分冷读、热读、追加、分页传输和远程网络耗时；不将 fixture 结果记为真实入口验收。
- 原生界面与排版回归由 `AGENTHUB_RENDER_TESTS=1 swift test --package-path macos` 在本机与 GitHub Actions `macos-15` 执行，覆盖浅／深色真实视图离屏光栅化、字形非空白墨迹、布局热区、输入法组字与反向分页像素锚点断言。

Claude native command envelopes render as the slash command and exact arguments. Classify isMeta, isCompactSummary and complete automatic envelopes before mutating prompt state; automatic records never start a human turn or clear pending tools. Adjacent metadata stays in place in closed system/context groups, including the preamble. Preserve native anchors, disclosure aliases, literal examples and instruction search; compaction remains searchable and closed by default.

## Grok Bot

- `grokbot.list`／`grokbot.detail` 仅在桌面注册，复用公共历史 DTO、搜索和缓存。只读取当前账户 roster 与近期 transcript 切片，不扫描远程目录、不写原生会话。账户与线程共同确定身份；路径复制只返回实际存在的本线程缓存文件。
- Grok Bot 恢复 v2.1.16 的消息级 Bark：非失败 send-message 文字／附件及明确待回复／待授权记录可产生通知，默认关闭。沿用账户／线程／消息或请求身份、消息游标、启用基线、加密队列与首发／重试来源复核；暂停期间已扫描的消息不能在升级后补发。消息级 completion 不证明整轮终结，不能把 requestId、最后一条消息、单工具状态或静默计时描述为可靠整轮结束证据。
- 只读入口沿用共享菜单、阅读区、字体与间距；待处理交互使用“需要回复／需要授权”，不显示运行 spinner，不增加管理动作、说明小字或 Hook 占位。

## GitHub CI

- 读取来源机器当前用户的活跃 `gh` 账号；凭据仅在该机器后端内存使用，不写数据库、响应或日志。`github.ci.status` 是可选 `github_ci` 能力下的共享查询；协议 2、schema 3 保持。
- 两端监控默认关闭。存在符合筛选条件的未结束运行时 5 秒、空闲时 60 秒查该仓库首页 `per_page=10`，不翻运行历史、不追查窗口外旧运行或重跑；10 分钟刷新账号名下公开／私有／fork 仓库，保留目录分页、HTTP 连接复用、ETag 和凭据同源检查，刷新目录时淘汰已消失仓库缓存。前端先筛仓库再按更新时间最多展示 20 条，不用展示上限截断通知判断。
- 扫描中的刷新由本轮结果满足，保留启停与基线变化检查。限流时间仅在 Monitor 内存保留，限流刷新须结束 refreshing 并保留错误反馈；旧状态仍可读取。App／Hook 后台启动条件包含 CI 开关；关闭 CI 时仅返回缓存，不请求扫描或持续显示 refreshing；桌面不另持有 Monitor。账号与 gh 凭据按 60 秒节奏核对；快速轮询复用内存凭据及解析后的精简 ETag 摘要，未变结果不额外写库。
- 首次／重新启用及账号变化建立基线，已有完成记录不补发，在途运行结束正常通知。账号、仓库、run ID、attempt 共同去重；关闭、切号或新 attempt 淘汰旧待发通知。Monitor 直接返回可投递事件，已处理项批量确认一次，失败项保留；沿用过期过滤、首发校验、加密 Bark 队列与重试复核，链接使用 GitHub 返回的 html_url，兼容旧队列。

- CI 系统通知在 macOS App 运行期间每 5 秒串行读取当前机器缓存，不依赖页面可见性；`cached_only` 不触发扫描，`baseline_ms` 沿用既有监控基线。使用来源机器的仓库／Bot 筛选和已成功扫描仓库集合；新增或重新加入的仓库独立建立基线，空仓库后续新运行照常通知。首次观察、换机、切号、重新启用不补发旧结果；仅成功、失败、超时，点击打开对应运行。复用原生 UserNotifications 和已有 SSH 通道，观察去重只在内存；Bark 的持久化、确认与重试独立保留。
- 隐藏清理扫描只接受 start/poll/cancel，复用同一连接中的线程行与父子关系；保留两种父 ID 字段及 spawn edges、固定候选、分页和取消。通知保留期维护不提供 dry-run 统计，标记批量删除保留保护集合；Probe 配置通过共同原子写入入口替换实际文件并保留权限。

- 关闭的 Provider／错误监控只在状态或统计变化时写库，启用状态下状态／游标／扫描行数／新增异常变化时立即写库，完全空闲未变的 `"ok"` 心跳按 300 秒节流写库；Claude Hook 配置只为启用的 Claude 读取。Probe 聚合沿用受限阻塞读取任务。Mac CI 有待发事件才请求授权，由原生回调发送该批事件，观察循环不等待；消费前校验来源，已接受通知按原机器名发送，稳定事件身份不包含连接修订；不增加授权重试状态。

## 读取与保存边界

- Probe 复用当轮会话定位，仅为候选读取通知状态，不统计目录大小；后台通知扫描范围不缩减。实时活动态势覆盖六大 Provider（Codex、Claude Code、Grok Build `active_sessions`＋内核进程存活性、Antigravity 在线多实例与离线 CLI 锁、DSH `session.lock`＋未结回合／提问／审批，以及本机 Grok Bot 待回复／待授权）。
- Antigravity／DSH 列表完成筛选、排序和截取后才请求大小；详情继续按需统计。DSH 通知缓存只受现有 32MiB 字节预算及空闲释放约束；不要与 64 项历史缓存混淆。Grok 通知 JSONL 共享不可变数组，DSH／Grok 派生条目只在变更时计重。
- 通知先定位目标再复核对应来源，保留首发与重试前的身份、重名和状态检查。DSH 常规删除保留排他租约、inode 和后代归属校验；统一归档清理只由 execute 扫描和校验一次，保留逐项结果。
- 配置补丁为空时不写 TOML；生成并校验后的文本与原文相同也不替换。实际变更原子替换配置路径并保留权限，不维护符号链接；Key 写入仍加密，业务审计保持。
- 统一清理预览使用已有 NoRequest 无参数契约，兼容旧空请求；内部固定范围用枚举，执行继续校验外部范围、确认、目标与指纹。隐藏扫描能力用于入口识别，分页器负责终态与停止回调。协议保持 2、schema 保持 3。
- Hook 仅查询对应 Provider 的待发通知，筛选必须先于限量、解密和领取；后台轮询仍处理全部来源。首发和重试沿现有检查读取当前配置，服务端和文件入口复用现有文件配置缓存，扫描入库及通道启停同步在设置保存所用事务内重读配置；旧排队事件不改投新目标。
- DSH 的 1 万条通知扫描限制不扩展到普通列表；Probe 没有候选时返回空扫描，解析错误计数和重复身份检查保持。配置保存仍解析磁盘文本并读取密钥，不能用旧内存配置代替。

- 多选行点击显式保留焦点；列表、对应选择操作栏及 macOS Edit → Select All 共用选择动作，只选择本范围已加载且已筛选的行。输入控件保留文字全选，弹窗不选择底层列表。
- Grok Bot 仍待处理的免游标交互在保留期维护前按小时续期 sent／failed／skipped／unknown 去重身份；读取失败不得视为交互消失。升级秒级扫描时间覆盖该秒全部毫秒。附件通知只保留名称，原始阅读链接不变。
- Claude 统一清理执行只接受 Inactive；普通显式删除仍不检查活动。跨项目同 ID 的批次只建立一次文件元数据索引；任一副本保留或失败时保留共享 sidecar、历史、登记及原生后台作业，最后全部成功才清理。

- CI 来源持久化各仓库首次成功扫描基线与取消 Bot 筛选的截止时间；Mac 观察器沿用来源基线，即使错过中间配置快照也不补发旧完成。HTTP 请求不持有数据库事务，完成后在同一短事务中核对启停、筛选、基线并写回；保留并发确认。凭据／账号失败与目录错误按 60 秒或实际 Retry-After 重试，失败不延长账号凭据有效期。
- Grok Bot 升级截止时间只在既有 settings 中建立一次，覆盖已有非零游标；后续扫描不推进该时间。Claude 文件索引跳过有主 JSONL 的专属目录，不设深度上限；Grok 统一清理仅接受 Inactive，未知占用记失败，普通删除保持。
- Codex 列表在补充正文前筛选归档状态。Antigravity 的进程表只在单次根会话读取中按需获取、解析一次，根与相关子代理共享，下次读取重新获取；继续在阻塞任务中匹配。原生全选没有整页回退，输入文字和列表各用原有范围，dialog／ARIA 弹窗及菜单不触发底层列表全选。
