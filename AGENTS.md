# Agent Instructions

## Delivery

- Work from the six current Markdown documents and the checked-in contract registry. Keep implementation, tests and acceptance evidence in source or the current status document; do not add release-specific Markdown or backups.
- Preserve user sessions, credentials, configuration and provider-specific behavior. Deployment hosts, domains, private paths and acceptance evidence are explicit runtime inputs and never repository data.
- For code delivery, select only the build, type, static, contract and privacy checks and targeted automated tests needed for affected code and concrete hard failures; keep full regression in existing CI. Complete authorized deployment/install with only necessary success or startup checks. Functional, UI, native-app and end-to-end acceptance defaults to the user from concise entrypoint, steps, expected results and coverage notes, unless explicitly requested or needed to reproduce/verify a concrete code failure. Mark it pending user acceptance without blocking assistant delivery, and report automated evidence separately. Routine rule maintenance does not trigger a release.

## Architecture

- Shared actions flow through `AgentHubUseCases` and the contract registry, then typed WebUI query/domain code and thin Linux/Tauri adapters.
- Codex, Claude Code and Grok are read-first providers with explicit rename and direct deletion. Local Codex main threads additionally support collaboration through a verified native desktop owner; remote threads, subagents, Claude and Grok remain read-first. Claude rename retains native identity, version, ownership and fingerprint validation. Archive/restore retain their existing rules. Deletion is independent of content reading, process activity, writer locks, terminal state, versions, fingerprints and external references. Resolve targets from the selected machine, Provider, native ID or project-relative filename key; unlink directory symlinks themselves and only remove matching records from shared files. Never install Claude or infer permission waits from generic pending tools.
- The AgentHub Goal feature and automatic recovery chain are retired. Goal RPC names remain only as unavailable contract tombstones. Codex native logs are retained as read-only error-monitor input; no Goal scheduler, recovery retry or Goal database table remains. The separate desktop outgoing queue stores only user-submitted messages and never runs models or edits the native queue.
- Probe keeps error detection, persistent cursors, dedupe, retention and encrypted Bark delivery. Provider identities and terminal evidence are validated independently. Codex ordinary feedback requests are classified locally only after a verified terminal turn. Native asynchronous questions remain pending across tool acknowledgements and normal completion; all notification entrances use the shared question tracker, identity recheck and persistent per-call delivery claim. Claude completion requires terminal evidence and no pending tools; an empty result falls back to the current turn's last assistant text. Compatibility warnings remain readable without blocking management; fatal parse, identity, path, permission or uncertain ownership errors still guard rename and collaboration. Only fatal parse or identity errors suppress notification scanning. Claude hooks are explicit local-only settings operations; App installation never enables them. Hooks and local polling share verified native prompt identities and one claim per prompt/category; this rule never changes Codex per-question claims. Retain completed Claude turns on the current branch across later prompts. Check Claude failure freshness before first delivery and every retry; a new prompt, resumed execution, success or cancellation obsoletes the old failure and clears its queued payload. Main-turn completion does not wait for independent background tasks. New Bark events are one plain-text summary (four lines, 300 Unicode characters); preserve encrypted legacy chunks and their acknowledgements. Partial answers never change the original dedupe identity. Never replay questions older than the asynchronous enablement baseline.
- Settings use one `更新与维护` surface and the retired `system.status` action cannot execute; capability bootstrap uses `system.capabilities`. File-path Markdown actions must preserve the source path and never invent a web origin.

- The macOS App is the sole UI. Remote management uses an independent `x-api-key` stored in Keychain and a fixed allowlisted Rust HTTP bridge; never send keys from browser fetch, log them or permit redirects. API protocol and connection revisions gate requests. Switching machines cannot reuse previews, selection or cached responses; mutations are never replayed automatically.
- Old web authentication, static assets and EventSource are retired. Preserve API authentication throttling, business audit, Bark encryption and notification state. Database erasure is limited to AgentHub's own retired web credentials, Pi integration and Gotify records; never migrate native provider databases.

Grok activity display uses one native-process snapshot per list and bounded incremental event reads. Deletion locates every matching native ID directory without parsing summaries or checking ownership, and removes corresponding active_sessions registrations. Prefer updates.jsonl for reading; when absent, map native chat_history.jsonl user, assistant and tool records into the existing history, search and timeline. System prompts, system_reminder and encrypted reasoning are not conversation content. Claude empty and metadata-only records remain readable without cwd or version.

