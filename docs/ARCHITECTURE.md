# AgentHub 架构说明（维护者文档）

本文描述当前实现及维护边界。用户操作与下载见 [README](../README.md)，视觉规则见 [DESIGN](../DESIGN.md)，部署步骤见[部署手册](cloud-deploy-runbook.md)，交付状态见[当前进度](progress/MASTER.md)。

## GitHub CI 通知

`github.ci.status` 通过 AgentHubUseCases 及薄 FFI／SSH RPC 适配返回账号与运行摘要；`github_ci` 为可选能力，旧服务不显示面板。配置沿用 `probe.settings.get/save` 的 `notifications.notify_github_ci`，默认 false，监控还受 Probe 主开关控制。Bark 的启停独立控制实际投递。

现有 serve／monitor 启动独立扫描循环，App 与 Hook 的后台启动判断均包含 CI 开关。关闭 CI 时只读取现有缓存，首次读取与手动刷新均不请求扫描，refreshing 为 false；DesktopState 不再持有独立 Monitor。`gh auth token --hostname github.com` 只在来源机器的当前服务用户下执行，凭据仅驻留后端内存，按 60 秒核对账号和凭据，5 秒扫描复用已有 token。ETag 缓存仅保存解析后的账号、仓库和运行摘要，Bot 身份在 200 响应时计算，304 不重复解析完整响应。扫描按到期时间调度，未变快照不新增写入。扫描直接使用 GitHub REST Actions API，复用 HTTP 连接、ETag 及同源校验；账号名下公开／私有／fork 仓库目录每 10 分钟分页刷新，并淘汰已消失仓库的运行缓存。有符合来源筛选条件的未结束运行时每 5 秒、空闲时每 60 秒仅查询该仓库首页 `per_page=10`，不翻运行历史或追查窗口外旧运行／旧重跑。前端先筛选仓库，再按现有更新时间最多展示 20 条，展示上限不限制通知判断。运行日志和产物通过 GitHub 查看。

Linux 服务与独立 Mac monitor 共用配置文件缓存，扫描快照及最终提交都在短 SQLite 事务内读取最新有效配置。设置保存从同一事务内读取文件旧值、原子替换 TOML、写加密设置并更新 CI 基线；网络请求在事务外。扫描期间新增刷新请求由已提交结果满足，设置启停、仓库、Bot 筛选或基线变化时丢弃旧结果并保留刷新请求，同时合并并发确认。限流时间只留在 Monitor 内存，限流期间的刷新及时结束并返回现有错误；状态保留首次读取需要的 last_scan_ms，旧版含 retry_at_ms 的状态可读取但不再写入或公开该字段。

现有 settings 保存基线、有界摘要与待交接事件，未变化时不重写；CI 快照的可缺省 repository_observed_ms 记录各仓库结果变化或首次成功跨过来源基线的时间，失败及其他仓库刷新不推进该证据；设置接口与直接修改配置均处理每次取消 Bot 过滤的截止时间。协议 2、schema 3 无变化。首次启用、重新启用及账号变化的基线轮跳过全部已完成记录，在途任务结束仍通知。Bark 与 Mac 对当前来源基线内已观察到的未完成 → 完成或新 attempt 使用状态变化判定，避免 GitHub 整秒时间戳早于毫秒基线而漏报；无有效前态时仍按截止时间跳过历史。Monitor::tick 直接返回可投递事件，Stored.pending 继续持久化；成功处理的事件批量确认一次，中途失败保留尚未处理项。过期事件过滤、首发有效性和加密队列重试复核保持。账号 ID、仓库 ID、run ID、attempt 构成投递身份；仅 success、failure、timed_out 进入现有加密 Bark 队列。关闭、切号及新 attempt 会停止旧投递，重启保留去重与有限重试。Bark 内部请求增加可选 url，旧队列未含该字段仍可读取；CI 直接使用 GitHub 返回的 html_url。通知历史维护直接使用 SQL 删除结果计数，移除无调用的统计及 dry-run 分支；去重按复合主键原子认领或过期续期，过期标记一次批量删除，保留期、保护集合及范围不变。

桌面通过单个 5 秒串行观察任务读取当前机器缓存，与 Probe 页面可见性无关；`github.ci.status` 的 `cached_only` 默认 false，为 true 时不请求扫描，响应带已有 `baseline_ms` 及来源机器的 `monitored_repositories`、`filter_bots`、`scanned_repository_ids`。新加入的仓库首次结果独立建立基线，成功扫描过的空仓库后续新运行照常通知；不读取本机配置替代远程筛选。观察状态仅保留机器修订、账号、启用基线及运行尝试身份，不落盘；首次观察跳过已完成记录，换机、切号和重新启用重新建立基线。读取沿用现有 SSH 通道，迟到响应不参与通知。macOS 使用 UserNotifications，请求系统授权、支持前台呈现和点击打开运行页；Bark 的持久化队列、确认与重试独立保留；桌面详情缓存空闲回收仍为 60 秒。完成记录不再额外受全局 1000 条限制。

CI 比对以本轮临时索引查找新旧运行及待发事件，排序、attempt 与基线语义保持；只对符合通知条件的新增完成结果解析时间。Mac 只在观察到待发事件时申请系统通知授权，由原生授权回调发送该批事件，观察循环不等待；消费前核对来源，已接受事件按原机器名称投递。系统通知标识使用运行事件身份，不包含连接修订。本机 CI 关闭时重置观察状态且不读取数据库。HTTP 客户端与 updater 共用 reqwest 0.13.4 和 ring。

关闭的 Provider／错误监控仅状态或统计变化时写库，启用状态仍更新扫描时间。Claude Hooks 设置操作不再读取无用配置；通知扫描仍只在处理启用的 Claude 时读取 Hook 配置。Probe 同步聚合进入既有 `read_tasks::spawn_blocking`，复用本轮 Grok／DSH 定位结果，仅读取候选会话的通知状态，不请求目录大小。后台通知扫描范围保持；首发和重试先定位目标，再复核对应来源，Grok 目标事件仅复核一次，DSH／Grok Bot 不读无关正文。活动态势只消费 `active_sessions`，通知汇总包含 Grok Bot。

隐藏扫描统一 start/poll/cancel，取消旧无模式入口；线程行及其中两种父 ID／JSON 关系只读一次，再在同一连接中补充 spawn edges。来源租约保持至终态写完；统一预览使用 NoRequest 无参数契约，兼容旧空请求，内部固定范围以枚举遍历；执行仍校验外部范围、确认、目标和指纹，不同步扫描隐藏线程；分页器负责开始／停止与终态回调，缺失分页进度直接报错。Probe 两端共用保存入口，使用已有 toml_edit 依赖按语义编辑目标表和键，保留未知内容、注释及多行字符串；待写文本直接解析并加载 env、规范化，仅在文本变化时原子替换配置路径并保留原权限，不维护符号链接；空补丁不写 TOML，Key 仍加密写入，env 仍为 0600。DSH 归档／恢复只定位目标，删除继续使用排他租约并核验 inode 和后代；统一归档清理直接由 execute 扫描与校验一次；改名与归档后复用定位结果及最新归档集合生成摘要。

## 运行架构与模块

Codex `list_threads` 统一负责列表筛选、排序和截取，Linux／FFI 适配直接返回共享加载结果；归档条件仍在正文补充前筛选。

macOS 原生 Swift 6 桌面宿主（协议面标识保持 `desktop_embedded_tauri` 以保持兼容）是唯一图形界面宿主；`linux_server_api` 只提供管理 API 与健康检查，不提供网页、登录或静态资源。SwiftUI／AppKit 视图依次调用类型化 Swift Store、`agenthub-ffi` C ABI 与 `AgentHubUseCases`；Provider 读取与受控写操作位于共享核心，SSH RPC／FFI 只做薄适配。

| 模块 | 职责 |
| --- | --- |
| Codex | 原生 state/index/rollout/log 读取、改名、归档恢复、账号隔离解除与直接删除 |
| Claude Code | 递归发现、分支历史、原生进程身份、改名与按文件名定位的直接删除 |
| Grok Build | 原生目录发现、主回合历史、登记与进程校验、改名及定向删除 |
| Antigravity | 本机在线 Connect-RPC 与离线 SQLite／pbtxt／transcript 双模会话列表、分页详情、改名、归档恢复与含子代理后代的清理 |
| DeepSeek Harness | `workspace.json` v2 与明文／多帧 Zstandard 会话读取、`session.lock` 并发保护改名，以及要求退出运行中 DSH 后的归档恢复与永久删除 |
| Probe | 错误与终态识别、游标、基线、去重、脱敏、保留策略和加密 Bark 投递 |
| AgentHub 数据库与配置 | 自有设置、任务、事件、投递及迁移；不包含原生 Provider 数据迁移 |
| `macos/`（Swift 6）+ `agenthub-ffi` | 三栏 SwiftUI 外壳、AppKit 阅读表格与显式 TextKit 1 文本视图、执行分组、搜索、附件、Plan 导出及无死锁 C ABI 桥接 |
| 打包 | macOS ARM64 原生 Swift 6 App/updater（`package-macos-swift-arm64.sh`）与 Linux x86_64 无界面 webd（`package-webd-linux-x86_64.sh`） |

