# AgentHub 界面设计规范（维护者文档）

本文记录稳定的布局、交互和可访问性约束，不是面向用户的产品说明。

AgentHub（底层包标识 `com.lich13.agenthub`）follows a quiet, refined reading workspace styled after the ChatGPT macOS Desktop App: a three-tier canonical surface hierarchy (`--bg-rail`, `--bg-surface`, `--bg-stage`), a clean sidebar task list and one centered readable conversation column. Only local, unarchived Codex main threads have a compact floating collaboration composer after connecting to the native desktop. Other history surfaces and subagent details remain read-only, with no duplicate status cards.

## Layout and interaction

- Navigation contains Codex, Claude Code, Grok Build, Antigravity, DeepSeek Harness, Probe and settings.
- Desktop uses a compact navigation rail (collapsible to `48px`), a `288px` task list and a centered message column up to `768px`. At `767px` and below, each provider keeps one mounted list/detail/back flow and a 7-column mobile tab bar with ellipsis-safe labels.
- Search, provider changes and status changes clear batch selection. The list shows the selected count and limits batches to 100 explicit keys. Menus support keyboard navigation and restore focus after dialogs.
- Running Codex, Claude Code and Grok sessions use the same spinner, `aria-label` and reduced-motion behavior. Unknown state remains text.
- Codex, Claude Code and Grok pair native calls/results and group adjacent tool rows at the original call position. Text, Plans, questions and history markers end a group. A muted icon row names the provider and shows activity/failure counts even when closed; each expanded row shows its tool name, clipped input preview and state. Groups have a bounded scrolling list and lazily rendered details. Historical tools are never replaced by one global summary. Both levels use native `<details>`: completed groups/rows start closed, active and failed groups/rows open, and user toggle state survives polling and appended events.
- Visible assistant replies in all three providers expose a compact copy button that copies Markdown after structured memory metadata is removed. Codex and Grok plan cards add copy and title-named Markdown download actions. Provider list rows enter inline rename on double-click; Enter or blur saves and Escape cancels, while activity-protected or archived rows remain read-only.
- AGENTS.md tools and body sections default to closed even when running or failed. Instruction-only outer groups also stay closed; mixed groups retain normal state rules. Summaries show the basename, line/byte counts for body sections and activity status without private paths. Native disclosures preserve user choices across polling, appended text, theme changes and mobile navigation. Open file bodies have their own bounded scroll area.
- Remove `oai-mem-citation`, `citation_entries`, `rollout_ids` and standalone `memory_citation` metadata before rendering, copying or downloading. Keep fenced/inline code examples, ordinary memory prose and native session files intact. Plans use the cleaned heading for download names.
- Probe settings show notification and error-monitor controls only. Goal recovery controls are removed. Settings combine update, archive cleanup, hidden-thread cleanup and job history under `更新与维护`; the retired system status panel and polling do not exist.
- Markdown file links copy their source path for remote machines, while local macOS threads reveal a resolved path in Finder and offer a neighboring copy action. Codex ordinary final replies that explicitly wait for user feedback use the existing Bark reply-needed event. Native synchronous/asynchronous questions use the same event while unanswered, with their title, question text and options. An asynchronous acknowledgement and background progress do not dismiss a question; native matched answers update only the corresponding items. Claude completion requires terminal evidence and no pending tools; an empty result falls back to the current turn's last assistant text. Compatibility warnings remain readable without disabling safe management; only fatal parse or identity errors suppress notification scanning.
- The reading column has a 20px timeline rail outside the document flow on desktop and mobile. Its complete user-message index is independent of loaded or folded body pages. Only a bounded range of marks mounts; scrolling and Home/End/arrow keys reach every entry. A single hover/focus preview sizes to its text, capped at 280px on desktop and 240px on mobile, without a duplicate browser tooltip. Selecting an unloaded entry reads one bounded window; older/newer paging and 返回最新 preserve the reading context. Assistant replies, plans and activities retain search anchors without rail marks. `⌘F` on macOS and `Ctrl+F` elsewhere opens machine-scoped 当前线程/当前 Provider search.

## 本机 Codex 协作区

本机 Codex 列表提供新建线程，先用原生目录选择器选工作目录；空线程由 Codex 创建并打开。正文下方仅保留一个紧凑协作区，连接失败在原处说明并可在 Codex 中打开；不显示调试身份或附件内部路径；不显示 build 常驻提示；协议不兼容时仅说明该操作不可用并提供原生入口，owner/失步错误提示打开同一线程或重连。远程、归档、子线程和其他 Provider 不显示发送入口。

