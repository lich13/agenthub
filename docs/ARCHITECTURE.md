# AgentHub 架构说明（维护者文档）

本文描述当前实现及维护边界。用户操作与下载见 [README](../README.md)，视觉规则见 [DESIGN](../DESIGN.md)，部署步骤见[部署手册](cloud-deploy-runbook.md)，交付状态见[当前进度](progress/MASTER.md)。

## GitHub CI 通知

`github.ci.status` 通过 AgentHubUseCases 及薄 Tauri／SSH RPC 适配返回账号与运行摘要；`github_ci` 为可选能力，旧服务不显示面板。配置沿用 `probe.settings.get/save` 的 `notifications.notify_github_ci`，默认 false，监控还受 Probe 主开关控制。Bark 的启停独立控制实际投递。

现有 serve／monitor 启动独立扫描循环，App 与 Hook 的后台启动判断均包含 CI 开关。关闭 CI 时只读取现有缓存，首次读取与手动刷新均不请求扫描，refreshing 为 false；DesktopState 不再持有独立 Monitor。`gh auth token --hostname github.com` 只在来源机器的当前服务用户下执行，凭据仅驻留后端内存。扫描直接使用 GitHub REST Actions API，复用 HTTP 连接、ETag 及同源校验；账号名下公开／私有／fork 仓库目录每 10 分钟分页刷新，并淘汰已消失仓库的运行缓存。每 60 秒仅查询各仓库首页 `per_page=10`，不翻运行历史或追查窗口外旧运行／旧重跑。前端先筛选仓库，再按现有更新时间最多展示 20 条，展示上限不限制通知判断。运行日志和产物通过 GitHub 查看。

扫描期间新增刷新请求由本轮结果满足，设置启停或基线变化仍丢弃旧结果。限流时间只留在 Monitor 内存，限流期间的刷新及时结束并返回现有错误；状态保留首次读取需要的 last_scan_ms，旧版含 retry_at_ms 的状态可读取但不再写入或公开该字段。

现有 settings 保存基线、有界摘要与待交接事件，未变化时不重写；协议 2、schema 3 无变化。首次启用、重新启用及账号变化的基线轮跳过全部已完成记录，在途任务结束仍通知。Monitor::tick 直接返回可投递事件，Stored.pending 继续持久化；成功处理的事件批量确认一次，中途失败保留尚未处理项。过期事件过滤、首发有效性和加密队列重试复核保持。账号 ID、仓库 ID、run ID、attempt 构成投递身份；仅 success、failure、timed_out 进入现有加密 Bark 队列。关闭、切号及新 attempt 会停止旧投递，重启保留去重与有限重试。Bark 内部请求增加可选 url，旧队列未含该字段仍可读取；CI 直接使用 GitHub 返回的 html_url。通知历史维护直接使用 SQL 删除结果计数，移除无调用的统计及 dry-run 分支；去重按复合主键原子认领或过期续期，过期标记一次批量删除，保留期、保护集合及范围不变。

桌面通过单个 60 秒串行观察任务读取当前机器缓存，与 Probe 页面可见性无关；`github.ci.status` 的 `cached_only` 默认 false，为 true 时不请求扫描，响应带已有 `baseline_ms`。观察状态仅保留机器修订、账号、启用基线及运行尝试身份，不落盘；首次观察跳过已完成记录，换机、切号和重新启用重新建立基线。读取沿用现有 SSH 通道，迟到响应不参与通知。macOS 使用 UserNotifications，请求系统授权、支持前台呈现和点击打开运行页；Bark 的持久化队列、确认与重试独立保留。完成记录不再额外受全局 1000 条限制。

CI 比对以本轮临时索引查找新旧运行及待发事件，排序、attempt 与基线语义保持；只对符合通知条件的新增完成结果解析时间。Mac 只在观察到待发事件时申请系统通知授权，由原生授权回调发送该批事件，观察循环不等待；消费前核对来源，已接受事件按原机器名称投递。系统通知标识使用运行事件身份，不包含连接修订。本机 CI 关闭时重置观察状态且不读取数据库。HTTP 客户端与 updater 共用 reqwest 0.13.4 和 ring。

关闭的 Provider／错误监控仅状态或统计变化时写库，启用状态仍更新扫描时间。Claude Hooks 设置操作不再读取无用配置；通知扫描仍只在处理启用的 Claude 时读取 Hook 配置。Probe 同步聚合进入既有 `read_tasks::spawn_blocking`，复用本轮 Grok／DSH 定位结果，仅读取候选会话的通知状态，不请求目录大小。后台通知扫描范围保持；首发和重试先定位目标，再复核对应来源，Grok 目标事件仅复核一次，DSH／Grok Bot 不读无关正文。活动态势只消费 `active_sessions`，通知汇总包含 Grok Bot。

隐藏扫描统一 start/poll/cancel，取消旧无模式入口；线程行及其中两种父 ID／JSON 关系只读一次，再在同一连接中补充 spawn edges。来源租约保持至终态写完；统一预览使用 NoRequest 无参数契约，兼容旧空请求，内部固定范围以枚举遍历；执行仍校验外部范围、确认、目标和指纹，不同步扫描隐藏线程；分页器负责开始／停止与终态回调，缺失分页进度直接报错。Probe 两端共用保存入口，待写文本直接解析并加载 env、规范化，仅在文本变化时原子替换配置路径并保留原权限，不维护符号链接；空补丁不写 TOML，Key 仍加密写入，env 仍为 0600。DSH 归档／恢复只定位目标，删除继续使用排他租约并核验 inode 和后代；统一归档清理直接由 execute 扫描与校验一次；改名与归档后复用定位结果及最新归档集合生成摘要。

## 运行架构与模块

`desktop_embedded_tauri` 是唯一 React UI 宿主；`linux_server_api` 只提供管理 API 与健康检查，不提供网页、登录或静态资源。组件依次调用 query/domain、类型化 transport 和 `AgentHubUseCases`；Provider 读取与受控写操作位于共享核心，SSH RPC/Tauri 只做薄适配。

| 模块 | 职责 |
| --- | --- |
| Codex | 原生 state/index/rollout/log 读取、归档恢复、直接删除，以及本机主线程的原生桌面协作 |
| Claude Code | 递归发现、分支历史、原生进程身份、改名与按文件名定位的直接删除 |
| Grok Build | 原生目录发现、主回合历史、登记与进程校验、改名及定向删除 |
| Antigravity | 本机在线 Connect-RPC 与离线 SQLite／pbtxt／transcript 双模会话列表、分页详情、改名、归档恢复与含子代理后代的清理 |
| DeepSeek Harness | `workspace.json` v2 与明文／多帧 Zstandard 会话读取、`session.lock` 并发保护改名，以及要求退出运行中 DSH 后的归档恢复与永久删除 |
| Probe | 错误与终态识别、游标、基线、去重、脱敏、保留策略和加密 Bark 投递 |
| AgentHub 数据库与配置 | 自有设置、任务、事件、投递、加密发送队列及迁移；不包含原生 Provider 数据迁移 |
| WebUI | 展示契约、查询状态、执行分组、搜索、附件、Plan 导出及本机 Codex 协作输入区 |
| 打包 | macOS ARM64 Tauri App/updater 与 Linux x86_64 无界面 webd |

主窗口由 `desktop_boot` 隐藏创建，只在初始化时最大化一次或使用工作区回退，Ready 显示；Reopen 只显示、恢复和聚焦，不重新设置几何尺寸。

## 共享契约与修改流程（Shared changes）

API 协议固定为 **2**，`system.capabilities` 返回协议、宿主和能力矩阵。产品版本、协议版本和契约文件的 `schemaVersion` 是不同概念，不互相替代。