主窗口由 `MainWindowController` 隐藏创建，只在初始化时最大化一次或使用工作区回退，首屏就绪显示；Reopen 只显示、恢复和聚焦，不重新设置几何尺寸。

## 共享契约与修改流程（Shared changes）

API 协议固定为 **2**，`system.capabilities` 返回协议、宿主和能力矩阵。产品版本、协议版本和契约文件的 `schemaVersion` 是不同概念，不互相替代。

1. 同步 `contracts/agenthub-contract.json` 及其 schema，明确 action、DTO 归属和宿主能力。
2. 同步 Rust 核心 DTO/use case 与 Linux（`agenthub-webd`）／FFI（`agenthub-ffi`）适配层。
3. 同步 Swift 6（`macos/Sources/AgentHubCore/`）的模型、领域状态和对应测试，并更新 `contracts/wire-types.json`。
4. 执行契约、Rust、Swift 6、安装及隐私门禁；准确命令见 [README 开发](../README.md#开发)。

共享动作必须在 registry 中有明确映射；桌面专属 `remote.*`、`plans.save`、`claude.hooks`、`claude.ownershipDiagnostics`、`grokbot.list/detail` 不进入 Linux dispatcher。仅保留现行命令正向注册，未知命令统一拒绝；不保留退役表、空 transport/internal 分类或 retiredActions。新增类型或可选字段必须同步真实序列化结构，不能仅增加同名标记。

`contracts/wire-types.json` 是测试用映射及审阅后的源结构清单：Rust 用已有 syn/quote 开发依赖读取 `agenthub-core`、`agenthub-webd` 与 `agenthub-ffi` 的实际类型、serde 属性、枚举及 `dispatch.rs` 派发分支；Node 契约脚本无第三方依赖直接解析 Swift 6 模型声明与 RPC 调用。自由 JSON 与内联请求必须明确归类；代表性真实序列化测试验证缺省、null 和枚举拼写。仅名称标记不能通过实际类型/注册函数检查，负面 fixture 覆盖缺字段、缺映射和缺适配入口。更新清单必须同时审阅线上 wire shape，不能用重新生成清单代替兼容性判断。

## Provider 读取与历史

### 共享读取模型

`jsonl_cache` 核对文件身份、大小、mtime/ctime 和已消费前后锚点。冷读流式消费，普通追加只解析新增完整记录，半行等待写完。各来源独立串行；解析、分页不持有全局缓存锁，未变化结果复用不可变快照。列表、详情、搜索和子智能体复用快照；管理操作独立重新核验来源。

`read_tasks` 使用持有至真实结束的许可，把耗时读取移入有界后台，避免阻塞 Tokio 执行线程。摘要缓存保留服务端发现的身份，使用前重新校验；macOS/Linux 的派生摘要复用 dev/ino、大小、mtime/ctime，其他缓存保留边缘校验。带附件响应按需复制、重新检查附件，后续追加不能改写旧响应。

`history` 包含 epoch、revision 与有界变更记录。来源替换、截断或分支改变使旧窗口失效。Codex 保留 `b:index`，支持附带块 ID 的位置提示及 `a:block-id`；现有 `before` 同时支持 `j:` 定位、`p:` 向前、`n:` 向后、`w:` 刷新窗口。Grok 的 limit/before 可选，省略时保持原完整响应。

### Codex 与子智能体

详情与通知来源复核通过 WHERE id=? 按原生 ID 定位；通知发现仍扫描全部符合条件的头部，不为候选补充正文或大小。完成工具块保持原顺序，结果与调用配对并放在调用位置，历史压缩保留工具分隔。`history_navigation` 在分页和压缩前建立完整用户消息索引：重复文本仍保留独立原生身份，纯自动上下文不进入索引。最新详情可返回 `timeline`，旧窗口不重复传输；定位窗口使用未压缩块。搜索与主线程附件读取也使用未压缩的完整历史，不受初始详情折叠范围影响，沿用现有 JSONL／Zstandard 解析和块身份。

`MessageBlock.subagent` 通过原生调用 ID 配对创建与结果，再按线程 ID、精确任务路径和父子图关联；不按昵称猜测。`threads.subagentDetail` 从可见主线程检查唯一父链、循环、深度、索引、rollout 身份、符号链接及读取前后指纹，不接受客户端路径。子线程历史独立分页，接受主线程已有的 b:index、b:index:block-id、a:block-id 和 j/p/n/w 游标；JSONL 与 JSONL.ZST 共用身份、状态和任务名解析，解压流完整读取且不落盘。附件以 `rootThreadId` 绑定相同关系验证。`subagent_updates` 可更新已加载但不在最新页的卡片。

`item_completed → SubAgentActivity` 的 started/completed/interrupted/interacted 表示历史活动，不能用外层完成推断子线程终态。同次创建、结果及开始事件去重，续接的开始时点保持独立。`eventKind`、`eventId` 保留原始活动身份；任务名按 task_name、已验证路径末段、标题和昵称的顺序取得。

Codex 子活动的显式 ID 直接查关联索引；路径关联仅检查当前父线程的唯一直属候选，仍拒绝矛盾关联。

`ThreadDetail.subagents` 在分页前按直属原生线程 ID 去重，包含状态计数、完整性及逐项不可用原因，不累计孙级或回合数。当前状态只依据子线程自身回合和未完成工具，父文件未变也需刷新；存在运行中或创建中子线程时采用活动轮询频率。UI 可省略通用完整性说明，但不得改变 complete/warning、关联和未知状态语义。

Codex 普通和 Zstandard rollout 的摘要、详情、子智能体状态及创建任务名共用各自现有 JsonlCache 容量池。压缩文件通过普通文件、O_NOFOLLOW 和原生身份核验后完整解压，未变元数据复用快照，变化后完整重建；不生成磁盘副本、不限制解压总长度，超缓存预算仍正常读取但不保留。详情沿用同一分页、时间线与历史变更元数据，附件按请求刷新。子线程复用主线程的明文／压缩／归档路径定位后继续核验父链、目录和文件，不改写原生索引。Codex 8MiB 摘要与 Grok 2MiB 元数据缓存接入原有五分钟空闲淘汰。

Codex Hook Stop 与 completion 正文选择共用逐行读取，不保留整份转录字符串或新增常驻缓存；回合范围、计划／问题优先级、行号与候选计数保持。完整无换行末行继续读取，无效 JSON 行跳过，UTF-8 与 IO 错误明确返回。

### Claude Code

`claude.detail` 的可选 `agentId` 始终配合父 `sessionKey`，FFI 与 Linux 均接收类型化请求并共用 core；独立 `claude_subagents` 能力缺省为 false，旧服务不发送该参数。主详情每次独立枚举同名目录的 subagents，子正文只在打开时读取。Claude Code 2.1.323 脱敏原生 fixture 保留原生 sessionId、agentId、isSidechain 和 parentUuid；后者仅用于记录分支，目录仅证明父会话归属，不建立子代理之间的父链。名称只取父 Agent 工具及结果精确配对的 agentId/description，否则显示文件名 ID。

子读取核验父文件名身份、授权项目根和逐级目录，拒绝符号链接；沿用 O_NOFOLLOW 与读前后源身份检查。子文件不注册主定位键，不参与主列表或管理。主／子解析模式共享原缓存预算、隔离快照；只有已验证子模式接受 sidechain。事件和游标绑定父键、agentId 及源身份，历史 epoch 同时区分父源；保留原文本脱敏、工具配对、分支和分页。子状态为 unknown，不从父运行、mtime、追加或工具结果推断终态。Hook 关闭也可浏览，原子代理 Stop 通知过滤保持。子附件不读取字节。

opaque session key 绑定项目相对路径和文件名身份，同名 ID 互不混淆；子智能体文件不作为独立主会话。详情按稳定事件身份分页，保留分支和原生事件顺序。兼容常见 2.x user/assistant、标题、工具、queue/history/usage、compaction、API 请求辅助、会话设置、续接及已核实 system subtype。兼容 warning 仅展示，未知记录不伪造完成或权限；损坏、半行、身份冲突、路径和权限异常继续阻止改名。

原生会话登记结合会话 ID、PID、进程身份、启动标识和主机进程域判断占用，打开 transcript 与显式恢复参数补充证据；PID 复用或不匹配登记不能证明占用。已明确属于其他会话的进程不锁住同 cwd 的全部历史会话。不使用静默时间窗口。占用及版本判断用于改名；canDelete 保持可用，删除不依赖这些判断。

改名仅追加原生 `custom-title`，执行前核验归属和指纹。删除按文件名定位主 transcript、同名目录和原生旁路，专属目录包括 subagents、工具结果和元数据；共享索引只移除对应记录。删除范围详见下方管理与隐藏扫描。

### Grok Build

活动快照用一轮进程表校验登记数组/对象、原生可执行文件身份与进程启动时间。退出、僵尸或复用 PID 不能证明仍运行。只有活跃归属与未结束主回合同时成立才显示运行；不可读、损坏和身份不确定保持未知。

冷读消费完整历史，不截取尾部；增量缓存保留文件身份、ctime 和前缀锚点。优先读取 updates.jsonl，缺失时使用 chat_history.jsonl。可选 `deletionEligibility` 保留协议兼容并返回可删除；删除按原生目录定位，不采集进程快照或核验指纹。

### Grok Bot

仅 macOS 桌面通过 `GrokBotUseCases` 暴露 `grokbot.list`／`grokbot.detail`；能力字段 `grokbot_sessions`／`grokbot_notifications` 为可选扩展，Linux 不注册读取入口。当前账户槽确定 roster 与 transcript 切片路径，按原生键读取短名 `.blob` 或带键头的 `.kblob`，校验格式及身份。列表只读 roster 与文件元数据，未缓存项仍存在；复制路径重新读取详情，缺失时不替换为共享目录或其他文件。

历史映射用户、助手、工具、附件与交互记录，保留原生 ID 和顺序；详情复用共享历史快照、分页与时间线，搜索和附件复用同一投影。账户与线程共同确定身份，未变化切片复用有界缓存；不导出历史、不增加监听进程或状态库，不提供写操作。

Probe 复用原生轮询、启用基线、消息游标、加密队列与有限重试。Grok Bot 恢复 v2.1.16 的消息级通知：非失败 send-message 文字／附件可产生 completion，明确待回复／待授权继续按原生请求提醒；更新的交互覆盖旧回复，沿用原有筛选。完成候选以消息 ID 去重，不能证明父轮、工具、审批及子任务全部终结。账户隔离、首发与重试来源复核、缓存淘汰及重新启用基线保持；升级保留 v2.1.17 已推进的消息游标，不补发已扫描消息。原生缓存仅覆盖近期内容，不代表完整云端历史。

### Antigravity 与 DeepSeek Harness

Antigravity 当前回合遍历完整相关子代理树，沿用循环去重；每个子代理提取待处理任务、子代理 ID 与时间锚点后释放原始 steps，逆序评估时共享最终状态表，保留较晚发现后代的最新时间，不为优化新增深度或数量限制。

`antigravity.{list,detail,rename,archive,restore}` 与 `dsh.{list,detail,rename,archive,restore}` 经共享 `SessionUseCases`、Linux RPC、FFI 及远端 allowlist 暴露，同时扩展 `sessions.bulkPreview`／`sessions.bulkExecute`（`archive`、`restore`、`delete`）与 `sessions.search`／`sessions.attachmentRead`。五个代码 Provider 列表摘要统一附带可选 `workspaceRoot`（单次只读解析 `<cwd>/.git` worktree 指针文件，零 `git` 子进程，兼容 `ProtectHome=read-only`）供 macOS 跨平台「工作区」总览页与页内工作区分组归并。API 协议保持 **2**，`LinuxServerApi` 完整声明 `github_ci`、`codex_account_scope`、`hidden_cleanup_scan`、`antigravity_sessions`、`antigravity_notifications`（DSH 会话保持 `dsh_sessions: false` 以探测或平滑提示）。

Antigravity 优先通过本机同用户运行中的 language server Connect-RPC（兼容命令行 `--csrf_token` 与 2.34.0+ 的 `~/.gemini/antigravity/daemon/hub_<httpPort>.json` 同 UID／本机回环监听端口核验，Linux 支持 `/proc/<pid>/net/{tcp,tcp6}` + `/proc/<pid>/fd` 无子进程探测）读取桌面会话摘要与轨迹并执行在线改名、归档和删除，拆分运行（`running`）、等待处理（`waiting`）、待回复（`reply_needed`）与错误终态（`failed`）；同时合并 `ANTIGRAVITY_HOME`（默认 `~/.gemini/antigravity`）尚未加载的根会话与 `ANTIGRAVITY_CLI_HOME`（默认 `~/.gemini/antigravity-cli`）的 `agy` CLI 离线根会话（含未入索引的 `.db`／`.pb` 及持锁活跃根会话，按转录排除未入索引子代理与孤立 `brain/<id>`，结合 `presence` 锁、`steps.status` `1/2/8/9/7` 与转录尾部判定实时状态），仅认领主存储归属会话并排除跨端幽灵摘要与 `implicit/*.pb`。用户回合在摄取层统一剥离 `<USER_REQUEST>`／`<ADDITIONAL_METADATA>`／`<USER_SETTINGS_CHANGE>`／`<SYSTEM_MESSAGE>` 与截断占位，在线 Connect-RPC 优先读取 `userInput.userResponse` 原文并共用无硬编码页数截断的分页器，离线 `.db` 按精确 Protobuf 子字段解码（`f19.f2` 用户原文、`f20.f8`/`f20.f1` 回复与 `f20.f3` 思考、`f24.f3.f2` 错误摘要，排除 `f19.f3` 展开工作流与 `f19.f12` 系统提示词章节占位），输出 `contextBlocks = []` 的纯净 `UserMessage` 且跳过纯设置变更空回合；子智能体优先读取 `.system_generated/subagents/<id>.json` 的 `subagentDescriptor.role` 与 `conversation_summaries.agent_name`，提纯 `<original_task>` 并剥离 `<prior_attempt>`／`<instructions>` 后在抽屉顶部可折叠「委派内容」展示。通过可缺省能力 `antigravity_subagents` 在单一归属存储内校验无符号链接、唯一无环父链（`<= 32` 层），于 `HISTORY_CACHE` 外动态富化直属子智能体集合、内联活动卡、`subagentUpdates`、多级详情（`{digest}:` 隔离游标与 `epoch`）及子智能体图片附件。离线写入均先校验目标及后代 `presence/<id>.lock` 未被占用，同步 `.pbtxt`、`conversation_summaries.raw_summary`（`ConversationAnnotations` 字段 `1`/`4`/`5` 保损补丁）、整型 FTS5 `archived`、`cache/conversation_metadata.json` 与四类 proto 缓存文件。永久删除按叶节点优先拓扑逆序清理确认集合（对未水合后代的在线 `404/not_found` 继续清理磁盘与 SQLite 记录）及 CLI 共享文件（`history.jsonl`、`cache/conversation_metadata.json`、`cache/last_conversations.json`），本机与远端 Linux 共用 `antigravity_archived` 批量清理已归档会话与祖先链已移除的无锁孤立子会话。

DeepSeek Harness 从 `DSH_HOME`（默认 `~/.dsh`）的 `storages/workspace.json` v2、`storages/session_projcache/*.json` 与 `sessions/<id>/session.v1..v4.jsonl[.zstd]` 无锁只读列表和分页详情。改名先对 `session.lock` 获取非阻塞排他 `flock`（占用时立即拒绝），按 80 UTF-8 字节规范化标题，并以明文 JSONL 或带内容校验和的独立 Zstandard 帧追加 `session/title` 记录，同时最佳努力同步 `session_projcache`。归档、恢复与含 `origin=subagent` 后代闭包的永久删除均先扫描宿主进程表，若检测到运行中的 `DeepSeek Harness`／`dsh` 进程则拒绝写入，避免被宿主进程内存快照覆盖。 改名追加前校验完整记录与压缩帧：合法末行缺换行时补分隔；半条 JSON 或半帧报错并保持原文件，不自动修复历史。

Antigravity 与 DSH 共用有界 `ReadCache`，快照包含不可变事件及按需建立的时间线，分页只复制请求页，附件单独刷新。DSH 以原生日志身份、大小及 mtime/ctime 失效；Antigravity 离线同时检查完整／简要转录及 SQLite／WAL（读缓存与错误监控的 `-wal` 指纹只比对设备／inode、大小与 mtime 而不取 ctime，避免 root 服务只读打开 WAL 库时 SQLite 对 `-wal` 执行 `fchown` 仅更新 ctime 导致读缓存每轮失效；Codex 子代理父链复核仍保留完整 ctime 比对），在线仅复用原生 owner、摘要和完整终态未变化的结果，运行中不缓存。详情与附件复用同次请求的摘要扫描；本路径未实现追加增量解析，超预算快照仍可读取但不保留。

## 消息、搜索与界面状态

用户消息与助手 Markdown 分开处理，保留用户字面文本和换行。三个 Provider 的原生消息/图片分组形成稳定 user message；共享投影拆出正文、附件及可选 `contextBlocks`，类别固定为 instructions/environment/runtime/files。只在代码和引用之外识别完整白名单包装；允许相邻标签和完整 INSTRUCTIONS 的同行结束标签，内部标题不截断包装。原生问答回复、用户 shell 操作和中断保留专用解析。

新旧 `My request:` / `My request for Codex:` 附件包装均可识别；按原生身份去重并优先已有嵌入图片，失效临时文件不隐藏请求。消息按附件、一个默认收起的“上下文”、正文排列；上下文展开才挂载，纯上下文不产生空气泡或时间线刻度。展开原文按原顺序保留，全部清理仅用于展示，不回写会话。

`visibleMarkdown` 在解析前按源范围去除结构化 memory 与完整 `external_codex_apps_open_page`/page_id 元数据，并有渲染保护；CommonMark 代码、引用和普通示例不被误删。显示、复制、预览、Plan 内容和文件名共用清理结果。指令分段、GFM AST 解析（Rust `pulldown-cmark`）与 `NSAttributedString` 排版按原文复用有界缓存，不保存路径解析结果或折叠状态；文件链接的路径范围与代码块复制在原生视图渲染时取得。

`sessions.search` 是当前机器内的只读动作，输入 Provider、线程/Provider 范围、opaque key、query、cursor 和 limit，不接受文件路径。搜索摘要与有界正文页，返回稳定 block/event position key；达到现有 200 条上限立即停止后续历史读取，保持原顺序和分页规则。当前线程按选中身份直接读取，不扫描最近 200 条列表。用户搜索取实际正文、问题/回答、附件名和指令块，自动字段不重复命中。AGENTS.md 命中仍能展开上下文。Plan、工具、助手回复保留搜索锚点，但不成为用户时间线刻度。

Swift `PagedHistoryStore` 分离最新窗口轮询与历史取页；静态页不重复请求，只刷新变化的已加载身份，快速追加时补齐中间窗口。`HistoryScrollAnchorCoordinator` 统一首次底部定位、向上滚动意图门控、搜索分页、请求互斥和可见消息＋合并别名像素偏移恢复；失败或游标不前进停止自动重试。时间轴窗口直接请求定位游标，复用最新变更记录，无需下载中间全部历史。

搜索命中折叠内容后自动展开并定位，优先精确原生 ID，其次最深别名节点；Codex 压缩只豁免当前机器/线程最近命中的一个块，不展开整段历史。时间线位置由独立 `TimelineActiveTracker` 局部订阅，刻度有界挂载且键盘可达全部记录。工作区按需加载，隐藏窗口暂停界面轮询，后台 monitor 独立运行。

普通与归档会话选择通过同一选择入口互斥，点击、快捷键、原生菜单和浮动栏均清除另一范围。归档按钮展开时全选已加载归档项，收起时消费全选而不选中普通列表；输入框与弹窗继续独立处理。

显式折叠选择按机器、Provider、会话、活动身份保存在有界内存中，不写数据库。机器、线程或来源变化取消无效请求并丢弃迟到响应；子智能体面板有独立分页、滚动与嵌套返回，父视图保持挂载。连续工具组保持原位置，完成默认收起，活动/失败默认展开，指令组始终默认收起；用户选择优先于默认值。

### 附件、路径与 Plan

`sessions.attachmentRead` 经共享 use case、身份校验和薄适配读取；只接受 Provider、session/message/attachment ID，以及子线程必要的根线程上下文。重新解析来源及文件身份，PNG/JPEG/WebP/GIF 单张预览限 20MiB。视口附近才读取图片，前端缓存按机器隔离并在切换时清除；轮询只携描述，不含图片，不抓取仅有远程引用的附件。

文件链接在解析 URL 前从原始 Markdown target 提取，剥离行列后缀并按线程 cwd 解析相对路径。远程只复制，本机 macOS 通过 `NSWorkspace.shared.activateFileViewerSelecting` 在 Finder 定位。`plans.save` 始终是本地保存导出，校验有界 Markdown 文件名/正文并独占创建，不覆盖既有下载、不经过远程 RPC。

## 管理与隐藏扫描

五种 Provider 共用 useSessionSelection。⌘ 切换单项、Shift 按可见顺序追加区间，切换机器／Provider／筛选或退出多选清空选择。轮询不选择新行。删除没有批量数量上限；归档、恢复保留原有规则，客户端按 100 项分批提交。普通点击、⌘／Shift 选择及全选清除旧预览／执行错误；批量操作忙碌时，各 Provider 的详情写入与普通／归档选择共享 busy 状态，关闭已打开的改名框并阻止提交。分批执行发生请求错误时保留前序已确认结果、清除对应选择并刷新列表，后续批次停止；结果不明的请求不自动重发。

直接删除预览展示目标，一次确认后仅执行提交的根 ID 和 descendants，重新计算的父子关系只用于归属校验，不扩大集合。Codex、Claude 与 Grok 的直接删除不依据正文、活动进程、writer 锁、终态或旧 fingerprint 阻止执行；Antigravity 与 DSH 保留所选目标的运行、占用和原生写入校验。统一清理另复核预览指纹及最新允许集，API 协议保持 2。目录流式遍历，没有文件大小、深度或文件数量业务上限；符号链接只删除链接本身，共享文件只移除目标记录，原子替换保留 uid／gid／mode，并与读取前的文件身份比较；发现并发变化时明确报错，由用户重试。

Codex 依据原生父子关系生成后代集合，循环去重；includeDescendants 和 SessionBatchSelection.descendants 明确传递确认范围。执行不再发现新后代，也不扩大到引用目标历史的其他线程。直接清理普通／归档 rollout、压缩副本、writer 锁、shell snapshots、visualizations、原生数据库、分页历史和名称索引，不调用 helper。归档、恢复仍使用原有指纹和状态规则。归档／恢复预览在本批次复用数据库连接、字段信息、目录清单、名称索引及反向文件引用；执行不复用预览快照，每批在写事务之外重新读取一次清单，逐项在独立短事务内复核线程行、文件身份、内容与指纹后更新，单项失败不回滚其他目标。记录中的 rollout 路径按明文／压缩候选解析，已被原生压缩的线程沿用相同的身份、运行信号、终态与引用校验。Antigravity 批量预览共用一次实时摘要和父子索引，DSH 删除预览共用一次进程检查、归档索引和完整会话发现；Grok 删除预览只枚举一次工作区，批内逐项定位并统计大小，执行仍重新定位全部同 ID 目录。原有执行检查和逐项结果保持。

Claude 的稳定定位键由项目相对路径和文件名身份生成，与正文解析独立。主列表、缓存／回退定位及删除索引共用目录过滤：同名主 JSONL（含正常或失效符号链接）拥有其附属目录，内部 JSONL 不作为独立主会话；子代理只读入口保持独立。精确匹配 `jobs/*/state.json` 的原生 ID、`sessionId` 或 `resumeSessionId` 后，先使用来源机器已有的 `claude rm` 清理后台记录及工作树；原生命令未支持或未成功时保留历史，不强制移除或停止原生进程。没有匹配后台记录时不需要 CLI。删除主 JSONL、同名专属目录、按 ID 定位的 file-history、tasks、session-env、debug、todos，以及项目索引、共享历史和会话登记中的对应记录。Grok 直接删除各工作区下的目标 ID 目录，并移除 active_sessions.json 对应登记；摘要缺失或损坏不阻止定位。

单条、批量和统一清理共用删除实现。统一清理先核验当前允许集、指纹和活动状态，再把固定目标交给 Codex／Claude／Grok 已有批量入口，合并共享索引写回。仅保留 cleanup.unifiedDryRun／cleanup.unifiedExecute 与渐进 cleanup.hiddenDryRun，旧独立 archive／hiddenExecute／dshArchive 入口及适配器已删除。缺失文件视为完成；权限、文件系统和数据库失败返回逐项 failed/residuals，继续处理其他独立目标及平台，不回滚整批；DSH 共享索引或原生运行状态变化仍停止相关写入。新执行不会因单项失败停止整批；移除恒为 false 的 stoppedEarly 响应字段。数据库修改按目标事务执行。原生程序之后新写入的数据在刷新后显示，不启动重复删除任务。

Claude 列表右键确认与批量删除共用原生清理入口。同批首次匹配后台登记才执行 rm 帮助检查，成功后复用至批次结束；下一批重新核验。帮助输出非阻塞读取、上限 64 KiB，等待最多 5 秒；单次 rm 最多 30 秒。超时或读取失败时 kill 并 wait 本次 helper，保留历史，不向原生会话或进程组发送信号，不自动重放。

Codex 账号隔离：Codex 0.168 起在新会话 rollout 首行写入 `account_scoped`，其历史与 resume 列表只按该行过滤其他账号创建的会话，不读取数据库。`threads.accountScopeStatus` 只统计带标记的线程和文件；`threads.accountScopeClear` 把首行中 Codex 写出的精确片段替换为等长空白，替换后的 JSON 必须等于原值去掉该字段，文件名、`session_meta` 与 `payload.id` 不一致或写法不同的文件记为格式不支持。明文 rollout 以 O_NOFOLLOW 读写打开、同一描述符复核后原地写入并恢复 mtime，Codex 只追加写入，inode、长度与历史偏移不变。压缩 rollout 先解压得到长度和预期摘要，在同目录以 zstd 3 级重编码临时文件，复制属主、权限与 mtime，同步并核对解压摘要后，在 Codex 协调锁内确认线程写入锁空闲、来源身份未变且没有同名明文文件，再原子替换；线程锁只在协调锁内探测，与 Codex 的取锁顺序一致。失败时删除临时副本；副本存续期间持有文件锁，进程被终止遗留的副本由下一次解除清理。Codex 的维护锁、协调锁和线程锁只以只读方式打开已存在的文件，从不创建；维护进行中整次返回错误，缺少协调锁或线程被占用记为忙碌。同一线程先处理原始文件，回退副本仅在原始文件可读、`payload.id` 属于该线程且已无标记时处理。配置的 Codex 数据目录即根目录，两个入口都不进入符号链接的 `sessions/`、`archived_sessions/`，也不跟随其下的符号链接。扫描完成后计时，单次调用约 20 秒后返回 `remainingFiles`，界面在仍有进展时继续调用。响应只含数量、线程 ID 与原因，审计只记数量；不读取 Codex 凭据。可选的 `probe.hooks.clear_account_scope` 在 Codex Stop Hook 内、建立通知事件之前，通过原生数据库的父子关系定位本回合线程及后代，只处理明文原始文件。

cleanup.hiddenDryRun 保留 start/poll/cancel、扫描 ID 与游标和旧完整响应。只扫描元数据及原生父子关系，不核验正文或指纹；每页最多 100 项，候选总数没有业务上限。同源最多一个活动扫描，进程最多两个上下文；五分钟无请求释放。界面加载完整确认范围后允许执行，停止结果不能执行，切换机器隔离迟到响应。

DSH 归档清理纳入统一 CleanupUseCases、Linux RPC 与 FFI 入口，能力字段 dsh_archive_cleanup 可缺省为不支持，API 仍为 2。独立会话入口另由 dsh_sessions 能力声明。DSH_HOME 默认为用户 .dsh，以 storages/workspace.json v2 归档名单为准，读取明文／Zstandard session 头，只有 origin=subagent 且 parentSession 可核验的后代随归档根入选，独立 fork 保留。预览可在 DSH 运行时读取，执行必须先退出 DSH；固定 id/archiveId 经重新核验后删除专属目录、锁和投影缓存，仅移除成功身份的归档、置顶及工作区引用。后续新增归档不自动纳入，恢复或归属变化跳过，文件／索引并发变化停止相关写入，失败项目保留原因供重试。共享附件、项目和配置不清理，损坏或自定义持久化格式明确拒绝，缺少存储为空状态；systemd 仅对 sessions 和 storages 子目录授权写入，覆盖默认及自定义根。

Claude 空文件、命名记录和仅元数据会话显示“暂无对话”；缺少 cwd 或版本不产生读取失败。Grok 优先读取 updates.jsonl，缺失时读取 chat_history.jsonl，映射用户、助手和工具调用／结果，复用分页、增量缓存、搜索和时间线。系统提示、system_reminder 和加密推理不作为普通对话展示。

## 远程连接与认证

Swift 界面只向 `agenthub-ffi` 传命令、参数及连接 revision。桌面读取用户 OpenSSH 配置及 Include 中的明确 Host 别名，实际连接交给系统 ssh，复用密钥、ssh-agent、钥匙串和跳板设置。桌面仅保存 Host、显示名称与 useSudo；需要交互认证时先在终端完成。原生层沿用命令 allowlist、协议 2 和能力验证。

当前机器保持一条自有 SSH 通道，固定执行 `/usr/local/bin/agenthub-webd rpc-stdio`，useSudo 时加 `sudo -n`。代理只转发至 `/run/agenthub-webd/rpc.sock`，不打开数据库或启动扫描器。常驻服务继续承担通知、缓存和后台任务；systemd RuntimeDirectory 为 0700，套接字为 0600。请求使用带 ID 的 JSON 帧与四字节长度前缀，并发响应按 ID 配对，沿用 32MiB 响应限制。

切回本机、切换机器和退出均释放自有通道；断线显示重连入口，写操作不自动重放。连接 revision 隔离 QueryClient、工作区、预览、选择、分页、滚动、标题、附件和折叠，迟到响应丢弃。仅尚未返回的写请求阻止切换；已受理作业继续在远端运行，返回该机器后从执行记录查看。jobs.list 只返回摘要，展开时通过 jobs.detail 读取完整日志，逐完整行解码和脱敏并保留尾行。

「安装并连接」经 SSH 发送 App 内置固定引导脚本，下载同版本正式服务包，复用可信公钥、签名验证及现有安装器。Ubuntu/Debian x86_64 自动补齐必要依赖；其他 Linux x86_64 需要已有 systemd 和依赖。失败按认证、依赖、下载、签名、安装或健康检查阶段报错，可明确重试；不修改 sudoers。

TCP/HTTP 管理监听、API Key 鉴权和限流已删除；旧 HTTP/Nginx、目录、配置和桌面钥匙串迁移由 v2.1.0 完成，本版不再执行。本机 Codex 桌面连接对话、发送队列与 CredentialAccess 已退役（存量钥匙串条目保留不触碰），业务审计独立保留。systemd 继续保留隔离及 Provider 数据根的精确 ReadWritePaths，Grok／Claude 配置与凭据使用可选 ReadOnlyPaths。

更新任务与 Probe helper 使用内部 `ProcessCommand` 的 program/args 直接启动，不执行配置 shell 字符串。现行默认更新命令映射到固定包装器及参数；自定义 shell、环境赋值、管道、重定向和命令替换返回固定错误。任务名称、状态、输出处理、互斥与审计字段保留；备份清理直接运行固定 Python 参数，保留最近三份。序列化 command 字段仅保留兼容展示，不改变 wire shape；Hook/配置文本的 shell_quote 统一实现并保留既有引用策略。

按需恢复复用原刷新或请求，不增加轮询任务或连接修订号。同轮请求共享一个握手结果；读取最多重试一次，业务错误不触发恢复，写操作不重放，传输写失败与超时关闭旧通道。切换或退出后迟到握手不能恢复旧目标。启动时已保存的远程目标立即返回连接视图，首次握手在后台进行并由首个远程请求共用；远程不可达不阻塞界面显示。Probe 设置读取与保存、Bark 测试及 Hook 作业、事件与作业读取、Codex 归档／恢复和更新状态等桌面命令在既有有界阻塞任务池执行，不占用主线程。

桌面专属 remote.sshDelete 接收 revision 与 alias，返回更新后的 RemoteConnectionView，不接收前端文件路径。用户配置与 Include 按显式别名编辑，保留注释、权限、其他别名与密钥；系统配置不能删除。原子替换失败报告并恢复已替换文件；删除对应 AgentHub 记录，当前连接被删除时回到本机。

Linux 派发直接返回 Result<Value, ApiError>，没有 HTTP 请求／响应转换或测试 Router；Axum 只作为模拟服务的开发依赖。仅常驻 `serve` 与 `probe monitor-errors` 使用双工作线程 Tokio，`rpc-stdio`、`health` 及各短时 CLI／Hook 子命令统一使用单线程 Tokio。作业日志按 CR、LF、CRLF 分记录，整条脱敏后在现有读取任务中累计 16KiB 或 200ms 写入，结束前排空。jobs.detail 的可选 includeOutput 默认为 true，所有后台轮询（包括终态）传 false，展开时读取全文，终态仍刷新相关状态。作业注册表仅持有互斥组；启动、等待或读取失败统一收尾，日志排空与终态处理后释放，无现成运行时时使用单线程 runtime。前端不改写文件名或地址，后端保留凭据脱敏。

## 通知与监测

设置读取、Probe 新鲜状态及被动扫描复用 config/env 元数据指纹缓存，原子替换或内容变化后刷新内存，读取失败明确报错。Linux 服务及 CLI 使用实际启动配置路径，设置保存、Hook 作业、状态检查与生成的 Hook 命令保持该路径。probe.status 的可选 refresh 参数用于手动刷新、保存完成及 Hook 作业终结，当前机器的查询缓存接收新鲜结果，普通轮询保持原快照和频率；桌面 FFI 状态始终读取新鲜配置。Hook 写回沿用原子替换并保留权限，原有等待每两秒比较文件指纹，扫描与等待周期使用相同配置。

Probe 关闭时普通 Bark 待发项结束并清除加密正文，重新启用推进已有通道基线；显式测试只受 Bark 通知开关控制。错误监控的有效关闭／开启重置已有游标，首次扫描建立末尾基线，重新开启到首次扫描之间的错误也不补发；其待发项服从错误监控开关。Codex 普通反馈与异步问题基线随有效开关在保存事务内同步，监控／Hook 同步在同一事务内读取配置，来源复核只读基线，不用旧配置改写。

Codex 通知先按当前配置与旧去重记录判断通道关闭、事件开关关闭和重复事件，命中即返回原跳过结果；确定入队后才对 rollout 计算内容摘要并写入候选，重试前的来源复核保持。错误监控每轮读取上限为扫描函数已有的 1,000 行。安装／移除 Codex Hook 用 toml_edit 就地编辑 `config.toml`：只改 `features.hooks` 的值并移除 `hooks.json` 对应的过期状态表，其余注释、键顺序与文本逐字节保留；缺少 `[features]` 时追加到文件末尾，响应只在实际复制了备份时返回备份路径。

Antigravity 通知在本机 macOS（`~/.gemini/config/hooks.json` 受管 Stop Hook + 15 秒轮询）与远端 Linux `agenthub-webd`（15 秒只读轮询离线 `antigravity-cli`／桌面存储，`LinuxServerApi` 开放 `antigravity_notifications`，Hook 写入仍仅限本机）同步运行；DSH 的通知仍仅在本机运行（`dsh_notifications` 仅由 macOS 桌面声明），协议仍为 2。每种来源提供总开关及完成、失败、需要回复、需要授权四类开关，默认关闭，沿用现有 Probe 间隔、数据库、持久化认领和加密队列；存量开启 Antigravity 通知的实例首次扫描 CLI 存储时持久化一次性 `antigravity_cli_resume_cutoff_ms`，跳过升级前历史会话。

Antigravity 通知优先从已运行且同用户的原生 language server 读取摘要、轨迹及执行段元数据（限回环并禁止代理和重定向），桌面未启动或会话位于 `~/.gemini/antigravity-cli` 时回退离线 SQLite／转录快照，统一产出完成、`ask_question`／`RequestFeedback`／正文追问需要回复、权限等待及重试结束后的明确错误终态；子代理 `Stop` Hook 识别后静默忽略且 `scan_one` 对串联 Hook 结算延迟做有界重试。服务未运行不阻塞离线 CLI 扫描，身份不明或读取损坏在 Probe 来源状态显示异常。

DSH 复用标准存储边界，读取 v4 JSONL 或逐帧解压追加的 Zstandard，未写完整的尾部等待下次扫描，完整记录按序列核验。turn/start 与 turn/end 决定主回合终态；approval/asked 与 approval/decided 配对授权；ask_user_question 调用、结果及 user-question-reply 身份配对问题。超时返回 pending 的异步问题跨正常回合结束仍保持待回复，实际回答、取消或明确失效解除。子代理、归档和 fork 继承范围不产生主线程通知；不支持的版本、持久化格式及未知必要记录显示读取异常。

设置保存与原生平台／Bark 启用基线在同一事务完成，首次 Hook 按保存时间排除历史。重新启用重建该平台流游标，保留已投递身份；单会话 Hook 只更新所见流，不标记全量扫描完成或覆盖全量统计。两者首次启用及重新启用建立基线，不补发历史；按来源、会话和回合／请求身份认领，同一请求不因摘要变化重复发送。首次发送及每次重试前读取原生状态，恢复执行淘汰旧失败，已处理问题／审批清除重试正文；正常完成沿用既有有效期补发。无法复核时只使用既有有限重试，不猜测终态。

Hook 按 Provider 筛选待发队列，再限量、解密和领取；后台全量轮询不加来源筛选。失败数与待发数由单次分组查询汇总，状态口径、平台过滤及 Provider 顺序保持。扫描入库后仅保留 Grok 当轮全文快照；Claude、Grok Bot、Antigravity、DSH 继续按原路径复核来源。

通知缓存快照的事件数组由不可变 Arc 共享，缓存命中及流复制不再深拷贝正文；序列化数组形状和投递语义保持。

通知首发、重试、Hook、CI 与 monitor 共用配置读取回调；服务端和文件入口复用按 TOML／env 文件身份、修改时间及长度失效的既有缓存。原生扫描入库及 Bark 启用同步在设置保存所用事务内重读配置，避免旧扫描覆盖新启停基线。等待轮询时每两秒检查设置变化，变化后立即进入下轮；未变化时保持原扫描间隔，不增加监控任务。来源检查结束及每段发送前读取当前开关和目标，排队事件保留原目标指纹；身份解析不会将旧事件改投新目标。配置读取失败沿用错误处理。

原生通知待发正文用既有 SecretBox 加密；交接到 Bark 重试队列后清除原生正文副本，终态只留身份、结果和界面短摘要。保留期清理包括 unknown，保护待发、重试和当前游标仍需要的去重身份，Antigravity 游标推进后再淘汰旧身份。会话身份或游标未变时不更新该行；原生待发队列使用 pending／delivering 局部索引；通知结果按实际变化的事件键更新关联 probe_events、原生投递和错误事件，借助 JSON 事件键及 event_id 索引避免历史全表扫描。没有状态变化时不执行结果回写，必要扫描状态照常持久化。原有三项通知关联索引与后续新增的四项增量索引（审计／作业时间、原生投递更新时间及 Claude 会话局部索引，合计 7 项）均在现有迁移锁内幂等补建，schema 保持 3。

Mac 普通启动和保存设置使用非强制 kickstart；仅 helper 内容变化且服务已加载时执行一次 kickstart -k，plist 变化仍沿用 bootout／bootstrap。

桌面 App 与 monitor 共用 `Config::load_runtime`。新环境在既有受限 env 文件中生成独立随机密钥；有效密钥继续使用，缺失私有密钥、损坏配置及解密异常明确报错并保留数据。旧开发密钥迁移已移除；不提供桌面加载失败的密钥回退。

事件识别与 Bark HTTP 投递分离。Codex 错误绑定规范主任务身份；普通自然语言反馈仅在确认终态后本地识别，monitor、Stop Hook 和完成入口共享 `assistant_question` 与持久化认领。Grok 使用主回合结束证据；Claude 轮询要求明确终态且无 pending tools，空完成内容回退到本回合最后一段助手回复。兼容警告继续可读，仅致命解析/身份错误禁止扫描。

原生 Codex 问题解析位于 `codex/user_input.rs`，统一同步 question、异步 title 和字符串/对象选项。tracker 以原始 turn/call/question 身份配对完整回复，处理明确取消与替换；工具确认、背景进度或正常完成不能关闭尚未回答的问题。异步 monitor 独立于列表状态，与 Hook、passive、Stop 共用发送前复核。启用基线和原子 per-call 认领保证首次跳过旧历史、部分回答不产生新身份，重启/TTL 过期不重发已确认调用。

Claude 轮询复用 provider monitor 游标；已核验 owner 为 busy 的当前完成事件保留在原生加密待发队列，就绪后才领取 Bark 发送任务，不消耗发送次数或阻塞有效 Stop hook；已明确撤销且尚未被原生发送者或 Bark 领取的完成候选直接结束并清空密文，不创建可见完成事件。读取失败继续使用既有有限重试。仅明确完成／失败、未解决 AskUserQuestion 或绑定 pending 主工具的权限请求符合条件，不从普通 pending tool 猜权限。解析器保留有效分支上先前回合的终态，后续新用户消息不清空旧完成，增量游标可补发未处理事件；一轮共享扫描或按目标读取完成投递复核，避免每项全库重扫。

claude.hooks 是仅限本机的 status／install／remove 操作。安装须设置页明确确认，App 安装不启用；配置位置沿用 Claude Provider，按固定程序和专属子命令识别受管条目，结构化合并并原子替换，保留用户／插件 hooks。安装、修复、移除幂等，格式错误或并发修改停止。Linux RPC 和 remote.invoke 不开放 hooks 配置写入。

受管 hook 使用 exec 程序与参数、async=true，handler 自行限制总时间五秒，不依赖异步 timeout 字段，不输出审批、模型上下文或唤醒指令。Stop 使用 last_assistant_message；Notification 的 permission_prompt 为授权，elicitation_dialog／elicitation_url_dialog／agent_needs_input 为回复；StopFailure 仅使用过滤后的错误类别。idle_prompt 不独立提醒，仅在尚未通知完成、未被启用基线覆盖且可核实终态时作为完成兜底。

hook 先进入现有加密队列，再至多进行一次有界发送，重试由 monitor 处理。prompt_id 与 transcript promptId 必须经原生映射验证，消息 uuid 不能代替 prompt ID；不明确时保留队列等待核验。去重绑定机器、Provider、会话、文件来源身份、prompt 和类别；仅 Claude hooks 及其同机轮询每回合每类认领一次，兼容旧投递键与成功记录。已确认回复／授权不因新回合撤销；完成还须具备对应回合终态，保留有效分支上较早回合的完成补发。StopFailure 为 API 错误结束的候选；Hook 首次处理、首次发送与每次重试均核验当前主回合、执行活动及终态，轮询失败使用相同规则。新回合、同回合恢复、成功或取消使旧失败过期并清空待投递正文；普通元数据追加和仍打开的进程不表示恢复执行，独立后台任务不阻挡主回合提醒。身份或终态未写完整时沿用有限重试。远端继续轮询；Codex 的逐问题身份、Stop、PreToolUse 与问题跟踪不变。

所有通知来源共用摘要格式：标题 Provider｜状态｜线程标题，状态仅完成／需要回复／需要授权／失败，缺失标题回退项目名再固定名称。正文先完整脱敏，再去 Markdown、代码块、工具输出及内部元数据，最多四个非空行、300 个 Unicode 字符；一旦确定截断便停止整理后续正文，保持空白和省略号规则。仅完整有效的图片语法被删除，普通感叹号、数组和未完成图片文字保留，链接文字复用内联标记与控制字符清理。不附线程／回合 ID。每个新事件一条摘要；group 为既有前缀加 Provider，回复／授权 level=timeSensitive，其余 active，保留声音与合法显式 URL，不构造 AgentHub 深链。

Bark 保存和发送前共用 URL 校验。HTTPS 拒绝回环、未指定、链路本地、IPv4 私网、IPv6 ULA 及对应映射地址；拒绝数字 IP 别名、非法 authority 和 URL 凭据。保留普通 HTTPS 主机名与真实 HTTP 回环服务，不作 DNS 解析。无效配置保留原值并停止投递，不改用默认服务携带原密钥发送。

`bark` 复用支持 HTTP/2 的连接池并禁重定向，分类 DNS、连接、TLS、超时、读取、HTTP 及业务拒绝；已知永久 APNs 错误优先。原始异常仅用于内部分类，对外保留安全分类、状态码、耗时、次数及匿名关联，不输出 Key 或正文。

`notification_delivery` 将不可变请求加密入队，事务化认领／确认／事件回写；新事件保存单条摘要，旧在途请求仍按原内容和分段确认继续，不重新格式化。租约持有者才能确认，过期租约只恢复未确认内容。每条／旧分段最多 3 次，等待 15/60 秒并加 ±20% 抖动，首次投递后最长 10 分钟；尊重 Retry-After，超出剩余期限即终止。成功项不重发，稳定通知 ID 由机器实例、事件与分段组成；保留默认关闭、Provider 开关和启用基线，不清空数据库或凭据。

Hook 短等待，后台每轮最多领取 10 项；重试在来源验证前后检查当前配置、身份、开关、目标与事件有效性。正常追加以原始前缀和文件身份复核，不因长度变化取消。结束清除队列正文，同一 UI 事件更新状态。旧终态失败/未知不回放，新网络未知可有限重试且可能重复提醒。主动测试使用独立身份和同一队列，最终未受理以失败退出；受理不代表手机实收。

## 大小统计与资源预算

`session_storage` 只统计已验证归属的逻辑字节，不读正文或计算完整内容哈希。Codex 统计 rollout，Claude 统计主 JSONL 及同项目内确认身份的同名会话目录，Grok 统计专属目录；独立线程、共享数据、工作目录和外部附件不计入，不跟随符号链接。

Antigravity／DSH 列表在筛选、排序与截取后才请求大小；详情与最终删除确认继续按需统计。DSH 附件读取、Antigravity 删除预览摘要及清理允许集、Claude／Antigravity／DSH 失败预览摘要不再统计大小。五个平台摘要的可选 `storageSize` 使用 bytes/scope/status，未知不等于零。普通追加可复用身份缓存；截断、替换和归属改变立即失效。后台扫描收尾按同一规则复核，扫描期间的追加保留本次结果，替换或截断不发布。K/M/G 按 1024 换算，颜色按准确字节：小于 100MiB 绿色，100MiB 至不足 300MiB 琥珀色，至少 300MiB 红色。未知灰色，部分结果按确认下限分色并保留 ≥，提示准确字节和不完整状态。

固定预算由 Rust `resource_limits` 与 Swift 缓存常量命名管理，不是设置项。每个 JSONL/普通读取缓存默认最多 64 个来源，Codex 活动扫描与 Grok 摘要为 2,000 个来源；以下字节数独立计重，条目与字节上限先到者生效：

| 缓存或工作 | 当前预算 |
| --- | --- |
| Codex 正文 / 活动扫描 / session index | 128 / 8 / 16MiB；活动扫描（`SCAN_CACHE`）按结构与嵌套字符串／问答堆容量真实计重，至多 2,000 个线程驻留，超出条目或字节预算按 LRU 淘汰 |
| Codex 问题 / 子线程生命周期 / 关联 | 4 / 4 / 1MiB |
| Claude 正文 / 摘要 | 160MiB；摘要 2,000 条、8MiB |
| Claude 输入文件 / 单条记录 | 512 / 32MiB |
| Grok 正文 / 活动 | 64 / 2MiB |
| 原生通知派生读取 | 各 32MiB；DSH 不限条目数，沿用空闲释放 |
| Antigravity／DSH 历史与时间线 | 合计 32MiB、64 项 |
| DSH 折叠摘要 | 8MiB、2,000 项，按旧项淘汰 |
| 历史变更记录 | 64 个 revision，合计 4,096 个变化身份；超限要求安全刷新 |
| 大小缓存 / 队列 / worker | 512 项 / 128 项 / 2 个；TTL 30 秒 |
| 单次目录统计 | 10,000 项、16 层、250ms |
| 阻塞读取 | 按可用并行度限制为 2–4 个 |
| Swift 阅读器已挂载行视图缓存 | 最多 128 个已渲染单元格，配合增量行高表与 Lucide 图标光栅缓存 |

Antigravity／DSH 时间线随历史快照生成，按实际分配容量计入现有缓存预算；revision 在快照生成时按实际内容计算，不新增增量变更日志，保留保守旧页重读。Antigravity 在线缓存修订只依据会话 ID 与摘要，终态判断复用来源层结果，身份、解析或提前空页错误明确返回且不进入缓存。分页以首批总数为目标，末页允许因原生追加超出目标，不设 50,000 步硬上限；仅服务不可达或无在线内容时回退离线。

Claude 摘要与 Codex 活动扫描、`session_index`、问答、子线程状态及创建任务名缓存均接入既有 5 分钟空闲释放入口，不新增定时器；实际发生缓存淘汰时在 Linux glibc 下触发 `malloc_trim(0)` 归还空闲堆页，空轮询不触发；释放缓存引用后再次读取按原数据重建。

DSH／Grok 通知派生条目只在内容变化时计重并维护总量，Grok JSONL 缓存使用共享不可变数组。扩大 DSH 缓存覆盖可能增加驻留内存。上述缓存各自计重，**不是进程内存硬上界**：正在引用的 Arc 快照可能暂时不可淘汰，解析、响应序列化、已渲染的 `NSAttributedString`、AppKit 视图与并发请求有额外内存；App、helper 和远程服务又是独立进程。目录条目按数目而非完整堆内存计重，隐藏扫描预算见管理章节。超大快照可返回但不长期保留；不能以改变成功读取行为的方式把配置预算解释成全局 RSS 限额。

## 兼容与自有数据迁移

最低兼容边界继续由协议 2、宿主及能力响应判断，不仅看版本字符串。下表的版本表示字段/能力首次随 Release 提供，不宣称旧服务拥有后来新增的能力。

| 能力引入 | 缺失时的现有行为 | 未来删除回退的前提 |
| --- | --- | --- |
| 1.2.11 直属集合 | 保留既有阅读和摘要状态回退，缺能力按不支持处理 | 最低服务具备对应集合和能力 |
| 1.2.16 storageSize | 显示未知，不能当作零 | 最低服务保证大小 DTO |
| 1.2.17 history/稳定 cursor/Grok 分页 | 旧 cursor、全量快照或逐页刷新 | 最低服务保证变更元数据及分页 |
| 1.2.20 hidden_cleanup_scan/deletionEligibility | 单次扫描无虚假进度/取消；Grok 使用原活动资格判断 | 最低服务保证两项能力 |
| 1.2.21 contextBlocks/timeline | 保守解析原包装；历史顺序加载和定位 | 最低服务保证结构化内容及索引 |
| 1.3.5 codex_delete_tree／includeDescendants | 保留旧能力字段兼容；删除以确认时提交的目标和后代集合为边界 | 最低服务保证整树预览、执行和复检 |
| 1.3.6 压缩 rollout／Claude 过程显示 | 旧客户端忽略可选阶段字段；压缩目标继续按旧能力边界拒绝离线回退 | 最低服务保证压缩只读核验、引用复检和可选 Claude 阶段字段 |
| 2.1.27 codex_account_scope | 不显示「Codex 账号隔离」入口；`probe.hooks.clear_account_scope` 缺省为关闭 | 最低服务保证状态查询与解除命令 |

v2.1.1 支持新安装及已由 v2.1.0 完成迁移的配置。更早版本先升级至 v2.1.0；早于 schema 3 的数据库须先经过 v2.0.10。协议保持 2、数据库 schema 保持 3。旧 `thread_followups` 与退役 `codex_desktop_submissions` 表在存量升级库中保持不删除，可直接完整回退至 v2.1.30；`v2.1.38+` 全新创建的数据库仍标记 schema 3 但不再创建 `codex_desktop_submissions` 表，将该全新库回退至 v2.1.30 时会话阅读、管理与通知正常可用，旧版本机 Codex 发送队列因缺表不可用。

新库在现有迁移锁和事务内建立最新结构，schema 3 打开时不再运行旧开发密钥重加密、明文正文迁移或抽样解密预检。实际读取密文时仍验证 GCM tag。配置加载保留真实解析错误，缺失或空运行密钥在现有 env 文件中以 0600 原子写入，替换空条目；有效 Base64、Base64URL 与 hex 密钥保留。

Claude reader 一次性升级查询已删除，通知基线仅首次建立或启停变化时写入，重新启用建立新基线。保留当前通知队列、去重、认领、重试次数、成功分段、任务、审计及其他业务数据；不打开或迁移原生会话数据库，也不执行额外空间回收。

## 构建、CI 与发布

单一根 Cargo workspace 统一纳管 `agenthub-core`、`agenthub-ffi` 与 `agenthub-webd`，共用一份 `Cargo.lock` 与一致的 `edition`／`rust-version`；`macos/Package.swift` 声明 Swift 6（`swift-tools-version: 6.0`、`.macOS(.v12)`、零第三方依赖）并链接 `libagenthub_ffi.a`。开发与 CI 使用 `rust-toolchain.toml` 声明的 stable 工具链；`check-workspaces.py` 核对三成员继承声明、共同外部依赖解析一致性及 Swift 6 包清单；`check-versions.py` 核对 Cargo workspace、`Cargo.lock` 本地包与 `DesignTokens.swift` 回退版本号一致。

升级依赖时先确认 manifest、feature 和实际依赖链，再定向更新 `Cargo.lock`、运行 Rust 与 Swift 检查及两项 CI；不得自动全量升级。契约 `schemaVersion` 与 API 协议单独校验。

CI 保留 `Backend & contract checks`（`ubuntu-24.04`）与 `macOS Swift 6 checks`（`macos-15`）。第三方 Action 固定 SHA，权限按作业收敛，超时均为 45 分钟。Cargo test、clippy 和正式构建均使用 `--locked`；缓存拆分为 `restore`／`save` 并只在 `refs/heads/main` 未精确命中且前置步骤成功时保存（标签与 PR 只读不写），Cargo 编译缓存区分平台、架构、工具版本与锁文件（macOS 缓存不再包含 `macos/.build`），固定版本 `cargo-audit` 缓存键不含 `Cargo.lock`。`cargo audit` 保留 JSON artifact：高危、严重及无法判级的活动漏洞阻断，低/中危和 RustSec informational 警告进入 summary；扫描失败、数据库获取失败或报告不完整也阻断。`macos-15` 同时验证 Debug 编译、Release `-O -whole-module-optimization` 编译、`agenthub-ffi` 测试与开启 `AGENTHUB_RENDER_TESTS=1` 的全部 Swift XCTest（含离屏真实视图光栅化断言）；非标签触发时，按排除根目录 `*.md` 与 `docs/**/*.md` 后的 Git 代码树哈希（`scripts/ci-code-tree-hash.sh`）查找 `main` 已完整通过的 `macos-verified-<hash>` 标记，命中时跳过工具链与原生构建测试步骤并以原作业名成功结束，标签发布与 Backend 作业始终完整执行。

Release guard 核对 tag/版本、契约、隐私、发布说明和同提交两项成功 CI。guard/Linux/macOS/publish 超时为 10/45/60/15 分钟。发布仅有 macOS ARM64、Linux x86_64 webd 和汇总阶段，共八项资产：DMG/校验文件、updater 归档/签名、`latest.json`、`agenthub-webd-linux-x86_64.tar.gz`/校验文件/分离签名。updater 只映射 `darwin-aarch64`，文件名的 darwin-arm64 是同一架构；webd 不能成为桌面 updater 条目。

`scripts/package-macos-swift-arm64.sh` 通过 `--remap-path-prefix`、`-file-prefix-map` 与 `-no-serialize-debugging-options` 剥离本机路径，在生成 DMG 和无 `./` 前缀顶层 `AgentHub.app` updater 归档前完成代码签名（默认 ad-hoc，支持 `MACOS_CODESIGN_IDENTITY` 覆盖），并检查完整 App 的 deep/strict 签名与 `--payload` 隐私门禁。代码签名与 updater Minisign 独立，ad-hoc 不代表 Developer ID 或 Apple 公证。桌面端更新在 Rust `agenthub-ffi` 内使用 `minisign-verify` 校验 `"ED"` 签名、安全解包并原子替换安装。

正式安装复用同提交成品，先远程服务后 App；部署输入不进入仓库。两端归档统一通过 `scripts/sign-minisign-archive.py` 调用 `minisign` 生成带版本 trusted comment 的 Base64 分离签名，私钥仅从 GitHub Secrets 环境提供。在线下载先校验发布 SHA-256，再用预装 Minisign 与固定公钥验签，失败不解包；指定版本需与已签名版本相同，解包后核对布局、ELF x86_64 平台及程序版本。显式本地 `--archive`/`--binary` 运维方式保留。部署另比对本地与远端文件 SHA-256。解包拒绝路径穿越、链接、特殊文件及重复成员；目录与可执行文件统一 0755，普通文件 0644，不继承特殊位或可写权限。程序与部署文件通过同文件系统临时文件原子替换，普通安装失败恢复本轮替换项，不回滚业务数据库或原生会话；在线更新仍使用既有异步重启，成功调度不等于新进程已启动，也不提供崩溃自动恢复。安装成功/启动检查与用户功能验收分开报告。维护当前文档并使用普通增量提交，历史由 Git 和 Release 保留。

Claude 删除按批次建立不读取正文的文件元数据索引，项目文件及索引按实际结果处理；同 ID 的其他副本保留或失败时不删除全局文件、历史、登记和原生后台作业。统一清理执行重验 Inactive，普通删除沿用显式选择规则。Grok Bot 当前交互先按小时续期终态去重标记再维护保留期，来源读取失败时保留其标记；附件通知只显示名称，通用摘要完整移除括号或尖括号包裹的链接目标。Antigravity 检查当前回合相关子代理图，不按数量截断，保留身份去重、循环检测及完整轨迹检查。