打开和新建两个明确入口在桌面打开后自动连接，普通浏览保持阅读。重连、重新同步与 owner 待确认在连接行分别说明；owner 改变只恢复阅读，使用现有“连接”按钮确认。只读时暂停及结果待核对项仍可见，队列操作禁用。复制诊断在连接失败时仍可用，正文、路径及原生错误原文不进入剪贴板。

输入区使用紧凑圆角容器、左侧“＋”添加菜单、右侧模型摘要与圆形发送按钮；模型/思考强度/模式在摘要内展开，保留键盘可达与 Escape 关闭。诊断使用带标签的图标按钮，复制后就地反馈。原生 Goal 显示一行目标/状态，可展开目标全文，修改仅跳转原生桌面。

输入区支持文件/图片及原生技能、应用、已启用插件引用，输入 `$` / `@` 时按当前输入位置展示过滤目录，上下键移动、Enter/Tab 选取、Escape 关闭，组合输入不触发选择。引用同时显示可读占位文字和可移除名称标签，编辑占位文字撤销绑定，移除标签同步删除占位文字。每页最多 100 项；超时和部分目录失败可重试且保留其他来源的结果。模型、思考强度和默认/Plan 模式采用原生可用目录。⌘/Ctrl+Enter 发送，输入法组合过程中不触发；运行时区分排队、补充和停止。等待队列可取消，暂停后由用户继续；结果不明保留记录，只有明确核对后才能移除，不自动重发。

协作状态用一行显示，待处理请求优先、队列数同行，运行复用 RunningIndicator。文件可拖入输入区，也可从 Finder、完整路径文本和截图粘贴。附件标签只显示名称；整段路径无法识别为有效文件集合时保留原文、换行及编辑历史。普通文本沿用原生粘贴；发送、隐藏或换线程后不接收旧附件结果，界面只持有名称和 handle。

提交等待显示“正在提交到 Codex…”。模型、模式、技能、应用、插件各自显示加载、空状态和可重试错误，失败不清空其他结果。设置错误就地显示，选择值以原生视图为准；成功或重连后清除过期错误，结果不明仍由队列保留。附件错误应区分授权过期、不可读、数量、大小、格式、文件变化与存储容量，使用固定安全文案。失败保留草稿；重新选择或移除附件清除对应操作提示，原生错误与队列结果不明状态保留。

审批内容按需展开且高度受限；命令等宽显示，文件 diff 按文件折叠且展开才挂载，原生文件路径仅供只读审阅，未知字段折叠显示；命令、文件变化和权限展示完整可核对内容后才允许确认。仅后端声明支持时，命令/文件审批显示“允许本次会话”，权限仍限本回合。问答不预选答案，密码型回答不自动补全；不支持的请求提供在 Codex 中处理的入口。连接和请求失败保留草稿，迟到响应不得切换线程或恢复旧审批。既有阅读、历史分页、工具折叠、搜索定位与滚动锚点规则继续适用。

## User messages and attachments

User messages use a shared right-aligned ChatGPT-style neutral pill (`border-radius: 20px`) with normal-weight system text and preserved whitespace. Markdown characters are literal in user requests; assistant Markdown and Plan rendering keep their existing rules. User role labels are omitted. The bubble stays within the reading column and narrows on mobile.

Confirmed native question-reply envelopes become one bubble per answer. A muted one-line question can expand by pointer or keyboard; the answer remains fully visible and literal. The copy button beside each bubble copies only its answer and reports success/failure locally. Native IDs and JSON/XML transport syntax are not displayed.

Each user message has at most one initially closed “上下文” disclosure. AGENTS.md, complete environment/runtime envelopes and native attachment wrappers share this row, in original order when opened. Collapsed bodies are not mounted. Render attachments first, context second and literal requests last; context-only messages have neither an empty blue bubble nor a timeline mark. Preserve explicit choices by machine, session and message across refresh, history paging and navigation. Both My request: and My request for Codex: preserve the original request line breaks. Code, quoted examples, ordinary XML and incomplete wrappers remain literal. Search includes requests, questions/answers, attachment names and AGENTS.md instructions; AGENTS.md hits open the same context row, while automatic fields and structured memory/Page metadata are excluded.