## 本机 Codex 桌面协作

- `codex.desktop.*` 只注册到 macOS Tauri，不进入 Linux RPC 或远程 allowlist；API 协议保持 2。使用固定原生 IPC 方法和版本，逐次核验本机 owner、线程、当前回合及审批请求。身份、状态修订或协议不明立即停止写入；有界恢复取得新 snapshot 后仍须核验 owner，owner 改变仅恢复阅读，显式连接确认才能写入。不能回退到独立 CLI 执行或整表覆盖原生队列。
- 独立短生命周期 helper 只使用固定目录读取及创建空线程；对话执行始终交给已打开线程的原生桌面。删除直接操作原生存储，不调用 helper、不启动回合或终止原生进程。文件和目录由 Rust 原生选择器授予不透明 handle，目录引用来自原生 catalog；不接受 WebUI 文件路径。队列中的文件引用发送前重验身份。
- 应用目录使用无线程请求、同一 helper 完成分页；五种目录独立失败和重试，设置成员由原生目录验证。诊断只保存有界内存中的白名单字段，不能保留或输出原生自由文本。只有已核验的执行前拒绝可判定未发送，其他提交异常保持结果不明。
- AgentHub 自有队列使用专属 Keychain 密钥加密，并持有跨进程唯一 worker 锁。发送前持久化认领，收到确认后清除正文；断线/重启保留暂停项，结果不明不得自动重发。停止先暂停本线程队列；补充绑定当前回合，失败补充不能转成下一回合。
- 内联引用的 UTF-8 区间和名称必须由 Rust 对已授权 handle 校验，WebUI 不提供原生 path 或 wire 对象；新引用按已核验的原生链接序列化，保留空 text_elements 和旧队列正文逐字节兼容。协议方法与版本集中在固定白名单，仅以核验后的桌面证据扩展。
- 兼容性以实际方法版本、状态与 owner 校验为准，build 只作诊断，不设 build 白名单或试探未知写入版本。新选择的小文件可用有界内容摘要确认单纯 ctime 变化，规范路径、设备/inode、大小、mtime、所有者、权限及内容必须保持；旧引用和大文件继续严格核验。
- 原生拖放和显式文件/图片粘贴只在 Rust 接收；WebUI 不传路径或字节，通用 Tauri event listen 保持禁止。Finder 文件和完整路径文本仅从系统剪贴板授权，整批验证后附加，文本回退保留原始内容；不读取 HTML 图片地址或执行路径中的 shell 语法。私有图片目录/文件权限为 0700/0600，生命周期索引专属密钥加密；发送前持久化保留标记，队列终结或取消不自动删除可能被原生历史引用的图片。只清理可证实归属的未提交草稿，缺失密钥/损坏索引/未知文件保留并停止。
- 原生 Goal 仅从已有快照同步目标与状态，操作通过既有“在 Codex 中打开”入口完成；不恢复退役 Goal 调度器，不新增未经核验的 follower Goal 写入方法。
- 审批和问答以完整原生请求及文件 diff 身份绑定。未完整显示或不支持的表单留在 Codex 中处理，不默认批准；仅声明支持的命令/文件审批可使用已核验会话决策，权限仍限本回合。机器切换、隐藏窗口和离开页面释放界面订阅，已授权的本机队列仍只属于原来源。

## UI and safety

- Use the shared `RunningIndicator` for running Codex, Claude Code and Grok sessions. Codex, Claude Code and Grok group adjacent native tool activity at its original position between messages. Never discard historical tool rows; keep chronological call/result pairing and bounded pagination. Both levels use native `<details>` controls. Completed groups are closed by default; active and failed groups remain open. AGENTS.md rows and instruction-only groups always start closed; retain explicit user choices by session/activity identity. Plan copy and Markdown download use the same cleaned source as rendering, with safe title-derived filenames. Hide structured memory metadata before parsing, preserve code examples and ordinary prose, and never rewrite native files.
- Timeline rail markers represent only recognized user messages for each Provider. Build the full index independently of loaded pages and folded history; use stable native identities and bounded jump windows, preserving both pagination directions. Tools, plans, assistant replies and activity groups keep their original anchors for search and focus without becoming rail markers. Keep the rail and single hover preview narrow, with bounded mounted markers and keyboard access to every entry.
- Deletion accepts any number of explicit keys; archive/restore requests are split into batches of 100. A changed filter clears selection; polling never selects new rows. One confirmation lists targets and native descendants; execution uses that fixed set. Per-item failures and residuals remain visible, missing files complete successfully, and a failed item never rolls back other targets.
- Never expose arbitrary shell, public Codex sockets, private deployment values or real session content in tests and packages. Validate loopback API listeners before startup; managed update and Probe jobs execute fixed programs with literal argument arrays. Reject unsafe Bark destinations before sending without substituting a default destination. Use reserved example values and a GitHub noreply commit identity.
- Keep systemd hardening (`ProtectSystem=full`, `ProtectHome=read-only`, `NoNewPrivileges=true`, `PrivateTmp=true`). Allow exact provider data roots in `ReadWritePaths` so shared indexes can be atomically replaced; protect Grok and Claude settings and credentials with optional `ReadOnlyPaths`. Resolve custom roots from the provider environment and Codex configuration. Missing provider storage must not block service startup.