1. 同步 `contracts/agenthub-contract.json` 及其 schema，明确 action、DTO 归属和宿主能力。
2. 同步 Rust 核心 DTO/use case 与 Linux/Tauri 适配层。
3. 同步 WebUI 的类型、query/domain 和对应测试。
4. 执行契约、Rust、WebUI、安装及隐私门禁；准确命令见 [README 开发](../README.md#开发)。

共享动作必须在 registry 中有明确映射；桌面专属 `remote.*`、`plans.save`、`codex.desktop.*`、`grokbot.list/detail` 不进入 Linux dispatcher。仅保留现行命令正向注册，未知命令统一拒绝；不保留退役表、空 transport/internal 分类或 retiredActions。新增类型或可选字段必须同步真实序列化结构，不能仅增加同名标记。

`contracts/wire-types.json` 是测试用映射及审阅后的源结构清单：Rust 用已有 syn/quote 开发依赖读取实际类型、serde 属性、枚举、适配签名及动态 JSON 表达式；TypeScript 用锁定编译器 API 读取实际声明及调用包装。自由 JSON 与内联请求必须明确归类；代表性真实序列化测试验证缺省、null 和枚举拼写。仅名称标记不能通过实际类型/注册函数检查，负面 fixture 覆盖缺字段、缺映射和缺适配入口。更新清单必须同时审阅线上 wire shape，不能用重新生成清单代替兼容性判断。

## 本机 Codex 桌面协作

`codex_desktop` 能力仅属于 macOS 的 `desktop_embedded_tauri`。九个 `codex.desktop.*` 桌面命令经类型化 query/API 和 Tauri 适配进入 `codex::desktop`；Linux dispatcher、远程 allowlist、原有 RPC 与 API 协议 2 保持原边界。旧服务没有该可选能力时继续阅读。归档、不可核验、远程来源和子线程不能执行。

IPC 通过私有 Unix socket 连接原生桌面，校验同 UID、文件身份、权限和 peer；逐次发现 owner，跟随精确本机线程。长度前缀帧上限 64MiB，连接及普通请求等待 10 秒，start 为 90 秒、steer 为 30 秒；原生 timeoutMs 与本地期限一致，等待时继续处理状态补丁。状态快照与补丁严格检查修订号、路径和预算，失步暂停写入。每个 App 最多四个会话 worker；只按需释放无队列的空闲 worker。独立的短生命周期 bundled helper 只读取模型/模式/技能/应用/插件目录或创建空线程，不执行 turn、resume 或队列；helper 串行许可最多等待 60 秒，繁忙与原生读取超时分别提示。取得许可后，同一个 helper 完成一个目录的全部分页：应用整体 60 秒，其他目录整体 20 秒；分页不重启进程。应用请求固定 threadId:null，不 resume 原线程，可用性按全局配置评估，线程级门控【未核实】。退出或取消时仅回收自有 helper。

原生执行使用 `desktop/protocol.rs` 中固定的白名单，对照的桌面 build 20586 与 20770 方法表一致：stream=11、queue=2，start=2、steer=1、interrupt=4、settings=2、审批/问答/elicitation=1；initialize=0、owner discovery/follow=1。没有猜测或探测新的写入方法。协议不支持、owner 不可用、修订失步、超时分别返回固定中文提示；协议不兼容只停止对应操作并提供原生处理入口，owner/失步提示打开同一线程或重连，不按 build 号禁止发送，不探测新的协议；历史阅读仍可使用，不启用其他执行器。发送前重验线程存在、未归档、当前 owner 与状态；补充/停止绑定当前回合。AgentHub 自有发送队列独立于原生整表队列，不覆盖原生排队内容。只在已核验空闲状态时发送，并等待返回的确切回合终态才发送下一项；原生界面并发操作仍由原生桌面处理，IPC 不提供跨界面的原子空闲锁。

自有数据库 schema 3 保留加密发送记录；旧版本迁移由 v2.0.10 完成。专属 Keychain 密钥通过 `CredentialAccess` 访问，没有开发密钥回退；缺失密钥时保留已有密文。跨进程私有文件锁限制唯一调度者。认领先落盘，只有原生确认或匹配 client message ID 后清除正文；断线/重启的未发项暂停，正在发送项标记结果不明，不自动重发。普通补充失败保留待取消，不能在恢复队列时改成新回合。新增非空引用区间以已解析引用索引保存，不依赖会过期的界面 handle；新提交同时记录内部 nativeMentions 标记，缺失时读取旧文本与 text_elements 编码，不重写已有密文。没有区间时沿用旧加密正文的逐字节格式，client message ID 仍按原正文判等，格式变化不能绕过幂等检查。停止先暂停本线程未发队列，移除结果不明记录需要明确核对。

单条加密正文最多 1MiB，总密文 32MiB，最多 1,000 个未结项、每线程 50 项，已结束身份保留 1,000 项；限制覆盖保留内容，不代表进程 RSS 上界。目录投影缓存最多八个快照/8MiB、每快照 20,000 项，60 秒失效，每页 100 项；选择授权最多 256 个、30 分钟，绑定连接 revision 与线程。缓存锁不跨越 helper I/O，同来源、线程和目录种类的并发读取合并；失败只共享给当轮等待者，不缓存。models、modes、skills、plugins、apps 独立读取、缓存、失败和重试。可选 forceRefetch 只允许首页使用，绕过对应缓存；仅 skills/plugin/app 已核验方法携带原生刷新参数。模型及 effort 由 model/list 校验，模式由 collaborationMode/list 与当前 default/plan 支持集取交集；core 只限制设置字符串非空、128 字节且无控制字符。模式变化保留合法 effort，不合法则 null 继承并提示；设置失败显示原生 view 的真实值。失效 cursor 不能继续使用，界面重试会从首页开始；前端先发起已安装插件请求，应用与插件分别发布结果和重试；缓存命中不等待另一种目录的 I/O，原生 helper 仍串行。单个来源失败或慢读不丢弃另一个成功来源，每次最多挂载 100 项。

WebUI 仅传不透明 handle，`referenceSpans` 为协议 2 内的可选字段，包含 handle 与用户文本中的 UTF-8 半开区间。输入 `$` / `@` 后可用上下键、Enter/Tab 选取技能或应用/插件，Escape 关闭；输入法组合时不选择或发送。编辑占位文本会撤销相应绑定，删除标签会同时移除其占位文本。Rust 从授权引用推导占位文字，校验身份、名称、字符边界、非重叠和最多 32 个区间，新提交按已核验的原生 composer 序列化为 Markdown 引用链接，同时保留结构化 skill / mention 和 `text_elements: []`；旧队列继续按原 `text_elements[].byteRange` / `placeholder` 构造。技能与应用链接标签为 `$name`，插件为 `@name`；界面仍使用 `$` 搜索技能、`@` 搜索插件和应用。链接由 Rust 授权引用生成并转义，WebUI 不接收路径。插件使用原生 canonical name 插入，友好标题保留为目录说明，避免显示名与原生引用不一致。

原生选择器路径和原生目录引用仅在 Rust 解析；选中文件及队列发送前核验规范路径、dev/inode、大小与修改身份。新选择的图片与不超过 20MiB 的普通文件在授权阶段以 64KiB 流式缓冲记录 SHA-256、uid/gid 和 mode；这些信息仅作为加密 FileIdentity 的可选 content_proof 保存。只有规范路径、dev/inode、大小、mtime、所有者与权限都相同，且重新读取的摘要一致时，才允许单纯 ctime 变化。读取前后重验描述符与路径身份；替换、改写、权限变化、符号链接替换或读取失败仍拒绝。大文件及没有摘要的旧引用保持严格规则，旧读者可忽略新增嵌套字段，不重写已有队列。图片限 20MiB，仍使用 `localImage` 和既有附件字段；Mention 使用 `app://id` 和 `plugin://name@marketplace`，仅展示原生可访问应用与已安装启用插件。普通文件暂保留 `# Files mentioned by the user:` 兼容前缀；旧队列同时有内联引用时统一平移区间。start 的附件元数据放在原生处理器实际读取的 turnStart.context.attachments，不能放入 app-server request。

`codex.desktop.attachments` 只处理本机主窗口的接收区绑定、坐标更新、明确粘贴、草稿引用释放和解绑；请求只含 revision、session/binding、矩形与 handle，不含路径、URL、图片字节或原生方法名。Rust 接收 OS 拖放，核对窗口可见/聚焦、区域、当前来源和可写线程后复用文件选择核验，整批超限或含无效文件时整批拒绝。单窗口和多 WebView 的原生拖放路由分别处理；禁用通用前端 event listen 权限，防止 Tauri 默认拖放事件把原生路径送入 JS，只用专用 Channel 返回名称、种类和 handle。发送或选择文件期间暂停接收；失焦、隐藏、切换与解绑后停止被动更新并丢弃迟到导入，重新聚焦重建接收区。前端只在 handle 值变化时更新授权集合；主动粘贴仍保留原有聚焦和身份检查，普通文本粘贴保持原行为。

明确粘贴时优先读取 Finder 的本机文件 URL 列表，再读取系统 PNG/JPEG/TIFF，最后识别完整路径文本；不读取 HTML 或抓取网络图片。文本每行一个绝对路径、`~/` 路径或本机 file URL，允许空格与成对引号；单段最多 32×8192 字节、32 个文件，单路径最多 8192 字节，不解释变量、命令替换或 shell。只在整段均为有效普通文件时去重并整批附加，目录、远程 file URL 和无效项不获得授权；文本识别失败时通过可选 pasteDisposition=text 回退浏览器原文，旧响应缺少该字段仍按原 Channel 处理。

图片单次捕获最多 64MiB，解码前按尺寸预留每像素 16 字节、共 128MiB 工作预算；只接受单图且深度不超过 16 位，输出 PNG 最多 20MiB，不缩放或降低质量。PNG 原样保留，TIFF/JPEG 由系统框架转换；WebKit 未提供 image MIME 时仍可通过明确粘贴读取原生图片。读取、转换和导入后核对 clipboard changeCount，同一接收区的重复粘贴不重复导入；移除附件后允许重新选择。绑定请求合并，恢复焦点后的主动粘贴最多等待接收区五秒；线程、窗口或会话变化丢弃迟到结果。普通文本保持默认编辑路径，异步文本回退使用原生 insertText，光标或草稿已变时不覆盖。导入串行，预算不代表系统剪贴板服务或进程 RSS 的硬上界。

附件命令的 onEvent 始终使用同一回调标识；Tauri 入口只解析标识，仅 bind 创建并持有 Channel，其他操作复用已有接收区。不能在每次 bounds、paste 或 release 调用中重建 Channel：临时实例释放会发送结束帧，提前关闭仍在使用的附件通知通道。解绑与切换会话仍关闭原通道，迟到事件不得恢复旧附件。

粘贴图片写入 AgentHub 数据库旁的私有附件目录（目录 0700、文件 0600、随机名、独占创建），写完并同步后捕获原有 `NativeReference::File` 身份，之后不改名或改写。图片为原生 owner 可读取的明文私有文件；归属和生命周期索引使用现有专属 Keychain 密钥加密，索引上限 1MiB，图片总量最多 512MiB/256 项。沿用跨进程 worker 锁，初始化/恢复后台工作取消也不提前释放锁。未提交草稿可显式释放或在 30 分钟后清理；隐藏/重连在授权有效期内保留草稿。发送前先原子持久化并同步保留标记，再交给原有队列，成功、失败、取消或结果不明均不自动删除这些图片，避免破坏原生历史。容量满时拒绝新粘贴，可继续使用文件选择器。启动只清理有完整归属证据的过期草稿，`Released` 标记允许接续草稿删除；密钥缺失、损坏/缺失索引、未知遗留文件或身份变化时保留文件并停止操作，不猜测崩溃后的归属，不新增数据库表或旧队列格式。

审批 key 包含完整原生请求；文件审批另外绑定对应 turn/item 的实际 changes。展示不完整、未知权限/表单约束均不得批准，保留原生处理入口。命令与文件审批在 `supportedDecisions` 可选字段明确声明后，才显示“允许本次会话”；新增可选 `decision` 保留原 `allow` 布尔兼容，只接受已核验的 accept/decline/acceptForSession 及一致布尔值。未知决策为 UnsupportedProtocol，过期 key 为 RequestExpired；权限仍限 turn scope，不创建永久授权。问答只提交当前请求的精确 ID；原生另一侧完成请求后以新的原生状态撤下原请求。

审批的可选 `review` 字段按已有字符串和结构投影命令、工作目录、原因、文件变化与网络/文件系统权限；旧 `body` 后备仍保留。未知字段放入折叠原文，命令用等宽文本、文件 diff 按文件折叠且展开才挂载；每个 diff 最多 32KiB、最多 100 文件、投影文本总量 64KiB，UTF-8 边界安全截断。原有 pretty body 16KiB/compact params 64KiB 审批守卫保持；截断或未完整显示时不能批准。序列化与请求哈希使用有界写入或流式处理，不克隆整份 params/changes，不改变完整请求绑定的哈希。

协作状态直接取当前 `DesktopView`：等待请求优先于执行、发送、排队、结果不明/暂停和空闲；同一行显示待处理数，运行复用 `RunningIndicator`。断连、只读、隐藏或 detach 后不显示旧状态，提交中显示“正在提交到 Codex…”，不添加 IPC 方法或未知进度字段。

Tauri 频道最多每秒十次投影视图，初值取自同一订阅；前端丢弃旧修订、旧连接或旧线程结果。仅当前协作线程通过 350ms 合并事件刷新详情；慢读取不被反复取消，期间的新事件在完成后触发一次尾刷新，包含审批完成和回合终态。原有阅读轮询保持，未添加全局高频轮询。隐藏/离开页面释放界面订阅和待刷新计时器，已提交的本机队列继续归原 worker；原有 monitor 和通知仍独立读取原生证据。

诊断只保存在进程内 50 条环形缓冲中；`codex.desktop.diagnostics` 只供 macOS 本机读取，连接失败也可使用。记录时间、来源、白名单方法/版本、数值 code、固定错误分类及最多 200 字符的安全提示；原生自由文本使用占位，不保留正文、路径、URL、邮箱或凭据。复制操作由用户触发，不落盘、不上传。桌面 build 与 bundled CLI 版本只读获取，失败返回未知；版本仅保留在诊断中，实际方法版本不符才阻断该操作，不尝试其他版本。

诊断请求的可选 copy 字段仅允许 Rust 将有界安全 DTO 写入本机系统剪贴板，不接收任意复制正文；重新核验本机 revision 与主窗口焦点，总文本限 64KiB。界面明确显示复制成功或失败，不依赖 WebKit 异步剪贴板授权。省略字段保留只读响应；诊断版本来自当前本机安装，不代表已验证该 build 的全部行为。

可选 DesktopView.goal 只投影原生 snapshot 的 threadGoal／completedThreadGoal 中目标、状态与截断标记，目标最多 4096 字节，未知状态固定显示待确认。支持已核实的 active、paused、blocked、usageLimited、budgetLimited、complete；随现有快照和补丁同步，断线不显示旧目标。创建、修改、暂停、恢复只提供既有“在 Codex 中打开”入口，不添加 follower Goal 方法，不恢复退役调度器或 Goal RPC。

原生 `request-timeout` 与旧 `timeout` 同属超时，提交结果仍按未知处理且不重发。原生 `request-version-mismatch`、`no-handler-for-request` 与旧名称映射为协议不支持；`no-client-found` 属于 owner 不可用。这些已核验的执行前拒绝可标记未发送。任意 handler 异常不能证明没有执行，start/steer 仍为结果不明，不重发。Settings、Stop、审批、问答和 elicitation 的明确拒绝保留 Rejected 且不断流；身份/传输故障不能伪装成业务拒绝。空线程创建提交后出错或返回身份不完整也保持结果不明，界面不重复创建。

失步先暂停队列，再自动重建一次订阅并获取完整 snapshot；断线按 1、2、4、8、16 秒最多五次尝试，每次重新校验 socket、UID、线程与 owner。同 owner 可恢复协作；owner 变化只恢复阅读，明确点击“连接”确认后才能发送、设置和审批，队列仍需手动继续；只读期间仍显示暂停与结果待核对项，禁用队列操作。只有 snapshot 的精确 client message ID 可核对 unknown。恢复旋转会话代际、重置修订比较；Tauri Channel 在恢复期间保持订阅，旧动作和附件结果被拒绝。可选 confirmOwner 区分明确连接意图与隐藏后重新订阅；旧请求不自动确认已有 worker 的新 owner。点击“在 Codex 中打开”或新建后自动连接，owner 等待总计 20 秒；普通浏览不自动连接。新建后保留已知身份，打开或后续选中失败只能重试打开/连接。线程连接 I/O 使用按线程的准入锁，不持有跨线程 worker 注册锁。

### 证据与未核实项

1.3.4 的隔离文件样本已复现 macOS 扩展属性更新只改变 ctime、内容与其他身份不变时被误拒；摘要复核覆盖此情形，不能据此确认用户截图中那次失败的唯一原因。新增 Finder、路径文本、PNG/TIFF/JPEG 使用专用 pasteboard 测试，不触碰真实剪贴板；修复候选真实粘贴与发送由用户确认，覆盖范围见[1.3.4 验收记录](https://github.com/lich13/agenthub/blob/2b443fb412670e436bdb919affeef405f4efe200/docs/progress/MASTER.md#候选与用户验收)。

1.3.3 修复前已只读复现：带线程 ID 的临时 helper 返回 -32600/thread not found；threadId:null 的应用冷读约 29–32 秒，同一 helper 第二页约 108ms；model/list 实际包含 max。当时桌面 build 为 20586、bundled CLI 为 0.163.0-alpha.10.2；这些目录探测不是回合或两端功能验收。

1.3.1 的只读核对依据桌面 build 20586 应用源码；`crates/agenthub-core/tests/fixtures/desktop-native-input/source-derived.json` 保存源文件标识、SHA-256 和脱敏合成输入，**不是桌面采集快照**。`conversation-turn-mutations` 确认内联字段为 camelCase `byteRange`；审批面板的 Qr / $r 与 follower 转发实现共同确认命令和文件的 `acceptForSession`。未复制原生源码到运行时代码，也未采集真实用户消息。

1.3.3 对照 build 20586 的 composer dG/uG/getPersistedText，确认已解析技能、应用和插件使用 Markdown 链接；原生 Ej 从 context 读取 attachments。`mention-links-source-derived.json` 仅为脱敏源码派生 fixture，不是 liveCaptured。只读 Goal 依据同一构建的 Jg 投影及目标面板状态枚举；参考项目的独立 Goal 元数据通道未采用。原生应用目录冷读仍可能等待约 30 秒，缓存和独立展示不能等同于降低原生网络耗时。

Goal 仅支持状态显示与原生操作入口，直接创建、修改、暂停和恢复不在当前范围内。build 20770 所带 CLI 0.163.0-alpha.10.2 的实际 schema 包含 `thread/goal/get`、`thread/goal/set`、`thread/goal/clear`，但它们不是已核验的 follower 方法。`origin: user` 用于明确的用户操作；缺失来源不能代表用户授权。[参考项目的 GoalRPC](https://github.com/try2love/codex-mobile-bridge/blob/70a275b0eabea6f3bda333f35ef925741927bd11/bridge/features/sessions/goal.py) 另开元数据 app-server，写入未携带该来源，再经桌面启动消息激活目标，不能据此认定与当前原生操作等价。隔离双进程验证中，owner 先创建 paused Goal 后，第二个进程可读目标，但携带 `origin: user` 的修改和清除返回 -32603（记录用户目标指令失败）；关闭 owner 后同样的修改与清除成功。未触发 turn，未操作真实会话。该结果不支持在保持当前 owner 写入边界时直接复用此通道，也没有证明跨进程写入能同步桌面内存状态；不得省略用户来源、关闭用户 owner、直接写原生数据库或模拟 follower 方法来绕过。

2026-10-18 用户诊断显示桌面已为 build 20770，bundled CLI 版本不变。只读核对其 `src-DsLVGakC.js` 的 SHA-256 与上述 fixture 所记源文件一致，现用连接、订阅与写入方法版本均未漂移。用户在目标线程尚未由桌面打开时点击“连接”得到 Timeout，按先打开同一线程的流程操作后确认连接正常，随后明确确认其余候选验收项目通过，范围见 [1.3.3 验收记录](https://github.com/lich13/agenthub/blob/cfb1be16f2f49200db8db6cfae6eab9ea797597d/docs/progress/MASTER.md#候选与十项用户验收)。该轮没有完成独立只读 socket 探测，不把用户验收写成助手实机测量。兼容检查依据方法版本、状态形状和身份，不以桌面 build 数字拒绝连接；新增未知字段不自动成为可写能力，未知方法版本仍停止对应操作，不能保证未来不兼容协议无需适配。

- **【未核实】** 桌面原生文件引用的完整 wire 形状。1.3.2 未采集 liveCaptured snapshot，N0/N1 按批准范围跳过；因此普通文件兼容前缀和旧加密正文保持，不宣称与原生气泡完全一致。
- **【未核实】** 文件审批 `cancel`、永久授权、策略修订和其他未列举决策；未暴露按钮或发送这些字符串。
- **【未核实】** 本轮真实 skill/app/plugin/文件/图片 snapshot 未采集。用户已确认候选两端流程通过，未提供逐项延迟测量或覆盖全部视觉/滚动场景；合成 socket 与 UI 单元测试不替代这些证据。
- **【未核实】** 仅 IPC pending、rollout 未落盘时的漏通知。当前 monitor 依据原生 rollout，尚无可复现的漏通知证据，N6 不扩展 worker 通知，也不声称已经证明无缺口。
- **【未核实】** 除上述证据及用户验收覆盖范围外的桌面版本与操作。仅 build 不同只显示提示，具体 IPC 方法版本或状态核验不通过时只读降级，由用户在 Codex 中继续处理。

## Provider 读取与历史

### 共享读取模型

`jsonl_cache` 核对文件身份、大小、mtime/ctime 和已消费前后锚点。冷读流式消费，普通追加只解析新增完整记录，半行等待写完。各来源独立串行；解析、分页不持有全局缓存锁，未变化结果复用不可变快照。列表、详情、搜索和子智能体复用快照；管理操作独立重新核验来源。

`read_tasks` 使用持有至真实结束的许可，把耗时读取移入有界后台，避免阻塞 Tokio 执行线程。摘要缓存保留服务端发现的身份，使用前重新校验；macOS/Linux 的派生摘要复用 dev/ino、大小、mtime/ctime，其他缓存保留边缘校验。带附件响应按需复制、重新检查附件，后续追加不能改写旧响应。

`history` 包含 epoch、revision 与有界变更记录。来源替换、截断或分支改变使旧窗口失效。Codex 保留 `b:index`，支持附带块 ID 的位置提示及 `a:block-id`；现有 `before` 同时支持 `j:` 定位、`p:` 向前、`n:` 向后、`w:` 刷新窗口。Grok 的 limit/before 可选，省略时保持原完整响应。

### Codex 与子智能体

详情直接按原生 ID 定位。完成工具块保持原顺序，结果与调用配对并放在调用位置，历史压缩保留工具分隔。`history_navigation` 在分页和压缩前建立完整用户消息索引：重复文本仍保留独立原生身份，纯自动上下文不进入索引。最新详情可返回 `timeline`，旧窗口不重复传输；定位窗口使用未压缩块。

`MessageBlock.subagent` 通过原生调用 ID 配对创建与结果，再按线程 ID、精确任务路径和父子图关联；不按昵称猜测。`threads.subagentDetail` 从可见主线程检查唯一父链、循环、深度、索引、rollout 身份、符号链接及读取前后指纹，不接受客户端路径。子线程历史独立分页，附件以 `rootThreadId` 绑定相同关系验证。`subagent_updates` 可更新已加载但不在最新页的卡片。

`item_completed → SubAgentActivity` 的 started/completed/interrupted/interacted 表示历史活动，不能用外层完成推断子线程终态。同次创建、结果及开始事件去重，续接的开始时点保持独立。`eventKind`、`eventId` 保留原始活动身份；任务名按 task_name、已验证路径末段、标题和昵称的顺序取得。

`ThreadDetail.subagents` 在分页前按直属原生线程 ID 去重，包含状态计数、完整性及逐项不可用原因，不累计孙级或回合数。当前状态只依据子线程自身回合和未完成工具，父文件未变也需刷新；存在运行中或创建中子线程时采用活动轮询频率。UI 可省略通用完整性说明，但不得改变 complete/warning、关联和未知状态语义。

### Claude Code

opaque session key 绑定项目相对路径和文件名身份，同名 ID 互不混淆；子智能体文件不作为独立主会话。详情按稳定事件身份分页，保留分支和原生事件顺序。兼容常见 2.x user/assistant、标题、工具、queue/history/usage、compaction、API 请求辅助、会话设置、续接及已核实 system subtype。兼容 warning 仅展示，未知记录不伪造完成或权限；损坏、半行、身份冲突、路径和权限异常继续阻止改名。

原生会话登记结合会话 ID、PID、进程身份、启动标识和主机进程域判断占用，打开 transcript 与显式恢复参数补充证据；PID 复用或不匹配登记不能证明占用。已明确属于其他会话的进程不锁住同 cwd 的全部历史会话。不使用静默时间窗口。占用及版本判断用于改名；canDelete 保持可用，删除不依赖这些判断。

改名仅追加原生 `custom-title`，执行前核验归属和指纹。删除按文件名定位主 transcript、同名目录和原生旁路，专属目录包括 subagents、工具结果和元数据；共享索引只移除对应记录。删除范围详见下方管理与隐藏扫描。

### Grok Build

活动快照用一轮进程表校验登记数组/对象、原生可执行文件身份与进程启动时间。退出、僵尸或复用 PID 不能证明仍运行。只有活跃归属与未结束主回合同时成立才显示运行；不可读、损坏和身份不确定保持未知。

冷读消费完整历史，不截取尾部；增量缓存保留文件身份、ctime 和前缀锚点。优先读取 updates.jsonl，缺失时使用 chat_history.jsonl。可选 `deletionEligibility` 保留协议兼容并返回可删除；删除按原生目录定位，不采集进程快照或核验指纹。

### Grok Bot

仅 macOS 桌面通过 `GrokBotUseCases` 暴露 `grokbot.list`／`grokbot.detail`；能力字段 `grokbot_sessions`／`grokbot_notifications` 为可选扩展，Linux 不注册读取入口。当前账户槽确定 roster 与 transcript 切片路径，按原生键读取短名 `.blob` 或带键头的 `.kblob`，校验格式及身份。列表只读 roster 与文件元数据，未缓存项仍存在；复制路径重新读取详情，缺失时不替换为共享目录或其他文件。

历史映射用户、助手、工具、附件与交互记录，保留原生 ID 和顺序；详情复用共享历史快照、分页与时间线，搜索和附件复用同一投影。账户与线程共同确定身份，未变化切片复用有界缓存；不导出历史、不增加监听进程或状态库，不提供写操作。

Probe 复用原生轮询、启用基线、加密队列与有限重试。只识别明确待回复／待授权，默认关闭；普通卡片、发送失败及单个工具错误不产生回合终态。请求去重绑定账户、线程与原生请求身份，滚动淘汰或原子替换不重置去重；首发及每次重试复核请求，已答复、已批准或账户切换停止旧提醒。原生缓存仅覆盖近期内容，不代表完整云端历史。

### Antigravity 与 DeepSeek Harness

`antigravity.{list,detail,rename,archive,restore}` 与 `dsh.{list,detail,rename,archive,restore}` 经共享 `SessionUseCases`、Linux RPC、Tauri 及远端 allowlist 暴露，同时扩展 `sessions.bulkPreview`／`sessions.bulkExecute`（`archive`、`restore`、`delete`）与 `sessions.search`／`sessions.attachmentRead`。API 协议保持 **2**，通过可选能力 `antigravity_sessions` 与 `dsh_sessions` 探测旧服务并平滑提示升级。

Antigravity 优先通过本机同用户运行中的 language server Connect-RPC 读取摘要与轨迹并执行在线改名、归档和删除；该服务只报告本次运行已加载的会话，列表因此合并 `conversation_summaries.db` 中尚未加载的根会话（在线条目优先，时间统一为 RFC 3339）；自动探测未命中、分页网络不可达、没有可见消息或远端环境下回退至 `ANTIGRAVITY_HOME`（默认 `~/.gemini/antigravity`）及 macOS state SQLite／`annotations/*.pbtxt`／`brain/<id>/.system_generated/logs/transcript.jsonl` 离线读写。永久删除按确认集合（含可核验子代理后代）清理 `brain/<id>`、`implicit`、`annotations`、`conversations`、`context_state` 及 SQLite 轨迹摘要条目。

DeepSeek Harness 从 `DSH_HOME`（默认 `~/.dsh`）的 `storages/workspace.json` v2、`storages/session_projcache/*.json` 与 `sessions/<id>/session.v1..v4.jsonl[.zstd]` 无锁只读列表和分页详情。改名先对 `session.lock` 获取非阻塞排他 `flock`（占用时立即拒绝），按 80 UTF-8 字节规范化标题，并以明文 JSONL 或带内容校验和的独立 Zstandard 帧追加 `session/title` 记录，同时最佳努力同步 `session_projcache`。归档、恢复与含 `origin=subagent` 后代闭包的永久删除均先扫描宿主进程表，若检测到运行中的 `DeepSeek Harness`／`dsh` 进程则拒绝写入，避免被宿主进程内存快照覆盖。 改名追加前校验完整记录与压缩帧：合法末行缺换行时补分隔；半条 JSON 或半帧报错并保持原文件，不自动修复历史。

Antigravity 与 DSH 共用有界 `ReadCache`，快照包含不可变事件及按需建立的时间线，分页只复制请求页，附件单独刷新。DSH 以原生日志身份、大小及 mtime/ctime 失效；Antigravity 离线同时检查完整／简要转录及 SQLite／WAL，在线仅复用原生 owner、摘要和完整终态未变化的结果，运行中不缓存。详情与附件复用同次请求的摘要扫描；本路径未实现追加增量解析，超预算快照仍可读取但不保留。

## 消息、搜索与界面状态

用户消息与助手 Markdown 分开处理，保留用户字面文本和换行。三个 Provider 的原生消息/图片分组形成稳定 user message；共享投影拆出正文、附件及可选 `contextBlocks`，类别固定为 instructions/environment/runtime/files。只在代码和引用之外识别完整白名单包装；允许相邻标签和完整 INSTRUCTIONS 的同行结束标签，内部标题不截断包装。原生问答回复、用户 shell 操作和中断保留专用解析。

新旧 `My request:` / `My request for Codex:` 附件包装均可识别；按原生身份去重并优先已有嵌入图片，失效临时文件不隐藏请求。消息按附件、一个默认收起的“上下文”、正文排列；上下文展开才挂载，纯上下文不产生空气泡或时间线刻度。展开原文按原顺序保留，全部清理仅用于展示，不回写会话。

`visibleMarkdown` 在解析前按源范围去除结构化 memory 与完整 `external_codex_apps_open_page`/page_id 元数据，并有渲染保护；CommonMark 代码、引用和普通示例不被误删。显示、复制、预览、Plan 内容和文件名共用清理结果。指令分段及完整 Markdown 转换缓存不保存 React owner、路径解析结果或折叠状态，每次恢复独立语法树后继续执行链接安全处理。

`sessions.search` 是当前机器内的只读动作，输入 Provider、线程/Provider 范围、opaque key、query、cursor 和 limit，不接受文件路径。搜索摘要与有界正文页，返回稳定 block/event position key；达到现有 200 条上限立即停止后续历史读取，保持原顺序和分页规则。当前线程按选中身份直接读取，不扫描最近 200 条列表。用户搜索取实际正文、问题/回答、附件名和指令块，自动字段不重复命中。AGENTS.md 命中仍能展开上下文。Plan、工具、助手回复保留搜索锚点，但不成为用户时间线刻度。

`usePagedHistory` 分离最新窗口轮询与历史取页；静态页不重复请求，只刷新变化的已加载身份，快速追加时补齐中间窗口。`historyScrollCoordinator` 统一首次底部定位、上滚、搜索分页、请求互斥和可见锚点补偿；失败或游标不前进停止自动重试。`historyWindows` 保留最早游标；`useTimelineWindow` 直接取定位窗口，复用最新变更记录，无需下载中间全部历史。

搜索通过 `DisclosureRevealContext` 挂载命中折叠内容后定位，优先精确原生 ID，其次最深别名节点；Codex 压缩只豁免当前机器/线程最近命中的一个块，不展开整段历史。时间线位置只在布局变化时重测，刻度有界挂载且键盘可达全部记录。工作区按需加载，隐藏窗口暂停界面轮询，后台 monitor 独立运行。

显式折叠选择按机器、Provider、会话、活动身份保存在有界内存中，不写数据库。机器、线程或来源变化取消无效请求并丢弃迟到响应；子智能体面板有独立分页、滚动与嵌套返回，父视图保持挂载。连续工具组保持原位置，完成默认收起，活动/失败默认展开，指令组始终默认收起；用户选择优先于默认值。

### 附件、路径与 Plan

`sessions.attachmentRead` 经共享 use case、身份校验和薄适配读取；只接受 Provider、session/message/attachment ID，以及子线程必要的根线程上下文。重新解析来源及文件身份，PNG/JPEG/WebP/GIF 单张预览限 20MiB。视口附近才读取图片，前端缓存按机器隔离并在切换时清除；轮询只携描述，不含图片，不抓取仅有远程引用的附件。

文件链接在浏览器解析 URL 前从原始 Markdown target 提取，剥离行列后缀并按线程 cwd 解析相对路径。远程只复制，本机 macOS 通过 Tauri opener 在 Finder 定位。`plans.save` 始终是本地 Downloads 导出，校验有界 Markdown 文件名/正文并独占创建，不覆盖既有下载、不经过远程 RPC。

## 管理与隐藏扫描

五种 Provider 共用 useSessionSelection。⌘ 切换单项、Shift 按可见顺序追加区间，切换机器／Provider／筛选或退出多选清空选择。轮询不选择新行。删除没有批量数量上限；归档、恢复保留原有规则，客户端按 100 项分批提交。

直接删除预览展示目标，一次确认后仅执行提交的根 ID 和 descendants，重新计算的父子关系只用于归属校验，不扩大集合。Codex、Claude 与 Grok 的直接删除不依据正文、活动进程、writer 锁、终态或旧 fingerprint 阻止执行；Antigravity 与 DSH 保留所选目标的运行、占用和原生写入校验。统一清理另复核预览指纹及最新允许集，API 协议保持 2。目录流式遍历，没有文件大小、深度或文件数量业务上限；符号链接只删除链接本身，共享文件只移除目标记录，原子替换保留 uid／gid／mode，并与读取前的文件身份比较；发现并发变化时明确报错，由用户重试。

Codex 依据原生父子关系生成后代集合，循环去重；includeDescendants 和 SessionBatchSelection.descendants 明确传递确认范围。执行不再发现新后代，也不扩大到引用目标历史的其他线程。直接清理普通／归档 rollout、压缩副本、writer 锁、shell snapshots、visualizations、原生数据库、分页历史和名称索引，不调用 helper。归档、恢复仍使用原有指纹和状态规则。

Claude 的稳定定位键由项目相对路径和文件名身份生成，与正文解析独立。精确匹配 `jobs/*/state.json` 的原生 ID、`sessionId` 或 `resumeSessionId` 后，先使用来源机器已有的 `claude rm` 清理后台记录及工作树；原生命令未支持或未成功时保留历史，不强制移除或停止原生进程。没有匹配后台记录时不需要 CLI。删除主 JSONL、同名专属目录、按 ID 定位的 file-history、tasks、session-env、debug、todos，以及项目索引、共享历史和会话登记中的对应记录。Grok 直接删除各工作区下的目标 ID 目录，并移除 active_sessions.json 对应登记；摘要缺失或损坏不阻止定位。

单条、批量和统一清理共用删除实现。仅保留 cleanup.unifiedDryRun／cleanup.unifiedExecute 与渐进 cleanup.hiddenDryRun，旧独立 archive／hiddenExecute／dshArchive 入口及适配器已删除。缺失文件视为完成；权限、文件系统和数据库失败返回逐项 failed/residuals，继续处理其他目标，不回滚整批。数据库修改按目标事务执行。原生程序之后新写入的数据在刷新后显示，不启动重复删除任务。

cleanup.hiddenDryRun 保留 start/poll/cancel、扫描 ID 与游标和旧完整响应。只扫描元数据及原生父子关系，不核验正文或指纹；每页最多 100 项，候选总数没有业务上限。同源最多一个活动扫描，进程最多两个上下文；五分钟无请求释放。界面加载完整确认范围后允许执行，停止结果不能执行，切换机器隔离迟到响应。

DSH 归档清理纳入统一 CleanupUseCases、Linux RPC 与 Tauri 入口，能力字段 dsh_archive_cleanup 可缺省为不支持，API 仍为 2。独立会话入口另由 dsh_sessions 能力声明。DSH_HOME 默认为用户 .dsh，以 storages/workspace.json v2 归档名单为准，读取明文／Zstandard session 头，只有 origin=subagent 且 parentSession 可核验的后代随归档根入选，独立 fork 保留。预览可在 DSH 运行时读取，执行必须先退出 DSH；固定 id/archiveId 经重新核验后删除专属目录、锁和投影缓存，仅移除成功身份的归档、置顶及工作区引用。后续新增归档不自动纳入，恢复或归属变化跳过，文件／索引并发变化停止相关写入，失败项目保留原因供重试。共享附件、项目和配置不清理，损坏或自定义持久化格式明确拒绝，缺少存储为空状态；systemd 仅对 sessions 和 storages 子目录授权写入，覆盖默认及自定义根。

Claude 空文件、命名记录和仅元数据会话显示“暂无对话”；缺少 cwd 或版本不产生读取失败。Grok 优先读取 updates.jsonl，缺失时读取 chat_history.jsonl，映射用户、助手和工具调用／结果，复用分页、增量缓存、搜索和时间线。系统提示、system_reminder 和加密推理不作为普通对话展示。

## 远程连接与认证

React 只向原生 IPC 传命令、参数及连接 revision。桌面读取用户 OpenSSH 配置及 Include 中的明确 Host 别名，实际连接交给系统 ssh，复用密钥、ssh-agent、钥匙串和跳板设置。桌面仅保存 Host、显示名称与 useSudo；需要交互认证时先在终端完成。原生层沿用命令 allowlist、协议 2 和能力验证。

当前机器保持一条自有 SSH 通道，固定执行 `/usr/local/bin/agenthub-webd rpc-stdio`，useSudo 时加 `sudo -n`。代理只转发至 `/run/agenthub-webd/rpc.sock`，不打开数据库或启动扫描器。常驻服务继续承担通知、缓存和后台任务；systemd RuntimeDirectory 为 0700，套接字为 0600。请求使用带 ID 的 JSON 帧与四字节长度前缀，并发响应按 ID 配对，沿用 32MiB 响应限制。

切回本机、切换机器和退出均释放自有通道；断线显示重连入口，写操作不自动重放。连接 revision 隔离 QueryClient、工作区、预览、选择、分页、滚动、标题、附件和折叠，迟到响应丢弃。仅尚未返回的写请求阻止切换；已受理作业继续在远端运行，返回该机器后从执行记录查看。jobs.list 只返回摘要，展开时通过 jobs.detail 读取完整日志，逐完整行解码和脱敏并保留尾行。

「安装并连接」经 SSH 发送 App 内置固定引导脚本，下载同版本正式服务包，复用可信公钥、签名验证及现有安装器。Ubuntu/Debian x86_64 自动补齐必要依赖；其他 Linux x86_64 需要已有 systemd 和依赖。失败按认证、依赖、下载、签名、安装或健康检查阶段报错，可明确重试；不修改 sudoers。

TCP/HTTP 管理监听、API Key 鉴权和限流已删除；旧 HTTP/Nginx、目录、配置和桌面钥匙串迁移由 v2.1.0 完成，本版不再执行。CredentialAccess 独立于 SSH，保留 Codex 发送队列密钥与业务审计。systemd 继续保留隔离及 Provider 数据根的精确 ReadWritePaths，Grok／Claude 配置与凭据使用可选 ReadOnlyPaths。

更新任务与 Probe helper 使用内部 `ProcessCommand` 的 program/args 直接启动，不执行配置 shell 字符串。现行默认更新命令映射到固定包装器及参数；自定义 shell、环境赋值、管道、重定向和命令替换返回固定错误。任务名称、状态、输出处理、互斥与审计字段保留；备份清理直接运行固定 Python 参数，保留最近三份。序列化 command 字段仅保留兼容展示，不改变 wire shape；Hook/配置文本的 shell_quote 统一实现并保留既有引用策略。

按需恢复复用原刷新或请求，不增加轮询任务或连接修订号。同轮请求共享一个握手结果；读取最多重试一次，业务错误不触发恢复，写操作不重放，传输写失败与超时关闭旧通道。切换或退出后迟到握手不能恢复旧目标。

桌面专属 remote.sshDelete 接收 revision 与 alias，返回更新后的 RemoteConnectionView，不接收前端文件路径。用户配置与 Include 按显式别名编辑，保留注释、权限、其他别名与密钥；系统配置不能删除。原子替换失败报告并恢复已替换文件；删除对应 AgentHub 记录，当前连接被删除时回到本机。

Linux 派发直接返回 Result<Value, ApiError>，没有 HTTP 请求／响应转换或测试 Router；Axum 只作为模拟服务的开发依赖。rpc-stdio 与 health 使用单线程 Tokio，其余命令保留原运行方式。作业日志按 CR、LF、CRLF 分记录，整条脱敏后在现有读取任务中累计 16KiB 或 200ms 写入，结束前排空。jobs.detail 的可选 includeOutput 默认为 true，所有后台轮询（包括终态）传 false，展开时读取全文，终态仍刷新相关状态。作业注册表仅持有互斥组；启动、等待或读取失败统一收尾，日志排空与终态处理后释放，无现成运行时时使用单线程 runtime。前端不改写文件名或地址，后端保留凭据脱敏。

## 通知与监测

Antigravity 与 DSH 的通知仅在本机运行，会话阅读及管理是独立能力。`system.capabilities` 的可选 `antigravity_notifications`／`dsh_notifications` 仅由本机 macOS Tauri 声明；远端隐藏入口且 Linux monitor 不运行这两项，协议仍为 2。每种来源提供总开关及完成、失败、需要回复、需要授权四类开关，默认关闭，沿用现有 Probe 间隔、数据库、持久化认领和加密队列。

Antigravity 通知扫描从已运行且同用户的原生 language server 读取摘要、轨迹及执行段元数据，使用固定方法，请求限回环并禁止代理和重定向；凭据只保留在本机内存。完成须核验主执行终态及已关联子代理、后台任务收敛；失败须是当前执行的明确错误终态。原生等待、requestedInteraction、提问及审批记录按既有解析器配对并去重；普通工具、中间错误、取消和自动继续不推断失败。服务未运行、身份不明或读取失败在 Probe 来源状态显示异常。

DSH 复用标准存储边界，读取 v4 JSONL 或逐帧解压追加的 Zstandard，未写完整的尾部等待下次扫描，完整记录按序列核验。turn/start 与 turn/end 决定主回合终态；approval/asked 与 approval/decided 配对授权；ask_user_question 调用、结果及 user-question-reply 身份配对问题。超时返回 pending 的异步问题跨正常回合结束仍保持待回复，实际回答、取消或明确失效解除。子代理、归档和 fork 继承范围不产生主线程通知；不支持的版本、持久化格式及未知必要记录显示读取异常。

两者首次启用及重新启用建立基线，不补发历史；按来源、会话和回合／请求身份认领，同一请求不因摘要变化重复发送。首次发送及每次重试前读取原生状态，恢复执行淘汰旧失败，已处理问题／审批清除重试正文；正常完成沿用既有有效期补发。无法复核时只使用既有有限重试，不猜测终态。

Hook 按 Provider 筛选待发队列，再限量、解密和领取；后台全量轮询不加来源筛选。失败数与待发数由单次分组查询汇总，状态口径、平台过滤及 Provider 顺序保持。扫描入库后仅保留 Grok 当轮全文快照；Claude、Grok Bot、Antigravity、DSH 继续按原路径复核来源。

通知首发、重试、Hook、CI 与 monitor 共用配置读取回调；服务端读取共享状态，文件入口复用按文件修改时间及长度失效的既有缓存。来源检查结束及每段发送前读取当前开关和目标，排队事件保留原目标指纹；身份解析不会将旧事件改投新目标。配置读取失败沿用错误处理。

原生通知待发正文用既有 SecretBox 加密；交接到 Bark 重试队列后清除原生正文副本，终态只留身份、结果和界面短摘要。保留期清理包括 unknown，保护待发、重试和当前游标仍需要的去重身份，Antigravity 游标推进后再淘汰旧身份。会话身份或游标未变时不更新该行；通知状态更新数为零时跳过三组历史同步 SQL，必要扫描状态照常持久化。

桌面 App 与 monitor 共用 `Config::load_runtime`。新环境在既有受限 env 文件中生成独立随机密钥；有效密钥继续使用，缺失私有密钥、损坏配置及解密异常明确报错并保留数据。旧开发密钥迁移已移除；不提供桌面加载失败的密钥回退。

事件识别与 Bark HTTP 投递分离。Codex 错误绑定规范主任务身份；普通自然语言反馈仅在确认终态后本地识别，monitor、Stop Hook 和完成入口共享 `assistant_question` 与持久化认领。Grok 使用主回合结束证据；Claude 轮询要求明确终态且无 pending tools，空完成内容回退到本回合最后一段助手回复。兼容警告继续可读，仅致命解析/身份错误禁止扫描。

原生 Codex 问题解析位于 `codex/user_input.rs`，统一同步 question、异步 title 和字符串/对象选项。tracker 以原始 turn/call/question 身份配对完整回复，处理明确取消与替换；工具确认、背景进度或正常完成不能关闭尚未回答的问题。异步 monitor 独立于列表状态，与 Hook、passive、Stop 共用发送前复核。启用基线和原子 per-call 认领保证首次跳过旧历史、部分回答不产生新身份，重启/TTL 过期不重发已确认调用。

Claude 轮询复用 provider monitor 游标；仅明确完成／失败、未解决 AskUserQuestion 或绑定 pending 主工具的权限请求符合条件，不从普通 pending tool 猜权限。解析器保留有效分支上先前回合的终态，后续新用户消息不清空旧完成，增量游标可补发未处理事件；一轮共享扫描或按目标读取完成投递复核，避免每项全库重扫。

claude.hooks 是仅限本机的 status／install／remove 操作。安装须设置页明确确认，App 安装不启用；配置位置沿用 Claude Provider，按固定程序和专属子命令识别受管条目，结构化合并并原子替换，保留用户／插件 hooks。安装、修复、移除幂等，格式错误或并发修改停止。Linux RPC 和 remote.invoke 不开放 hooks 配置写入。

受管 hook 使用 exec 程序与参数、async=true，handler 自行限制总时间五秒，不依赖异步 timeout 字段，不输出审批、模型上下文或唤醒指令。Stop 使用 last_assistant_message；Notification 的 permission_prompt 为授权，elicitation_dialog／elicitation_url_dialog／agent_needs_input 为回复；StopFailure 仅使用过滤后的错误类别。idle_prompt 不独立提醒，仅在尚未通知完成、未被启用基线覆盖且可核实终态时作为完成兜底。

hook 先进入现有加密队列，再至多进行一次有界发送，重试由 monitor 处理。prompt_id 与 transcript promptId 必须经原生映射验证，消息 uuid 不能代替 prompt ID；不明确时保留队列等待核验。去重绑定机器、Provider、会话、文件来源身份、prompt 和类别；仅 Claude hooks 及其同机轮询每回合每类认领一次，兼容旧投递键与成功记录。已确认回复／授权不因新回合撤销；完成还须具备对应回合终态，保留有效分支上较早回合的完成补发。StopFailure 为 API 错误结束的候选；Hook 首次处理、首次发送与每次重试均核验当前主回合、执行活动及终态，轮询失败使用相同规则。新回合、同回合恢复、成功或取消使旧失败过期并清空待投递正文；普通元数据追加和仍打开的进程不表示恢复执行，独立后台任务不阻挡主回合提醒。身份或终态未写完整时沿用有限重试。远端继续轮询；Codex 的逐问题身份、Stop、PreToolUse 与问题跟踪不变。

所有通知来源共用摘要格式：标题 Provider｜状态｜线程标题，状态仅完成／需要回复／需要授权／失败，缺失标题回退项目名再固定名称。正文取对应事件内容，去 Markdown、代码块、工具输出及内部元数据，最多四个非空行、300 个 Unicode 字符，截断加省略号，不附线程／回合 ID。每个新事件一条摘要；group 为既有前缀加 Provider，回复／授权 level=timeSensitive，其余 active，保留声音与合法显式 URL，不构造 AgentHub 深链。

Bark 保存和发送前共用 URL 校验。HTTPS 拒绝回环、未指定、链路本地、IPv4 私网、IPv6 ULA 及对应映射地址；拒绝数字 IP 别名、非法 authority 和 URL 凭据。保留普通 HTTPS 主机名与真实 HTTP 回环服务，不作 DNS 解析。无效配置保留原值并停止投递，不改用默认服务携带原密钥发送。

`bark` 复用支持 HTTP/2 的连接池并禁重定向，分类 DNS、连接、TLS、超时、读取、HTTP 及业务拒绝；已知永久 APNs 错误优先。原始异常仅用于内部分类，对外保留安全分类、状态码、耗时、次数及匿名关联，不输出 Key 或正文。

`notification_delivery` 将不可变请求加密入队，事务化认领／确认／事件回写；新事件保存单条摘要，旧在途请求仍按原内容和分段确认继续，不重新格式化。租约持有者才能确认，过期租约只恢复未确认内容。每条／旧分段最多 3 次，等待 15/60 秒并加 ±20% 抖动，首次投递后最长 10 分钟；尊重 Retry-After，超出剩余期限即终止。成功项不重发，稳定通知 ID 由机器实例、事件与分段组成；保留默认关闭、Provider 开关和启用基线，不清空数据库或凭据。

Hook 短等待，后台每轮最多领取 10 项；重试在来源验证前后检查当前配置、身份、开关、目标与事件有效性。正常追加以原始前缀和文件身份复核，不因长度变化取消。结束清除队列正文，同一 UI 事件更新状态。旧终态失败/未知不回放，新网络未知可有限重试且可能重复提醒。主动测试使用独立身份和同一队列，最终未受理以失败退出；受理不代表手机实收。

## 大小统计与资源预算

`session_storage` 只统计已验证归属的逻辑字节，不读正文或计算完整内容哈希。Codex 统计 rollout，Claude 统计主 JSONL 及同项目内确认身份的同名会话目录，Grok 统计专属目录；独立线程、共享数据、工作目录和外部附件不计入，不跟随符号链接。

Antigravity／DSH 列表在筛选、排序与截取后才请求大小；详情与最终删除确认继续按需统计。DSH 附件读取、Antigravity 删除预览摘要及清理允许集、Claude／Antigravity／DSH 失败预览摘要不再统计大小。五个平台摘要的可选 `storageSize` 使用 bytes/scope/status，未知不等于零。普通追加可复用身份缓存；截断、替换和归属改变立即失效。K/M/G 按 1024 换算，颜色按准确字节：小于 100MiB 绿色，100MiB 至不足 300MiB 琥珀色，至少 300MiB 红色。未知灰色，部分结果按确认下限分色并保留 ≥，提示准确字节和不完整状态。

固定预算由 Rust `resource_limits` 和 WebUI `resourceBudgets` 命名管理，不是设置项。每个 JSONL/普通读取缓存默认最多 64 个来源，以下字节数独立计重：

| 缓存或工作 | 当前预算 |
| --- | --- |
| Codex 正文 / 活动扫描 / session index | 128 / 8 / 16MiB |
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
| 阻塞读取 / 凭据工作 | 按可用并行度限制为 2–4 个 / 1 个 |
| WebView 指令切片 / 上下文投影 / Markdown 树 | 各 512 项、各 8MiB，分别计重 |

Antigravity／DSH 时间线随历史快照生成，按实际分配容量计入现有缓存预算；revision 在快照生成时按实际内容计算，不新增增量变更日志，保留保守旧页重读。Antigravity 在线缓存修订只依据会话 ID 与摘要，终态判断复用来源层结果，身份、解析或提前空页错误明确返回且不进入缓存。分页以首批总数为目标，末页允许因原生追加超出目标，不设 50,000 步硬上限；仅服务不可达或无在线内容时回退离线。

Claude 摘要与 Codex 问答缓存接入既有 5 分钟空闲释放入口，不新增定时器；释放缓存引用后再次读取按原数据重建。

DSH／Grok 通知派生条目只在内容变化时计重并维护总量，Grok JSONL 缓存使用共享不可变数组。扩大 DSH 缓存覆盖可能增加驻留内存。上述缓存各自计重，**不是进程内存硬上界**：正在引用的 Arc 快照可能暂时不可淘汰，解析、响应序列化、恢复的语法树、DOM 与并发请求有额外内存；App、helper、远程服务和 WebView 又是独立进程。目录条目按数目而非完整堆内存计重，隐藏扫描预算见管理章节。超大快照可返回但不长期保留；不能以改变成功读取行为的方式把配置预算解释成全局 RSS 限额。

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

v2.1.1 支持新安装及已由 v2.1.0 完成迁移的配置。更早版本先升级至 v2.1.0；早于 schema 3 的数据库须先经过 v2.0.10。协议保持 2、数据库 schema 保持 3。旧 thread_followups 表和历史数据保留，仅删除无生产调用的旧操作方法；当前加密发送队列保持原行为。

新库在现有迁移锁和事务内建立最新结构，schema 3 打开时不再运行旧开发密钥重加密、明文正文迁移或抽样解密预检。实际读取密文时仍验证 GCM tag。配置加载保留真实解析错误，缺失或空运行密钥在现有 env 文件中以 0600 原子写入，替换空条目；有效 Base64、Base64URL 与 hex 密钥保留。独立 Keychain 加密的 Codex 发送队列不参与通知密钥处理。

Claude reader 一次性升级查询已删除，通知基线仅首次建立或启停变化时写入，重新启用建立新基线。保留当前通知队列、去重、认领、重试次数、成功分段、任务、审计及其他业务数据；不打开或迁移原生会话数据库，也不执行额外空间回收。

## 构建、CI 与发布

根 Cargo workspace 构建共享核心和 Linux webd；`src-tauri` 独立 workspace 避免服务端构建引入桌面平台依赖，路径依赖共享核心。两份锁文件独立解析，Tauri 自身允许额外主版本；共享依赖约束、实际选择和最低 Rust 要求需一起检查。开发与 CI 使用 rust-toolchain.toml 声明的 stable 工具链；两个 workspace 保留一致的最低 Rust 声明，允许工具链独立更新。`check-workspaces.py` 核对两侧最低声明，并精确比较共同直接依赖的声明与锁定结果；当前允许的 `toml` 0.8 解析差异是根 workspace 0.8.23、桌面 0.8.2。Tauri 额外的 dirs、reqwest、toml 主版本属于独立依赖链，不能据此强行统一。

升级共享依赖时先确认两侧 manifest、feature 和实际依赖链，再定向更新两份锁文件、运行两侧 Rust 检查和三项 CI；不得为消除合法多版本而全量更新。产品版本以根 Cargo workspace 为准，JSON/Tauri manifest 和两份 lock 的自身 package 必须精确一致；契约 schemaVersion 与 API 协议单独校验。

CI 保留 Frontend checks、Backend checks、macOS Tauri checks。第三方 Action 固定 SHA，权限按作业收敛；Frontend/Backend/Tauri 超时为 30/45/60 分钟。Cargo test、clippy 和正式构建均使用 --locked，包括 Tauri 传递的 Cargo 参数。缓存区分平台、架构、工具版本、锁文件与构建参数，覆盖 Cargo、pnpm、固定版本 cargo-audit 和 Playwright 浏览器；Linux 浏览器系统依赖仍正常安装。cargo audit/pnpm audit 保留 JSON artifact：高危、严重及无法判级的活动漏洞阻断，低/中危和 RustSec informational 警告进入 summary；扫描失败、数据库获取失败或报告不完整也阻断。定向修复必须审阅两份锁文件，不自动批量升级。交互性能预算使用生产构建、可见 macOS 窗口、串行 Chromium/WebKit 和关闭 trace 的命令；单一阈值与 JSON 测量结果由既有 `@desktop-performance` 测试提供，复现见 README。

Release guard 核对 tag/版本、契约、隐私和同提交三项成功 CI。guard/Linux/macOS/publish 超时为 10/45/60/15 分钟。发布仅有 macOS ARM64、Linux x86_64 webd 和汇总阶段，共八项资产：DMG/校验文件、updater 归档/签名、`latest.json`、`agenthub-webd-linux-x86_64.tar.gz`/校验文件/分离签名。updater 只映射 `darwin-aarch64`，文件名的 darwin-arm64 是同一架构；webd 不能成为桌面 updater 条目。

macOS App 在 Tauri 生成 DMG 和 updater 归档前完成代码签名，默认 ad-hoc；原有 `MACOS_CODESIGN_IDENTITY` 覆盖传入打包配置，Tauri 的 `APPLE_SIGNING_IDENTITY` 优先级保持。打包检查完整 App 的 deep/strict 签名，不能仅重签临时归档副本。代码签名与 updater Minisign 独立，ad-hoc 不代表 Developer ID 或 Apple 公证。桌面 updater 只启用 rustls，macOS tar.gz 支持由其平台依赖保留，不引入未发布的 Windows ZIP 更新特性。

正式安装复用同提交成品，先远程服务后 App；部署输入不进入仓库。Linux 归档用现有 Tauri 签名工具生成带版本 trusted comment 的分离签名，私钥仅从 GitHub Secrets 环境提供。在线下载先校验发布 SHA-256，再用预装 Minisign 与固定公钥验签，失败不解包；指定版本需与已签名版本相同，解包后核对布局、ELF x86_64 平台及程序版本。旧 updater 首次升级由可信客户端先验签；显式本地 --archive/--binary 运维方式保留。部署另比对本地与远端文件 SHA-256。解包拒绝路径穿越、链接、特殊文件及重复成员；目录与可执行文件统一 0755，普通文件 0644，不继承特殊位或可写权限。程序与部署文件通过同文件系统临时文件原子替换，普通安装失败恢复本轮替换项，不回滚业务数据库或原生会话；在线更新仍使用既有异步重启，成功调度不等于新进程已启动，也不提供崩溃自动恢复。安装成功/启动检查与用户功能验收分开报告。维护六份当前文档并使用普通增量提交，历史由 Git 和 Release 保留。