Image attachments sit above the bubble in reserved 96px square thumbnails, with a modal image preview, Escape dismissal and focus restoration. Loading and failed states retain their footprint. File cards show basenames and reuse path controls. Only confirmed native attachment envelopes are removed; unknown text and literal examples are preserved. Image-only messages have no empty bubble.

## Visual tokens

Light: `--bg-rail: #f8f9fb`, `--bg-surface: #f3f5f8`, `--bg-stage: #ffffff`, `--bg-elevated: #ffffff`, `--user-bubble: #f3f5f8`, `--bg-selected: #e4e8ef`, `--text-primary: #0f172a`, `--text-muted: #64748b`, `--border-subtle: #e2e8f0`.

Dark: `--bg-rail: #12151c`, `--bg-surface: #171b24`, `--bg-stage: #1b202b`, `--bg-elevated: #222836`, `--user-bubble: #262d3d`, `--bg-selected: #283042`, `--text-primary: #f1f5f9`, `--text-muted: #94a3b8`, `--border-subtle: #283040`.

Use system fonts, stable control sizes and at least 4.5:1 text contrast. Focus indicators and essential borders meet 3:1. Long titles and paths wrap; only code and tables scroll horizontally.

## Verification

Browser checks cover both themes, 1440x900, 1280x820 and mobile widths, reduced motion, spinner state, mixed Grok tool grouping, keyboard expansion, long-session scroll containment, Plan export and provider empty states. The installed Tauri App is the reference desktop surface; remote checks use the same App through the API and explicit deployment inputs.

## Machine scope

Place a compact 本机/远程机器 selector below the navigation brand, with an icon in the collapsed rail and an accessible native selector. Keep the current target visible; errors never trigger a silent fallback. Disable switching during writes. Settings use 更新与维护 and 远程连接; no login, system-status or security panel remains. Show only the selected machine’s update panel: 本机 App 更新 or 远程服务更新. Use one compact row with 当前版本, 检查更新 and, only for a confirmed new version, 更新至 followed by its version. Normalize the version prefix; the server also offers 清理更新备份. Show 已是最新版本 only as feedback from a successful check, and keep progress/errors beside the controls.

Remote connection inputs are HTTPS 地址 and 管理员 API Key, followed by 验证并保存 and 移除连接. Do not display a saved key. Connection errors and save results stay near the controls. Machine changes clear unexecuted confirmations and selection; stable disclosure identities include the machine. All previous conversation layout, themes, keyboard and reduced-motion rules remain.

Plan export uses native local file saving, including remote sessions. Show success only after UTF-8 content is written; never overwrite an existing download.

Claude Code keeps a fixed navigation entry on both machine targets. Missing native data shows an empty state. Reuse provider lists, reply/Plan actions, attachment previews and stable tool disclosures; paged history prepends without moving the visible anchor. Compatibility warnings remain available in a closed disclosure; fatal reading errors remain visible. Only fatal record, identity, path, permission or uncertain ownership errors block the affected management action.

## 线程菜单、清理与窗口

线程项右键立即在指针处打开共享操作菜单，不改变当前详情。省略号、Shift+F10 和菜单键使用同一动作列表；Escape、外部点击和列表滚动关闭，菜单在视口边缘自动避让。复制、改名、归档和永久删除都有明确目标，批量模式不混入右键操作。

五个 Provider 通过共享选择逻辑处理行点击：⌘ 进入多选并切换目标，普通点击在多选中切换，否则打开；Shift 按可见顺序追加连续区间，连续 Shift 保留锚点，无有效锚点仅选当前项。键盘生成的点击忽略修饰键，Ctrl 点击仍打开菜单。保留全部已有选择；归档、恢复按 100 项分批执行；机器、Provider、筛选和退出多选清除选择及锚点，轮询不自动选择新项。

Codex 列表菜单不提供重复的“复制答复”，会话头部保留。Claude 菜单统一提供改名、复制线程 ID、复制文件路径、复制恢复命令、在 Finder 中显示和删除；改名理由独立显示，删除不按读取或活动状态禁用。远端 Finder 明确显示不可用原因，恢复命令仅复制，Claude 不提供归档。Antigravity 与 DeepSeek Harness 提供列表底部“已归档”抽屉、搜索、大小徽标与多选批量归档／恢复／永久删除；列表行右键／省略号菜单与会话头部菜单均提供改名、归档／恢复（取消归档）、复制 ID、复制路径和永久删除（头部另含复制答复），均不提供“复制恢复命令”。连接尚未升级的远端服务时，展示升级提示而非协议报错。