- User-message presentation is separate from assistant Markdown. Preserve user whitespace and literal Markdown. Read native attachment bytes only through `sessions.attachmentRead`, using provider/session/message/attachment identity; never accept client paths or include image bytes in polling responses.
- Search is the read-only `sessions.search` action scoped to the selected machine and either the current thread or current Provider. Use cleaned visible content, omit memory metadata, preserve searchable AGENTS.md text, and return stable event/block positions rather than paths. The timeline rail uses the same anchors for messages, plans, attachments and activity groups; history loading preserves scroll height and disclosure choices.
- Shared user-message projection separates literal requests, attachments and optional contextBlocks (instructions, environment, runtime, files). Recognize only complete allowlisted envelopes outside code/quoted examples; support both native attachment request headings and preserve request whitespace. Render attachments, at most one closed “上下文” row, then the request; mount context text only when expanded. Search uses actual content, questions/answers, attachment names and AGENTS.md instructions, never raw wrappers. Context-only messages have no empty bubble or rail marker. Native question replies, shell operations and interruptions retain their dedicated handling. Stable machine/session/message identities preserve disclosure choices; no presentation cleanup writes back to native sessions.

## Release matrix

- CI keeps frontend, backend and macOS Tauri checks. Linux Tauri packaging, AppImage/deb/rpm artifacts and xvfb desktop smoke are retired.
- Release keeps the macOS ARM64 app/updater and Linux webd stages. The eight assets are the DMG/checksum, updater archive/signature, `latest.json` for `darwin-aarch64`, and webd tarball/checksum/signature. Verify Linux archives with the preinstalled Minisign validator and trusted public key before extraction; never fall back to checksum-only updates.
- Cloud deployment remains manual and explicit through `scripts/deploy-cloud.sh`; production configuration is never committed.

## Required gates

Use the commands in README, plus `python3 scripts/privacy-check.py --git-objects`, `git diff --check`, contract checks and `bash scripts/test-install-script.sh`. Use normal incremental commits for subsequent changes.

## 管理边界

- 线程右键、省略号和键盘菜单共用动作模型；右键目标独立于当前详情。保留焦点返回、边缘避让、双击改名和批量模式。
- 单条、批量、归档清理和隐藏清理共用直接删除实现。Codex 覆盖普通／归档明文与压缩 rollout、writer 锁、shell snapshots、visualizations、原生状态和分页历史数据库、名称索引。父子关系去重遍历，循环不阻断；不反向扩大到引用方。执行仅处理确认时提交的 ID 集合，不加入后来出现的后代。
- Claude 定位键取项目相对路径及文件名身份；覆盖主 JSONL、同名目录、file-history、tasks、session-env、debug、todos，以及项目索引、共享历史和会话登记中的对应记录。Grok 按各工作区的 ID 目录定位，摘要缺失或损坏不影响删除。
- 删除不设批量数量、文件大小、目录深度和文件数量上限，目录流式遍历；文件、权限和数据库失败逐项报告，继续其他目标。fingerprint 是可省略的旧字段，不参与删除；归档及隐藏清理的 candidates 是固定执行范围，expectedCount 不再比较。删除是一次性操作，原生程序重新生成的数据在刷新后呈现。
- DSH 提供独立归档清理及本机通知，不新增 Provider 阅读入口。以标准 DSH_HOME 的 workspace v2 归档名单和 session 头的 origin=subagent/parentSession 定位归档根及后代；独立 fork 不扩入。预览可运行中读取，执行前要求对应机器的 DSH 退出。请求固定 id/archiveId，不接受路径；恢复、归属变化或并发修改停止对应清理，共享索引只原子移除成功目标。systemd 仅开放 DSH sessions/storages 子目录，旧服务缺少 dsh_archive_cleanup 能力时禁用入口。
- 隐藏扫描只定位元数据候选及其原生后代，不读取正文或计算指纹。分批接口继续复用 cleanup.hiddenDryRun，每页最多 100 项；保留同源扫描协调、取消及空闲释放，候选总数不设业务上限。停止后的不完整结果不执行清理，机器切换隔离迟到响应。
- Pi 活动契约、读取、管理、界面和监测全部退役，旧命令只能返回 unavailable。先删除自有队列中的 Pi JSON，再移除 Pi 提供方/流/投递/事件及专属设置；保留其原生程序与用户数据。
- API 协议为 2；先升级服务再安装 App。主窗口隐藏创建，仅初始化时适配一次工作区，Ready 和 Reopen 只显示、恢复及聚焦。
- 发布前检查当前树、索引、可达历史和资产。仅发现可达历史敏感值时才按授权重写；平台保留的不可达对象另行记录，不声称已完全擦除。

## 阅读与子智能体边界

- 三个 Provider 共用紧凑阅读基线；复制动作不增加正文行高，用户气泡、工具两层折叠、Plan、搜索与用户指令时间线保持原规则。
- Codex 子智能体只通过原生调用身份、精确任务路径和父子关联读取，不按显示名称猜测。只使用子线程自己的回合证据判断运行状态。
- `threads.subagentDetail` 和带主线程上下文的子线程附件必须校验可见主线程、唯一父链、深度、循环、文件身份与指纹。无任意路径、执行控制或子线程管理能力。
- 子智能体能力缺失时按不支持处理，API 协议保持 2；机器切换关闭面板、隔离缓存并丢弃迟到响应。
- Hook 集成测试必须启动本次 Cargo 构建的 CLI；不得跳过测试或借用已安装的正式服务。浏览器、Tauri 与真实入口验收分别记录。

## 凭据边界

- Keychain 读写统一经过 `CredentialAccess`，不在异步执行线程直接调用阻塞凭据 API。读取超时仍保留在执行工作所持许可，避免并发堆积。
- 保存与移除连接按完整后台事务执行，调用方取消不提前释放连接变更保护；提交或回滚后才允许切换。
- 凭据读取与能力验证后重新验证连接修订号及目标；固定错误消息不含原生异常、Key 或地址。测试不得访问真实钥匙串。

## 子智能体活动与汇总

- 原生 `item_completed` 中的 `SubAgentActivity.kind` 表示历史活动，外层完成不能当作子线程终态。保留开始、完成、中断及交互时点，并与同次创建调用/结果去重。
- 活动、列表和详情共用任务名解析，优先 `task_name`，再使用已验证路径末段；不让昵称或角色覆盖任务名。
- 直属集合在分页前按原生身份去重，当前状态只依据子线程自身回合。父线程未变时也刷新，保留内部完整性标记及各条目的状态与不可用原因；主线程列表不显示子代理数量。
- 汇总和嵌套列表复用只读详情边界、机器隔离及焦点/滚动恢复。API 协议保持 2，不新增 RPC、数据库表或原生写入。
- 完整内部 `external_codex_apps_open_page` JSON 包装从显示、复制、导出和搜索排除；代码与引用示例保留，不产生空用户气泡或时间线标记。

## 界面说明

子智能体汇总和列表不显示通用的统计不完整说明。归档与隐藏线程清理不显示重复描述，保留状态、预览、确认及逐项结果；保留一次确认及固定目标范围，不按读取或活动状态禁用删除。

## 指令与通知边界

- 完整原生 INSTRUCTIONS 包装允许结束标签与后续环境标签同行；内部标题不能截断折叠。代码、引用示例与不完整结构保持保守处理，原生会话不写回。
- Gotify 已退役；只允许迁移代码和不可执行契约 tombstone 引用旧标识。移除其配置、Token、队列、测试任务和投递字段时，必须保留 Bark 凭据、基线、成功/未知记录及待投递状态。
- Bark 使用下述有限重试规则。接口协议保持 2，不改变 Provider 或事件识别规则。
- 外部通知服务卸载归运维仓库，须明确配置与数据归属；不改共用 Nginx/TLS、其他服务或用户原生会话。

## Bark 投递边界