列表项与外层改名容器均填满可用宽度，保留 min-width: 0；长短标题、运行 spinner、复选框和选中背景的左右边界一致。

永久删除只保留一次确认，展示已定位的目标集合和定位错误。清理结束后显示已删除、失败及残留；没有候选时显示“无可清理项目”。执行不因文件变化或活动登记重新要求确认。

Codex、Antigravity 与 DSH 删除确认展示本次选中的后代、文件与总大小，执行不纳入之后新增的后代。Claude 展示主文件及专属目录完整路径集合。结果以整棵树或独立会话为单位；部分失败展示已删除 ID、残留类别和可行的下一步，详情及缓存按实际删除 ID 清理。残留路径只出现在操作结果中。

Grok 的列表菜单、详情和批量操作共用直接删除，不按活动、摘要完整性或旧资格字段禁用。Claude 与 Grok 空会话显示“暂无对话”。

隐藏扫描自动顺序获取结果，状态行显示准备、扫描、完成、停止或失败以及已处理／总数。候选和逐项结果每页最多 100 行；全部完成后才启用清理确认。停止保留已显示结果并禁用清理，重新扫描清空旧结果。切换机器或离开页面停止请求、取消扫描并丢弃迟到响应；旧服务仅显示单次扫描状态，不展示虚假进度或停止按钮。

主窗口隐藏创建，初始化时一次最大化，必要时一次工作区回退，然后显示。Dock 重开、最小化恢复和机器切换不重新调整尺寸或位置；不切换 macOS 独立全屏 Space。Pi 不再显示导航或设置项。

## 紧凑阅读与子智能体

保留 780px 阅读列上限，标题栏约 56px，正文 15px、行高 1.65，普通块间距 12px。用户新指令保留额外分隔。正文、工具摘要和子智能体活动文字共用左侧基线，图标使用固定侧栏位置。工具摘要约 30px 高，按读取、搜索、命令和集成生成中文文案；长名称单行省略，展开内容独立滚动。

助手正文不重复显示 Provider 标签。桌面复制按钮位于正文侧边，悬停或键盘聚焦时出现，触屏保持可见；成功和失败反馈不撑高消息。线程 ID 保留复制菜单入口。

Codex 子智能体行独立于工具组，显示任务名称和自身回合状态，缺少已验证关联时显示不可用原因。可用阅读区至少 960px 时详情并排显示，宽约 420px；较窄时覆盖显示并约束键盘焦点。支持 Escape、关闭、焦点返回和嵌套返回；主线程保持挂载与滚动位置。面板独立分页、滚动和折叠，机器、Provider 或主线程变化时关闭并丢弃旧响应。面板只读，不提供子智能体执行或管理动作。

## 远程连接反馈

钥匙串读取在后台串行进行，不阻塞界面。超过 15 秒时在当前操作附近显示错误，不能把超时当作成功或自动切换到本机。保存与移除连接等待已开始的事务完成；操作期间保持切换保护。连接身份变化后，旧读取不得发起业务操作或覆盖新机器结果。

## 原生活动与直属汇总

子智能体历史活动显示“开始工作”“已完成”“已中断”或“已交互”，与当前运行状态分离，刷新不改写过去的文案。任务名优先于随机昵称，活动行、列表和详情标题保持一致。

只在线程详情标题栏下显示紧凑“子智能体”汇总：运行中、已完成及非零异常数量。确认无直属子智能体时不占位；未知和不可用状态保留在对应条目。点击汇总打开右侧列表，运行中的排在前面，点击进入详情并可返回列表；子线程详情使用自己的直属集合。列表、详情、嵌套返回与 Escape 关闭保留阅读位置和焦点。

完整内部页面包装默认不显示，也不进入复制、导出和搜索；纯包装消息不留空气泡或用户指令短线。普通正文、代码与引用示例保留，未确认结构不误删。

## 说明精简

子智能体汇总和列表移除通用完整性说明。清理面板移除标题下的重复描述，状态标记保持右对齐；具体错误和逐项处理结果仍就地展示。

## 完整指令与通知

完整原生 AGENTS.md 包装默认关闭，包括结束标签与后续环境标签同行的记录。摘要按完整片段计算行数和字节数，展开使用受限高度的原文区域，普通请求独立显示。

Probe 默认以「通知配置」控制台作为首屏，直接呈现探针与错误监控开关、五大 Agent 平台级通知一键开关、Bark 通道配置与测试推送、五平台统一 Hooks／Host 管理矩阵（Codex、Claude Code、Grok Build、Antigravity 受管 Hooks 安装／修复／移除与 DeepSeek Harness 原生 `.lock` + `.zstd` 流直连状态）、五平台实时态势与最近事件流。远端以“仅限本机”或对应远端能力显示。安装、修复和移除均需明确确认并保留用户其他 hooks；App 安装不自动启用 hooks。退休通道不保留卡片或占位。

## Bark 状态与测试反馈

同一事件卡片显示最新投递结果，重试成功不新建卡片。“等待重试”显示下次时间；“重试中”显示当前次数；确认成功显示“Bark 已受理”；最终拒绝、结果不明和停止投递分别显示“发送失败”“结果未确认”“已跳过”。错误仅使用安全中文分类，旧队列分段通知继续显示已确认进度。

新事件标题为 `Provider｜完成／需要回复／需要授权／失败｜线程标题`；缺失标题回退项目名和固定名称。正文最多四个非空行、300 个 Unicode 字符，移除 Markdown、代码、工具输出与内部元数据，截断加省略号。Claude hooks 及同机轮询同回合每类提醒一次，完成后没有单独空闲提醒；Codex 保留逐问题提醒。

Bark 地址保存和投递共用安全校验；无效配置在既有错误位置显示失败，不新增入口，也不悄悄改用默认服务。HTTPS 服务与 HTTP 回环测试保持既有操作方式。

测试标题统一为“AgentHub 推送测试”，正文分别为“来自本机的测试通知。”或“来自远程机器的测试通知。”。任务入队不显示发送成功，等待期间保留运行状态；受理和失败结果就地呈现。旧程序名称、配置导入入口和无用诊断字段不进入产品输出。

## 历史与大小

- Codex 主线程、Claude、Grok 和 Codex 子智能体共用顶部加载逻辑。首次完成底部定位后，向上阅读至距顶部 200px 时加载一页；搜索与滚动使用同一请求协调，不并发请求相同游标。
- 搜索命中折叠历史时，仅显示命中的旧消息或问答；命中工具或指令时展开对应内容并定位原生身份。保留其他手动折叠选择，再次搜索已关闭的目标会重新展开。
- 顶部只在加载中显示紧凑进度；失败显示“加载失败，重试”，停止自动重试。历史耗尽时不保留占位。轮询、追加、工具展开及加载中继续滚动都应保留可见消息的像素偏移，不拉回底部。
- 大小位于原状态行右侧，使用等宽数字和固定数值位置，不增加卡片高度；保持标题省略、批量复选框和 spinner。K／M／G 按 1024 换算，最多一位小数，去除 .0。颜色独立按实际字节分级：小于 100MiB 为绿色，100MiB 至不足 300MiB 为琥珀色，300MiB 及以上为红色；浅暗主题保持可辨识。
- 零字节显示 0K，不足 1K 显示 <1K，未知或后台统计中显示灰色 —，部分结果使用 ≥。悬停或键盘聚焦显示准确字节数、文件／目录口径及不完整状态，不把下限当作完整大小。
- 历史和大小均沿用机器隔离。Codex 子智能体面板独立滚动，分页不重置父线程位置或手动折叠状态。

工作区切换按需加载内容，列表首击立即选中，双击仍进入原有改名流程。隐藏窗口暂停界面轮询；恢复后继续使用现有刷新时效。折叠正文仅在展开时挂载，复制、搜索定位和用户明确的折叠选择不能因此丢失。时间线滚动复用位置缓存，图片、工具或窗口布局变化后重新测量。

## Claude 自动内容

完整原生命令包装显示为斜杠命令和原始参数，真实请求保持换行、空格和附件。命令回显、任务通知、提醒和自动上下文不显示用户气泡、不产生用户指令标记，也不创建用户回合。相邻自动记录按原位置合并为默认关闭的“系统活动”；指令与环境归入“上下文”，压缩摘要默认关闭。首条输入前的设置与快照同样合并。搜索通过原生事件身份展开对应内容，分页与轮询复用已有展开选择；非阻断兼容提示置于“读取提示”折叠中。