- Antigravity 和 DSH 仅在本机运行通知，通过可选能力展示总开关和四类开关，默认关闭；协议保持 2。沿用 Probe 轮询、数据库和加密队列，首次启用／重新启用建立基线。Antigravity 只读原生服务，明确执行终态和 WAITING + requestedInteraction 才产生对应事件；DSH 只读标准 v4 日志及多帧 Zstandard，按主回合、问题和审批身份配对。子代理和 fork 继承记录不成为主线程事件；读取或身份不明显示异常，不推断终态。首发与重试均复核，恢复执行淘汰旧失败，问题和审批解除后终结重试；异步问题超时仍可回答时继续待回复。

- 每分段最多 3 次，重试等待 15/60 秒并加入 ±20% 抖动；首次投递起 10 分钟内有效。尊重 Retry-After，不提前发送或无限保留。
- 网络失败、结果未知和明确临时拒绝可重试；凭据、证书验证、重定向、非法请求及已知永久 APNs 拒绝不能重试。日志仅含安全分类、状态码、耗时、次数和匿名身份，不含原始异常、地址、Key 或正文。
- 使用加密不可变队列、持久化分段确认与带归属校验的租约。成功分段不重发，Hook 不等待后台重试，monitor 每轮最多处理 10 项。每次重试及来源复核后重新读取配置、核对当前状态和目标；正常追加不等于来源变化。
- 保留旧成功、去重与基线，旧终态未知或失败不重新入队；结束后清除待投递正文。稳定 Bark ID 不能被描述为绝对去重保证。
- 测试通知使用“AgentHub 推送测试”与机器来源正文，作业等待真实终态，HTTP 成功且 Bark code=200 才算受理。旧配置导入仅保留不可执行入口。

## 历史与大小边界

- Codex、Claude、Grok 和子智能体共用自动上滚分页；初次定位最新内容，不自动读完整条历史。搜索与滚动按机器、线程及游标协调，错误或游标不推进时停止；可见锚点与像素偏移优先于旧滚动高度。
- 轮询不能丢失已加载历史和最早游标；机器、线程或文件身份改变时丢弃旧响应。保留独立子面板、工具折叠与用户指令时间线。
- 大小只按已验证的 rollout 或会话专属目录统计逻辑字节，不读正文、不计算全内容哈希、不跟随符号链接、不扫描项目或共享存储。目录后台扫描和缓存必须有界，身份变化立即失效。
- `storageSize` 为可选摘要字段，未知、待统计和部分结果不能冒充完整大小；旧服务仍可读取。API 协议保持 2，不新增命令、表或客户端路径参数。

## 读取性能

- 读取使用有界后台执行和按来源串行的增量缓存；热读不重新解析正文，普通追加只消费新记录。缓存受条目数和内存限制，替换、截断、原地修改和原生身份变化必须失效；半行不能成为已发布记录。
- 历史变更元数据和 Grok 分页是协议 2 的可选扩展。旧客户端保持原响应行为；新客户端只在来源支持时使用稳定块游标。静态旧页不能随轮询重复请求，历史工具、问题与子智能体状态仍需更新。
- K／M／G 只表示单位，颜色按实际字节在 100MiB、300MiB 分界；未知或待统计为灰色，部分结果按已确认下限分色并保留 ≥ 与不完整提示。
- 常规受影响检查在本机运行，完整回归和跨平台构建交由现有三项 CI。性能比较使用运行时合成数据及相同样本，区分冷读、热读、追加、分页传输和远程网络耗时；不将 fixture 结果记为真实入口验收。
- Playwright 回归及 macOS 有头性能检查默认在现有 GitHub Actions 运行；除非用户另行明确要求，本机不安装或启动验收浏览器。CI 先构建前端、安装 Chromium 与 WebKit，只使用 Playwright 管理的浏览器缓存；禁止渠道、本机可执行路径和外部浏览器连接覆盖。全局启动守卫必须保留，不依赖或启动系统 Google Chrome。内置 Google Chrome for Testing 按真实路径确认来源，不以窗口名称判断。

Claude native command envelopes render as the slash command and exact arguments. Classify isMeta, isCompactSummary and complete automatic envelopes before mutating prompt state; automatic records never start a human turn or clear pending tools. Adjacent metadata stays in place in closed system/context groups, including the preamble. Preserve native anchors, disclosure aliases, literal examples and instruction search; compaction remains searchable and closed by default.
