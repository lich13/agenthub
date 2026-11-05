# AgentHub（AgentHub）当前状态（维护者）

## 当前版本

当前维护源码版本为 **2.0.10**，实现基线为 `ce5d956 / v2.0.9`。本版修复已确认的删除范围、DSH 改名日志完整性、通知正文与密钥迁移、保留期去重问题；复用 Antigravity／DSH 未变化历史，淘汰旧摘要缓存，减少空闲游标写入与无变化通知同步。两份锁文件定向更新至 `anyhow 1.0.103`、`tokio 1.53.3`，同步过时维护规则。API 协议保持 2，现有 RPC、用户操作流程和视觉设计保持不变。

历史实现与交付由 Git 和 [Release](https://github.com/lich13/agenthub/releases) 保留。完整回归、跨平台构建和既有性能检查以发布提交对应的 [CI](https://github.com/lich13/agenthub/actions/workflows/ci.yml) 与自包含 [Release](https://github.com/lich13/agenthub/actions/workflows/release.yml) 为准。

## 已实现范围

| 工作块 | 当前行为 | 核心提交 |
|---|---|---|
| 固定删除范围与 DSH 日志完整性 | Codex／Antigravity／DSH 只执行确认的根与后代 ID，保留归属校验；DSH 改名补齐合法末行分隔，不完整 JSON／Zstandard 帧报错且保持原文件。 | `v2.0.10` |
| 通知加密、密钥与保留期迁移 | schema 3 加密原生待发正文并清除终态副本，保留认领／重试／成功分段；独立随机运行密钥与已知旧开发密钥迁移只涉及通知数据；unknown 按保留期清理，当前游标需要的去重身份保留到推进。 | `v2.0.10` |
| 读取与空闲轮询资源优化 | Antigravity／DSH 共享 32MiB／64 项不可变历史及时间线缓存；DSH 摘要使用 8MiB／2000 项有界淘汰；来源变化失效、运行中读取、附件刷新；未变游标及无变化历史同步不重复写入／扫描。 | `v2.0.10` |
| Hook 目录边界恢复、helper 原子替换、ETXTBSY 重试与五大 Agent 标题统一 | `claude_hook::context` 恢复按 `ClaudePaths::default_for_user()` 严格校验路径边界并移除 `parse_with_paths`；`sync_agenthub_webd_helper_file` 改用同目录临时文件 `rename` 原子替换避免 macOS `SIGKILL 137`；子进程 `spawn` 针对 `ETXTBSY` 自动重试；五平台自检持有 `CONFIG_ENV_LOCK` 注入 `CLAUDE_CONFIG_DIR`／`GROK_HOME`／`DSH_HOME` 且 Phase 4 使用固定时间戳；统一五大 Agent 列表标题 `15px` 字号并移除 5 处说明小字；旧通知分组名（`"NexusHub"`／`"agenthub"`）按方案 A 不再自动规范化。 | `ee12316`, `60a1178`, `e2dd498` |
| 配置加载自迁移修复、轮询路径去诊断开销与测试／可见性收口 | 移除 `config.rs` 默认分组 `"AgentHub"` 自迁移重写并断言 `ino` 不变；`evaluate_trajectory(want_diag)` 在轮询路径跳过诊断构造与 `collect_turn_pending_work`；补齐跨回合挂起任务收敛补发断言、Claude Hook 入队自检，恢复 `notification_snapshots` 为 `pub(crate)`。 | `e518e97`, `a2a7582`, `0d77fb5` |
| Probe 宿主卡片排布修复、Antigravity 判定／Hook 收口与全量底层命名统一 | 修复 `.probe-host-card-head` 换行与 `.probe-host-card-copy` 宽度挤压并收紧 5 卡几何 E2E；`evaluate_trajectory` 合并 `snapshot_with_context` 与 `diagnose_trajectory`、修复跨回合未决后台任务游标越界、`antigravity-hook` 单次 `allow` 原子门控与分离后台扫描；补齐 `unrecorded_at_cursor` 与五平台真实解析器自检及脱敏回归夹具；全仓 crate、二进制、路径、契约与部署脚本统一为 `AgentHub`／`agenthub`。 | `44027ce`, `6da0e4e`, `dcac3b9`, `245a4d7` |
| Antigravity／Grok Hook 契约修复与后端 `config_path` 下发 | Antigravity 改用顶层 `"agenthub"` 契约（仅 `Stop` 事件）并清理污染顶层 `"hooks"` 键，`antigravity-hook` 恒向 stdout 输出 `{"decision":"allow"}`；Grok 清理遗留 `claude-hook` 条目；后端 Hook 状态接口直接返回真实宿主 `config_path`。 | `b1bd231` |
| 缓存合并去重、清理／目录枚举收敛与 Probe 扫描减半 | 合并 `core`／`webd` Antigravity 端点缓存并修复存活进程瞬时失败负缓存；移除缓存 `#[cfg(not(test))]` 门禁、补齐环境隔离键与 Grok 1s 进程缓存并全部纳入 `evict_idle_detail_caches`；`cleanup.rs` `run_scope_targets` 与 `tree.rs` `visit_codex_tree_files` 消除重复；DSH `load_all_events` 降至 `O(events)`，`aggregate_active_sessions` 消除双倍扫描。 | `b5ba008`, `8c2b206` |
| 多轨星枢图标重设计、`styles.css` 深度重写与全工作区统一重构 | 重绘无平台数量限制的多轨星枢图标；`styles.css` 彻底重写至 566 行纯净令牌与 `ws-*` 原语；提取 `useCopyFeedback`、`WorkspaceSplitShell` 与五平台统一 `AgentHookHostCard`，移除 Probe 冗余胶囊行与子分栏，完成七大工作区深度重构。 | `f3740a5` |
| DSH 完成状态修复与五平台热路径缓存 | 移除 `dsh/sessions.rs` 中 `\|\| locked` 误判分支，对齐 Seeded 会话跳过边界与 `turn/end` 工具状态收口；新增 `DSH_FOLD_CACHE`、Antigravity 端点缓存、Claude 活动快照 2s 缓存与 `canonicalize` 外提、`probe.status` 3s 会话快照缓存。 | `62ade32` |
| 统一清理引擎瘦身、`O(K)` 校验与全测试沙箱化 | 提炼 `execute_verified_scope_items` 与空会话共享清理管线；`execute_delete_hidden` 改用 `O(K)` 定向校验，`dry_run_single_scope` 改用 `estimate_codex_trees_bytes` 单次目录扫描；全部 `Config::default()` 测试入口统一替换为 `TempDir` 沙箱。 | `1ef92f3` |
| Probe 首屏控制台与五平台统一 Hooks／Host 矩阵 | Probe 默认首屏直接展示主控开关、五大平台通知开关、Bark 配置、五平台统一 Hooks／Host 卡片、实时态势与最近事件；新增 Grok Build 与 Antigravity 受管 Hooks 安装／修复／移除与实时通知接入；统一前端可见性感知轮询。 | `b8f2211` |
| 三层 ChatGPT macOS 表面重写、工作区 Hooks 收敛与 AgentHub 品牌焕新 | 落地 `--bg-rail`／`--bg-list`／`--bg-stage` 纯正中性灰阶令牌体系与共享 `useProviderWorkspace` Hooks；更名为 `AgentHub`（`AgentHub.app`）并换装全套高清钛黑五节点星枢图标。 | `39a4007` |
| 全历史隐私脱敏与测试环境隔离 | 重写全量 Git 历史对象与 `v1.0.0`–`v1.7.0` 标签、脱敏历史 Release 说明并移除跟踪缓存索引；`agenthub-webd` 集成测试统一注入独立临时 `HOME`／`XDG_*`。 | `1545b61` |
| 统一清理允许集交集与孤儿宽限收紧 | `cleanup.unifiedExecute` 强制要求请求项落入最新 `unifiedDryRun` 允许集并与重算候选求交集，越界或运行中目标首错即停；60s 宽限仅对已确认 `epoch` 归属目录生效。 | `3dc983a` |
| 原生扫描缓存、内存预算收敛与 Antigravity／去重边界修复 | Codex／Claude／Grok／Antigravity／DSH 元数据扫描按 `(mtime, size)` 增量复用，详情缓存空闲 TTL 收敛至 300s（5 分钟）并 `shrink_to_fit`；修复 Antigravity 完成时间回退、同执行器多次提问去重、空 `TASK_BOUNDARY` 与 SQLite `INSERT OR IGNORE` 原子认领。 | `436ee88` |
| 平台一键开关、Hooks 卸载与五大 Agent 能力对齐 | Probe 新增五大 Agent 平台级一键启停与 Claude／Codex Hooks 一键移除；对齐五平台行菜单、本机 Finder 定位、复制答复与 `⌘K` 全平台搜索。 | `b9a0812` |
| 多远端机器管理与 Keychain 平滑迁移 | 支持保存、切换、编辑与删除多台命名远端机器，每个 `machineId` 独立存储 Keychain Token 并自动迁移旧版单机配置。 | `a3f412c` |
| 官方原生五品牌图标提取、界面重写与切换帧预算优化 | 提取五大 Agent 官方原生品牌图标与高对比色板，深度重写侧栏、会话列表、Probe 矩阵与远端机器切换器；`Conversation` 派生视图 `WeakMap` 缓存与无变更轮询跳过重绘。 | `58ddf92` |
| Antigravity 原生执行段核验、子进程活性与中间提问 Bark 推送 | 当前轮增量拉取改调原生 `GetCascadeTrajectoryExecutorMetadatas` 核验真实段尾步下标与终态 `PLANNER_RESPONSE`／`FINISH`；无日志后台任务结合 `language_server` 子进程存活后再走 60s 静默兜底；覆盖 `/plan` `RequestFeedback: true`、纯文本提问、首段 `WAITING`、新增交互变体与时间戳回退的 `reply_needed`／`approval_needed` 推送。 | `f2a9067` |
| 服务端存储／内存／网络精简 | SQLite 定期按保留窗口裁剪 `probe_passive_sent_marker`／`probe_async_question_delivery`／终态投递与错误事件并执行被动 WAL Checkpoint + `optimize`；全 Provider 会话详情缓存 15 分钟空闲淘汰；`/api/*` 挂载 `>1 KiB` Gzip 压缩；单轮巡检共享 Codex 线程头快照；移除零引用 `thiserror` 与冗余 `reqwest` 特性。 | `9a77f71` |
| 删除执行端二次校验加固与全平台统一线程清理 | Codex 归档／隐藏清理（支持空候选无库安全空操作与非空强制库校验）、Claude 删除与 Grok 删除均在执行端重算允许集、复核运行／占用状态并首错即停回报 `residuals`；新增 `cleanup.unifiedDryRun`／`cleanup.unifiedExecute` 与「全平台线程清理」覆盖五大 Agent 六大分类，同时保留旧单平台清理 RPC 契约兼容。 | `052bf62`, `619cae9`, `a1a7664` |
| Claude 合并组展开态保留、前端轮询收敛与原生品牌视觉重写 | `ExecutionGroup.mergedIds` 完整保留被吸收工具组展开与时间线定位状态；移除全局 15s 默认轮询并统一可见性感知轮询，`demo` 模块拆为独立异步 chunk；五大 Agent 全面切换官方原生 SVG 图标与浅／深色高对比官方品牌色盘，深度精修三层原生表面、会话卡片、折叠胶囊与清理工作台。 | `6039fff`, `a1a7664` |
| 交接路径加固与 Antigravity 状态／折叠／通知收敛 | Codex 交接路径优先解析在盘明文、归档或 `.jsonl.zst` rollout，Grok 过滤零字节占位并回退非空日志／摘要；Antigravity 读路径零 `flock`、在线状态严格对齐活跃轨迹与子代理、离线删除改用 `LOCK_SH` 且索引库错误显式阻断；本机环回 RPC 256 MiB + 增量步扫描 + 子代理／后台任务有界宽限期保障 Bark 不抢发且最终必达；在线／离线 `GENERIC` 工具结果与过渡旁白统一归入单轮折叠。 | `26e53fd` |
| Claude 复杂回合折叠与多色调宝石视觉重写 | Claude Code 识别尾部仅含自动记账的终态／网关错误边界，跨 `atis-latch`／`attachment`／`native_context` 合并同轮命令执行组并将过程相邻自动元数据并入单框折叠；全局升级为三层冷暖表面层次与七工作区专属宝石主色（常驻七色导航图标、专属色选中条、云青用户胶囊、深海蓝代码胶囊与多色顶栏仪表盘卡片），浅／深色全表面满足 WCAG `>= 4.5:1`。 | `23d937a` |
| 自包含并行标签发布门禁 | `ci.yml` 支持 `workflow_call` 复用；`release.yml` 在 `guard` 前置校验六节中文发布说明后，并行执行完整前后端／Tauri CI 门禁与 Linux／macOS 签名打包，支持 `main` 与标签一次同推。 | `cf85a02` |
| 隐私门禁与子会话删除加固 | 消除 x86_64 端口解析跨字面量立即数片段并恢复原生二进制公网 IP 隐私扫描；Antigravity 与 DSH 删除执行端重新核验真实子代理闭包，逐项检查根与子会话运行／锁状态，在线 RPC 或文件系统首错即停并返回 residuals。 | `13d7de2` |
| 五类 Agent 最优交接路径与筛选收敛 | Antigravity 优先复制完整纯文本转录日志，Grok 新增 `handoff_path`（优先 `chat_history.jsonl`）与复制路径菜单，Codex 覆盖活跃／归档 rollout，Claude 与 DSH 保持明文优先；移除顶部状态筛选栏，提供底部轻量折叠「已归档」抽屉并支持打开归档详情。 | `9847927`, `6170acf` |
| 访达多选、过程折叠、时间线与通知收敛 | 移除多选按钮与复选框，统一访达风格 `⌘`／`Shift` 多选、右键批处理菜单与 `⌘A`／`Escape`／`⌘⌫`；同回合交替思考与工具折叠为单条摘要（单执行组零 DOM 膨胀）；分页轮询保留首页 `timeline` 并补全在线批注与首轮提示；Antigravity 完成 Bark 等待根与子轨迹、后台任务及子代理全部收敛。 | `9847927`, `6170acf` |
| Antigravity／DSH 会话工作区 | 七项导航包含 Antigravity 与 DeepSeek Harness；支持分页阅读、时间线、搜索、双击改名、归档／恢复、复制 ID、复制路径、永久删除（无复制恢复命令）及多选批处理；DSH 写操作遵循 `session.lock` 与宿主进程离线安全守卫。 | `04dcb98`, `6170acf` |
| ChatGPT macOS App 风格重构 | 统一浅／深色中性侧栏与画布、访达多选圆角高亮、右侧紧凑胶囊用户气泡、无框助手 Markdown 与圆角代码块、低干扰过程折叠行、重绘左侧横线时间线、圆角菜单／弹窗及卡片化 Probe／设置面板，保持 WCAG `>= 4.5:1` 对比度、`48px` 折叠导航与 WebKit 帧预算不变量。 | `04dcb98`, `6170acf` |
| 本机 Antigravity／DSH 通知 | 原生执行终态、WAITING 交互、v4 明文／多帧压缩读取；四类独立开关、启用基线、身份去重、回复和审批解除、旧失败失效。只在本机运行。 | `v1.3.11` |
| 多选与菜单 | 三种 Provider 共用 ⌘／Shift／普通／键盘点击和锚点；删除无数量上限，归档／恢复按 100 项分批，轮询不新增选择，切换范围清空。 | `5ffd995` |
| Codex 删除 | 按确认的原生父子关系直接删除普通／归档 rollout、压缩副本、writer lock、shell snapshots、visualizations、数据库、分页历史和名称索引；循环去重，错误逐项返回。 | `5ffd995` |
| Claude 管理 | 改名保留原有身份与占用规则；删除按项目相对路径和文件名定位主文件、同名目录、旁路及共享索引，不依赖正文完整性。 | `5ffd995` |
| Claude 自动内容 | 完整命令包装显示为命令与原始参数；通知、回显、上下文和摘要不创建用户回合，在原位置默认折叠并保留搜索、分页锚点。 | `f150ce8` |
| Claude／Grok 读取 | Claude 空文件、命名记录和仅元数据会话显示“暂无对话”；Grok 缺少 `updates.jsonl` 时读取 `chat_history.jsonl`，过滤系统提示、system_reminder 和加密推理。 | `5ffd995` |
| 直接删除边界 | fingerprint 可省略且不参与删除，expectedCount 不参与清理；缺失文件完成，权限／数据库失败继续处理其他目标并返回 residuals。 | `5ffd995` |
| Playwright 浏览器守卫 | 移除品牌 channel，启动探针核对 Playwright 缓存内真实可执行文件；浏览器回归与 macOS 有头性能检查交给三项 CI。 | `0d7afea` |
| Bark 与 hooks | 新事件统一单条短摘要；本机 hooks 须明确安装，远端继续轮询。Claude prompt 身份核验后每回合每类认领一次，保留有效旧完成以补发，完成后不单独提醒空闲。Codex 保留逐问题身份和提醒。 | `01be4b4` |
| Claude 提醒复核 | 首次处理、首次发送和每次重试均复核当前主回合与实际执行活动；新回合、恢复执行、成功及取消终结旧失败提醒，保留有效旧完成补发。 | `8e222e1` |
| DSH 归档清理 | 当前机器的维护页预览归档及已核验 subagent 后代；退出 DSH 后按固定身份清理专属文件和匹配索引，支持压缩文件、自定义根、并发保护与逐项重试。 | `8e222e1` |
| 线程消息排序 | Codex、Claude Code 和 Grok Build 均按用户或助手最后一条对话消息时间倒序；工具、改名和其他元数据更新时间不改变顺序，未知时间保持稳定。 | `5770133` |

实现、安全边界、旧字段与旧队列兼容见[架构说明](../ARCHITECTURE.md)。删除按确认目标逐项执行，失败返回 residuals 并继续其他目标；归档、恢复和改名仍沿用各自规则。Claude 删除不触碰工作目录或共享文件中的非目标记录。

## 必要验证

2.0.10 本机针对性回归覆盖固定删除集合与跨根拒绝、DSH 缺换行／半记录／半帧、通知旧数据迁移与重启、终态正文清除、unknown 清理和当前游标去重、密钥异常与有效密钥保留、历史缓存失效和附件刷新，以及空闲轮询不重写游标／不扫描历史。受影响 core、webd、Tauri 测试、两端 Clippy、类型／契约、版本／工作区、隐私及安装脚本检查通过；两份锁文件已确认不再命中本轮 anyhow／Tokio 通告。完整回归和跨平台构建由标签 CI 承担，未访问真实钥匙串或用真实用户会话执行删除测试。

相同合成样本的开发构建比较：

| 样本与指标 | 修复前 | 修复后 |
|---|---:|---:|
| 约 12.5MB DSH 历史热读耗时／CPU 时间 | 1652.108／1651.332ms | 0.528／0.518ms |
| 大历史峰值 RSS | 73.9MiB | 60.1MiB |
| 257 个摘要热读耗时 | 63.857ms | 19.658ms |
| 1000 个未变会话一轮 WAL 新增写入 | 32,960 字节 | 4,120 字节 |
| 2 万条无变化通知处理 CPU 时间 | 32.688ms | 0.181ms |

大历史冷读基本持平，部分小样本峰值 RSS 增加约 0.03–0.36MiB；空闲游标 CPU 时间基本持平。WAL 写入下降不代表磁盘空间已回收，以上不是线上 CPU、内存或硬盘占用收益。本轮未新增性能阈值断言或 CI 门禁。

2.0.0 本机与主／标签 CI 覆盖 `agenthub-core`／`agenthub-webd`／`src-tauri` 全量测试、格式与两端 Clippy（含 Antigravity 顶层 `"agenthub"` 契约安装／修复／移除与遗留 `"hooks"` 清理、`antigravity-hook` `{"decision":"allow"}` stdout 输出、Grok 跨 handler 重复条目清理、Antigravity 端点缓存合并与存活进程瞬时错误不写负缓存、`READONLY_SNAPSHOT_CACHE`／`LIVE_PROVIDER_SESSIONS_CACHE`／`GROK_SNAPSHOT_CACHE` 环境键隔离与 `evict_idle_detail_caches` 统一回收、`estimate_bytes_batch` 与 `inventory` 共享路径扫描、`execute_delete_hidden` `O(K)` 路径、DSH `O(events)` 工具状态结算、Probe 单次扫描复用）、Rust／TypeScript 线协议契约锁、WebUI 65 文件 839 项 Vitest 与类型检查、安装脚本及发布载荷隐私扫描。未使用真实用户会话执行写操作或删除测试。

1.9.0 本机与主／标签 CI 覆盖 `agenthub-core`／`agenthub-webd`／`src-tauri` 全量测试、格式与两端 Clippy（含 DSH 已打开完成线程不误报 `running`、Seeded 会话事件边界与 `turn/end` 工具状态收口、`.zstd` 折叠缓存、Antigravity 端点缓存、Claude 活动快照缓存、`probe.status` 3s 缓存、统一清理管线重算校验与 `O(K)` 隐藏校验及 `O(N)` 单次体积估算、全量测试沙箱隔离、Grok 与 Antigravity 受管 Hooks 安装／修复／移除与 CLI 实时投递、`AgentHub.app` 平台路径断言）、Rust／TypeScript 线协议契约锁、WebUI 64 文件 836 项 Vitest 与类型检查、安装脚本及发布载荷隐私扫描；GitHub Actions 主 CI 与自包含标签 Release CI 覆盖 280 项 Chromium／WebKit 端到端测试（含浅／深色 `--bg-rail`／`--bg-list`／`--bg-stage` 三层中性表面对比度与零溢出检查）及 macOS 有头双引擎交互帧预算检查。未使用真实用户会话执行写操作或删除测试。

1.8.0 本机与主／标签 CI 已通过 `agenthub-core`／`agenthub-webd`／`src-tauri` 全量测试、格式与两端 Clippy（覆盖全量 Git 对象与构建产物零隐私命中、测试 `HOME` 隔离、`unifiedExecute` 越界 ID 拒绝与运行态拦截、详情缓存 300s 空闲裁剪、Antigravity 时间单调与同执行器多次提问去重、SQLite 原子认领、五大 Agent 平台开关与 Hooks 卸载、五平台行菜单／Finder／全局搜索对齐、多命名远端机器 CRUD 与 Keychain 迁移）、Rust／TypeScript 线协议契约锁、WebUI 64 文件 836 项 Vitest 与类型检查、安装脚本及发布载荷隐私扫描；GitHub Actions 主 CI 与自包含标签 Release CI 全绿通过 280 项 Chromium／WebKit 端到端测试及 macOS 有头双引擎交互帧预算检查（`expand`、`scroll`、`switch` `p95 <= 100ms`）。未使用真实用户会话执行写操作或删除测试。

1.7.0 本机（含隔离空 `HOME` 全量工作区实测）与主／标签 CI 已通过 `agenthub-core`（629 项）／`agenthub-webd`（206 项及 CLI 集成）／`src-tauri` 全量测试、格式与两端 Clippy（覆盖原生多执行段间隙防抢发、子进程存活越过 60s 静默宽限、中间提问四类路径推送、SQLite 标记裁剪与被动 Checkpoint、详情缓存空闲淘汰、Gzip 压缩协商、Codex／Claude／Grok／统一清理越界 ID 拒绝与运行态拦截及首错即停、无 `state_5.sqlite` 空候选安全空操作）、Rust／TypeScript 线协议契约锁、WebUI 65 文件 837 项 Vitest（含 Claude 合并组展开态保留与全平台清理工作台）与类型检查、`demo` 异步分包构建、安装脚本及发布载荷隐私扫描；GitHub Actions 主 CI 与自包含标签 Release CI 全绿通过七工作区浅／深色 × 八视口官方品牌视觉零溢出与对比度 E2E、全平台线程清理 E2E 及 macOS 有头 WebKit 交互帧预算检查。未使用真实用户会话执行写操作或删除测试。

1.6.0 本机与主／标签 CI 已通过 `agenthub-core`／`agenthub-webd`／`src-tauri` 全量测试、格式与两端 Clippy（含 Codex 明文／归档／压缩交接路径解析、Grok 零字节占位过滤、Antigravity `presence.lock` 闲置不误判与离线 `LOCK_SH` 拦截、损坏 `conversation_summaries.db` 阻断删除、超长会话当前轮增量步扫描、子代理／后台任务无 `SYSTEM_MESSAGE` 退出有界宽限期最终必达与活跃期抑制双向断言、在线／离线 `GENERIC` 工具与过渡说明折叠）、Rust／TypeScript 线协议契约锁、WebUI 65 文件 835 项 Vitest（含 Claude 穿插自动元数据与网关错误结尾折叠）与类型检查、安装脚本及发布载荷隐私扫描；GitHub Actions 主 CI 与自包含标签 Release CI 一次全绿通过七工作区浅／深色 × 八视口多色调界面零溢出与对比度 E2E 及 macOS 有头 WebKit 交互帧预算检查。未使用真实用户会话执行写操作或删除测试。

1.5.0 本机与主 CI 已通过 `agenthub-core`／`agenthub-webd`／`src-tauri` 全量测试及两端 Clippy（含子会话越界注入、运行／锁拦截、在线 RPC 失败零误删与部分文件失败残留报告、五大 Agent 交接路径优先级、Antigravity 后台任务／定时器／子代理未收敛抑制 Bark 完成提醒）、Rust／TypeScript 线协议契约锁、WebUI 65 文件 833 项 Vitest 与类型检查、安装脚本及恢复原生二进制 `ipv4` 公网 IP 检查后的发布载荷隐私扫描；GitHub Actions 三项主 CI 已通过七工作区浅／深色 × 八视口零溢出与对比度 E2E、底部「已归档」抽屉与访达 `⌘`／`Shift` 多选批处理 E2E、五类 Agent 上下文菜单（含 Grok 复制路径）E2E 以及 macOS 有头 WebKit 交互帧预算检查。未使用真实用户会话执行写操作或删除测试。

1.4.0 本机与主 CI 已通过 `agenthub-core`／`agenthub-webd`／`src-tauri` 全量测试及两端 Clippy、Rust／TypeScript 线协议契约锁、WebUI 64 文件 825 项 Vitest 与类型检查、安装脚本与发布载荷隐私扫描；GitHub Actions 三项主 CI 已通过七工作区浅／深色 × 八视口（含 `320px`）零溢出与对比度 E2E、五项上下文菜单（确认不含“复制恢复命令”）E2E 以及 macOS 有头 WebKit 交互帧预算检查。未使用真实用户会话执行写操作或删除测试。

1.3.11 本机已通过 core 全量测试、WebUI 全量测试及类型检查、Tauri 测试和 Clippy；新增实际 CLI 回环用例验证 DSH 四类投递、重启去重及处理交互／恢复后的旧提醒停止重试。Antigravity 原生 API 仅作只读结构核对；事件识别、请求身份、服务不可用及非等待状态由合成用例验证。Zstandard 依赖升级至修复版 0.14.3／zstd-safe 8.2.0，保留现有依赖安全门禁。正式构建与完整回归以对应提交 CI 为准，真实交互及手机实收待用户验收。

以下证据覆盖 1.3.10 核心实现及收尾；最终提交 [`6811cb4`](https://github.com/lich13/agenthub/commit/6811cb4e7a8d7ea763191640b2badd42e6e93807) 的[三项主 CI](https://github.com/lich13/agenthub/actions/runs/40045161446) 与 [`v1.3.10` Release CI](https://github.com/lich13/agenthub/actions/runs/40046131999) 已通过。

| 范围 | 已通过证据 |
|---|---|
| Rust core | Claude 真实失败、新回合、恢复执行、取消、历史完成补发及辅助元数据测试通过；DSH 归档隔离、子线程与 fork、压缩文件、缺失文件、恢复、并发修改、部分失败与再次清理测试通过。 |
| Rust webd | 实际 Cargo CLI 覆盖 Hook／轮询去重、首发前恢复、重试前恢复及旧完成补发；受影响 API、适配器和完整 workspace 测试由主 CI 通过。 |
| Tauri | 新增 DSH 薄入口、命令注册及远端 allowlist 契约通过；Tauri 测试和 Clippy 通过。 |
| WebUI | Vitest 811 项及类型检查通过，覆盖 DSH 固定确认身份、失败结果、缺少数据、旧服务能力和机器切换后的状态重置；浏览器回归与 macOS 性能检查由主 CI 通过。 |
| 静态与脚本 | Rust 格式、两端 Clippy、Rust／TypeScript 契约、版本／工作区、安装脚本、systemd 路径及隐私检查通过。 |

未使用真实用户会话执行删除测试，未发送真实 Bark 验收消息；功能、视觉和端到端验收由用户完成。完整跨平台和性能门禁继续由现有三项 CI 执行，没有新增性能门禁或本机正式打包。

DSH 使用 workspace 原生归档名单，只纳入可核验的 subagent 后代，独立 fork 保留。执行请求不接受路径，恢复或归属变化跳过；共享索引仅原子移除成功目标，无匹配不写入。自定义持久化后端和无法识别格式明确拒绝，缺少数据目录显示空状态。

## 正式交付

2.0.10 已完成本机实现与必要验证，待标签 CI、正式发布、服务端部署及 Mac 原地安装。

1.9.0 按主题拆分的提交 [`62ade32`](https://github.com/lich13/agenthub/commit/62ade32)、[`1ef92f3`](https://github.com/lich13/agenthub/commit/1ef92f3)、[`b8f2211`](https://github.com/lich13/agenthub/commit/b8f2211)、[`39a4007`](https://github.com/lich13/agenthub/commit/39a4007)、[`221b809`](https://github.com/lich13/agenthub/commit/221b809) 与 [`7395f48`](https://github.com/lich13/agenthub/commit/7395f48fd9bb2f19b4ed6e4a02c2a4a53efb0584) 已与 `v1.9.0` 标签同推，[三项主 CI](https://github.com/lich13/agenthub/actions/runs/41179300628) 与自包含并行 [`v1.9.0` Release CI](https://github.com/lich13/agenthub/actions/runs/41179300785) 均全绿通过，已发布[八项正式资产](https://github.com/lich13/agenthub/releases/tag/v1.9.0)。已按先云端后本机顺序，先使用正式 Linux 服务包升级远端节点 `agenthub-webd 1.9.0`，service 为 `active`、health 为 `{"ok":true}`；再使用正式 `AgentHub-1.9.0-darwin-arm64.dmg`（SHA-256 与签名校验通过）原地替换安装本机 macOS App（`AgentHub.app 1.8.0 -> AgentHub.app 1.9.0`，Bundle ID `com.lich13.agenthub`），bundle 版本及内置 `agenthub-webd` 均为 1.9.0 且启动运行，没有保留旧副本。计划中建议不做的 1 项（`A6` `projcache` 额外后台任务／状态表）按确认结论未并入实现。真实用户会话未执行删除，待用户验收。

1.8.0 按单一独立主题拆分的六个提交 [`1545b61`](https://github.com/lich13/agenthub/commit/1545b61bd416f85c4f976614fb5b52110aa2f868)、[`3dc983a`](https://github.com/lich13/agenthub/commit/3dc983a736e22cb91ef0fa40e705c2ef6288a7f5)、[`436ee88`](https://github.com/lich13/agenthub/commit/436ee88fbff0d74a15bc5ba07213305fc35334d3)、[`b9a0812`](https://github.com/lich13/agenthub/commit/b9a0812e9a455ca52711fc54e023b959a74be735)、[`a3f412c`](https://github.com/lich13/agenthub/commit/a3f412cebf72620fd1609614fc821ae5774dc661) 与 [`58ddf92`](https://github.com/lich13/agenthub/commit/58ddf92a741de654f9df0037551c0c5607909250) 已与 `v1.8.0` 标签同推，[三项主 CI](https://github.com/lich13/agenthub/actions/runs/41114213895) 与自包含并行 [`v1.8.0` Release CI](https://github.com/lich13/agenthub/actions/runs/41114214135) 均全绿通过，已发布[八项正式资产](https://github.com/lich13/agenthub/releases/tag/v1.8.0)。已按先云端后本机顺序，先使用正式 Linux 服务包升级远端节点 `agenthub-webd 1.8.0`，service 为 `active`、health 为 `{"ok":true}`；再使用正式 `AgentHub-1.8.0-darwin-arm64.dmg`（SHA-256 与签名校验通过）原地覆盖安装本机 macOS App，bundle 版本及内置 `agenthub-webd` 均为 1.8.0 且启动运行，没有保留旧副本。未确认的 4 项（`M5`、`L7`、`L8`、`A7`）按已批准计划未并入实现。真实用户会话未执行删除，待用户验收。

1.7.0 按单一独立主题拆分的提交 [`f2a9067`](https://github.com/lich13/agenthub/commit/f2a906717a810613f91f7ac212adfba7908f91eb)、[`9a77f71`](https://github.com/lich13/agenthub/commit/9a77f71d0649380e9763c26ded671caecc102723)、[`052bf62`](https://github.com/lich13/agenthub/commit/052bf620c4e293fb4d99c250ba91aa35aac555dc)、[`6039fff`](https://github.com/lich13/agenthub/commit/6039fff358c9c522068e4d7c7ca81d4c994ede4e)、[`a1a7664`](https://github.com/lich13/agenthub/commit/a1a7664265174da58629dcc38c4af9bc8396a669)、[`f1ff6ae`](https://github.com/lich13/agenthub/commit/f1ff6aed5f89f959850da7cd573ac317a8868430) 与无库空候选修复提交 [`619cae9`](https://github.com/lich13/agenthub/commit/619cae992cdffeae33874b6e20c2c90eb4d83576) 已与 `v1.7.0` 标签同推，[三项主 CI](https://github.com/lich13/agenthub/actions/runs/41012893337) 与自包含并行 [`v1.7.0` Release CI](https://github.com/lich13/agenthub/actions/runs/41012893611) 均全绿通过，已发布[八项正式资产](https://github.com/lich13/agenthub/releases/tag/v1.7.0)。已按先云端后本机顺序，先使用正式 Linux 服务包升级远端节点 `agenthub-webd 1.7.0`，service 为 `active`、health 为 `{"ok":true}`；再使用正式 `AgentHub-1.7.0-darwin-arm64.dmg`（SHA-256 与签名校验通过）原地覆盖安装本机 macOS App，bundle 版本及内置 `agenthub-webd` 均为 1.7.0 且启动运行，没有保留旧副本。未确认的 3 项（A4 同推双跑 CI 改造、S6 目录体积缓存 30s TTL 调整、B4 移除旧单平台清理底层 RPC 命令名）按确认结论未并入实现。真实用户会话未执行删除，待用户验收。

1.6.0 按主题拆分的三个独立提交 [`26e53fd`](https://github.com/lich13/agenthub/commit/26e53fdeb7efc521258d446787c5036a6ca84d88)、[`23d937a`](https://github.com/lich13/agenthub/commit/23d937acecc96e3cdc2a01c422548bab3886a692) 与 [`cf85a02`](https://github.com/lich13/agenthub/commit/cf85a026c1fcb16e614b25f0c50f85db039c03f0) 已与 `v1.6.0` 标签同推，[三项主 CI](https://github.com/lich13/agenthub/actions/runs/40865327483) 与自包含并行 [`v1.6.0` Release CI](https://github.com/lich13/agenthub/actions/runs/40865327712) 均一次全绿，已发布[八项正式资产](https://github.com/lich13/agenthub/releases/tag/v1.6.0)。已按先云端后本机顺序，先使用正式 Linux 服务包升级远端节点 `agenthub-webd 1.6.0`，service 为 `active`、health 为 `{"ok":true}`；再使用正式 `AgentHub-1.6.0-darwin-arm64.dmg`（SHA-256 与签名校验通过）原地覆盖安装本机 macOS App，bundle 版本及本机监控守护进程均为 1.6.0 且启动运行，没有保留旧副本。真实用户会话未执行删除，待用户验收。

1.5.0 核心提交 [`13d7de2`](https://github.com/lich13/agenthub/commit/13d7de25ff8cbc1625736b306c7cfad5d88f7431)、[`9847927`](https://github.com/lich13/agenthub/commit/9847927ffc20406ba71718dc9871c6ee6a346880)、[`6170acf`](https://github.com/lich13/agenthub/commit/6170acf393622c9bbb9f1d002ce147d2820ac166) 与发版提交 [`016db18`](https://github.com/lich13/agenthub/commit/016db18c08b9bc91d68b661d6248b538b500c3cb) 的[三项主 CI](https://github.com/lich13/agenthub/actions/runs/40815957745)（macOS WebKit 性能作业重跑后）与 [`v1.5.0` Release CI](https://github.com/lich13/agenthub/actions/runs/40817452620) 全绿，已发布[八项正式资产](https://github.com/lich13/agenthub/releases/tag/v1.5.0)（Linux 与 macOS 原生二进制均通过恢复后的公网 IP 隐私门禁）。已按先云端后本机顺序，先使用正式 Linux 服务包升级远端节点 `agenthub-webd 1.5.0`，service 为 `active`、health 为 `{"ok":true}`；再使用正式 `AgentHub-1.5.0-darwin-arm64.dmg`（SHA-256 与签名校验通过）原地覆盖安装本机 macOS App，bundle 版本及本机监控进程均为 1.5.0 且启动运行，没有保留旧副本。真实用户会话未执行删除，待用户验收。

1.4.0 发布提交 [`04dcb98`](https://github.com/lich13/agenthub/commit/04dcb98b1e7d452dda5ffe28ba6f9dfd5aba065e) 的[三项主 CI](https://github.com/lich13/agenthub/actions/runs/40648048212)与 [`v1.4.0` Release CI](https://github.com/lich13/agenthub/actions/runs/40648932371) 全绿，已发布[八项正式资产](https://github.com/lich13/agenthub/releases/tag/v1.4.0)。已按先云端后本机顺序，先使用正式 Linux 服务包升级远端节点 `agenthub-webd 1.4.0`，service 为 `active`、health 为 `{"ok":true}`；再使用正式 `AgentHub-1.4.0-darwin-arm64.dmg`（SHA-256 与签名校验通过）原地覆盖安装本机 macOS App，bundle 版本为 1.4.0 且启动运行，没有保留旧副本。真实用户会话未执行删除，待用户验收。

1.3.11 发布提交 `1af6332` 的[三项主 CI](https://github.com/lich13/agenthub/actions/runs/40361843004)与 [Release CI](https://github.com/lich13/agenthub/actions/runs/40363304134)已通过，发布[八项正式资产](https://github.com/lich13/agenthub/releases/tag/v1.3.11)。已使用对应 Linux 产物部署远程服务器，服务版本为 1.3.11、service 为 active、health 为 `{"ok":true}`；已使用对应 DMG 原地安装 Mac，版本为 1.3.11 且启动运行，没有保留旧副本。本机 Antigravity／DSH 总开关及四类提醒已按授权开启，复用原有 Bark 配置；首次扫描无读取错误并跳过历史，监控进程运行。云端两项开关关闭。真实交互及手机实收仍待用户验收。

1.3.10 已发布[八项正式资产](https://github.com/lich13/agenthub/releases/tag/v1.3.10)。已使用正式 Linux 服务包部署远程服务器 `agenthub-webd 1.3.10`，service 为 `active`、health 为 `{"ok":true}`；已使用 `AgentHub-1.3.10-darwin-arm64.dmg` 原地安装 macOS App，bundle 版本为 1.3.10，签名核验通过且启动运行，没有保留旧副本。

1.3.9 的[三项主 CI](https://github.com/lich13/agenthub/actions/runs/39865303352)与 [`v1.3.9` Release CI](https://github.com/lich13/agenthub/actions/runs/39866444443) 全绿，已发布[八项正式资产](https://github.com/lich13/agenthub/releases/tag/v1.3.9)。已使用正式 Linux 服务包部署远端 API，版本为 1.3.9、service 为 active、health 为 `{"ok":true}`；已用 `AgentHub-1.3.9-darwin-arm64.dmg` 原地安装 macOS App，校验文件和签名通过，bundle 版本为 1.3.9 且启动成功，没有保留旧副本。安装与部署脚本已清理各自临时文件；真实用户会话未执行删除，仍待用户验收。

1.3.8 已同步现有 manifest 和两份 lock 中的自身版本，发布说明使用中文六节，保留八项正式资产：macOS DMG／校验、updater／签名、latest.json，以及 Linux API 服务包／校验／签名。核心修复提交 [`c645e44`](https://github.com/lich13/agenthub/commit/c645e44885a43f34ebddefb3e3f172c84a38a88b) 的[三项主 CI](https://github.com/lich13/agenthub/actions/runs/39840632543)（macOS WebKit 性能作业重跑后）全绿；[`v1.3.8` Release CI](https://github.com/lich13/agenthub/actions/runs/39843284210) 全绿并发布[八项正式资产](https://github.com/lich13/agenthub/releases/tag/v1.3.8)。已使用正式 Linux 服务包部署远端 API，版本为 1.3.8、service 为 active、health 为 `{"ok":true}`；已使用正式 macOS ARM64 DMG 原地安装 App，bundle 版本为 1.3.8 且启动成功，没有保留旧副本。收尾未创建预览服务或 SSH 转发；两个真实样例未执行删除，保留待用户验收。

上一版交付记录：提交 [`51dfcd5`](https://github.com/lich13/agenthub/commit/51dfcd5225eef215b4dade342d4330d3ad42159f) 的 [三项主 CI](https://github.com/lich13/agenthub/actions/runs/39585391199) 全绿，`v1.3.5` 的 [Release CI](https://github.com/lich13/agenthub/actions/runs/39586479754) 全绿并发布 [八项正式资产](https://github.com/lich13/agenthub/releases/tag/v1.3.5)。已使用正式 Linux 资产升级远端 API，服务版本、健康、公开 healthz、未授权 API 和退役路径检查通过；已安装正式 macOS ARM64 App，版本为 1.3.5 且启动成功。部署和安装保留既有回退材料，未自动启用 Claude hooks。

1.3.6 核心实现由 [`01c5695`](https://github.com/lich13/agenthub/commit/01c5695)、[`2aaa929`](https://github.com/lich13/agenthub/commit/2aaa929) 和 [`1cfa0a7`](https://github.com/lich13/agenthub/commit/1cfa0a7) 完成；版本、文档和 CI 启动器收尾提交为 [`7254912`](https://github.com/lich13/agenthub/commit/7254912)。该提交对应的[三项主 CI](https://github.com/lich13/agenthub/actions/runs/39716648742)（macOS 性能检查重跑后）与 [`v1.3.6` Release CI](https://github.com/lich13/agenthub/actions/runs/39717900484) 全绿，已发布[八项正式资产](https://github.com/lich13/agenthub/releases/tag/v1.3.6)。已使用正式 Linux 资产升级远端 API，服务版本、健康、公开 healthz、未授权 API 和退役路径检查通过；已安装正式 macOS ARM64 App，版本为 1.3.6 且启动成功，并保留 1.3.5 回退 App。收尾清理仅移除本轮临时文件和 AgentHub 专属缓存，实际回收 22,524 KiB；未自动启用 Claude hooks。

1.3.7 核心实现由 [`6ca0716`](https://github.com/lich13/agenthub/commit/6ca0716)、[`e363069`](https://github.com/lich13/agenthub/commit/e363069)、[`d73b2d4`](https://github.com/lich13/agenthub/commit/d73b2d4)、[`0d7afea`](https://github.com/lich13/agenthub/commit/0d7afea) 和 [`70602df`](https://github.com/lich13/agenthub/commit/70602df) 完成；版本、文档和发布说明收尾提交为 [`cffb3e9`](https://github.com/lich13/agenthub/commit/cffb3e9)。该提交对应的[三项主 CI](https://github.com/lich13/agenthub/actions/runs/39727312472) 与 [`v1.3.7` Release CI](https://github.com/lich13/agenthub/actions/runs/39728000909) 全绿，已发布[八项正式资产](https://github.com/lich13/agenthub/releases/tag/v1.3.7)。已使用正式 Linux 服务包升级远端 API，服务版本为 1.3.7、systemd 为 active、回环 healthz 成功；已安装正式 macOS ARM64 App，bundle 版本为 1.3.7 且启动成功，并保留 1.3.6 回退 App。安装和部署临时文件已清理，未自动启用 Claude hooks。

## 待用户验收

打开 `AgentHub.app 2.0.10`，检查五个平台的阅读、翻页、附件及删除确认范围；使用专门测试线程核验预览后新增子会话被保留。分别触发通知并确认手机实收，回答问题／处理审批／恢复执行后旧提醒不再重试。真实功能、视觉与手机实收待用户验收，线上资源占用未测量。

打开新版 `AgentHub.app`（`2.0.0`），在本机与远端切换下依次核验：① 全新「Command Nexus（多轨星枢）」应用图标与侧栏品牌徽标、566 行纯净三层表面体系（`--bg-rail`、`--bg-surface`、`--bg-stage`）下七大工作区的统一视觉与交互；② 打开「Probe」工作区，确认标题下已移除无用运行态胶囊行与冗余子分栏，五张宿主卡片（Codex、Claude Code、Grok Build、Antigravity、DeepSeek Harness）统一展示后端真实配置路径、4 格紧凑指标与弹窗确认开关（DSH 显示只读原生直连状态条，无禁用死按钮）；③ 在 Antigravity 与 Grok 宿主中完成一轮对话，确认 Stop Hook 正常放行且 Bark 收到单条通知、无重复触发；④「设置与维护」工作区的版本检查、全平台线程清理预览与诊断抽屉。

打开新版 `AgentHub.app`（`1.9.0`），在本机与远端切换下依次核验：① 应用名称 `AgentHub.app`、全新钛黑五节点星枢图标以及浅／深色 `--bg-rail`、`--bg-list`、`--bg-stage` 三层 ChatGPT macOS App 视觉表面；② DeepSeek Harness 工作区中已在桌面端打开但已结束的会话不再持续转圈，仅真正运行中的回合显示转圈；③ 打开「Probe」工作区，默认首屏即可直接操作探针与五大 Agent 通知开关、Bark 测试推送，并在「五平台 Hooks 与 Host 统一管理」矩阵中一键安装／修复／移除 Codex、Claude Code、Grok Build、Antigravity 受管 Hooks 及查看 DSH 原生直连状态；④「设置 → 更新与维护 → 全平台线程清理」六大分类预览与清理速度及准确性。

打开新版 `AgentHub.app`（`1.8.0`），在本机与远端切换下依次核验：① 侧栏、会话列表、Probe 与设置面板的五大 Agent 官方原生品牌图标与重写后的 macOS 原生界面；② 顶栏机器切换器与「设置 → 远端机器」中新增、编辑、切换和删除多台命名远端机器（旧版单机配置已自动迁移）；③ 「通知与探针 → 配置」中五大 Agent 平台级一键开关与 Claude／Codex Hooks 一键卸载；④ 五大 Agent 行菜单、本机 Finder 定位与 `⌘K` 全局搜索跨五大 Agent 的命中跳转。

打开新版 `AgentHub.app`（`1.7.0`），在本机与远端切换下依次核验：① 侧栏与各工作区顶栏／空状态的五大 Agent 官方原生品牌 SVG 图标、浅／深色高对比官方品牌色盘与重写后的 macOS 原生质感界面；② 「设置 → 更新与维护 → 全平台线程清理」一键扫描五大平台六大分类预览、隐藏线程深度扫描与二次确认清理流程（空间释放与跳过／残留分开展示）；③ Antigravity 会话中间纯文本提问、`/plan` 待确认或 `ask_question` 时的 Bark「需要回复」推送及两段执行间隙不抢发完成提醒；④ Claude Code 展开某一中间命令组后追加后续工具步骤被合并时仍保持展开状态。

打开新版 `AgentHub.app`（`1.6.0`），在本机与远端切换下依次核验：① 五类 Agent 的「复制路径」均复制出真实在盘最优交接文件；② Claude Code 复杂长会话（含穿插系统／上下文记录及错误收尾回合）与 Antigravity 超长会话的中间命令、思考与工具结果均收敛为单条折叠摘要；③ 已结束但在 Antigravity 窗口中保持打开的会话不再一直转圈或常亮「运行中」，新回合彻底完成后正常收到一次 Bark 完成通知；④ 七大工作区浅／深色三层冷暖表面与专属宝石色调视觉层次。

在本机与远端机器切换下打开五类 Agent 工作区，核验：顶部状态筛选栏与“多选线程”按钮已移除，列表底部可按需展开「已归档」抽屉并打开或恢复归档会话；五类 Agent 的「复制路径」（含 Grok Build 行菜单与顶栏更多菜单）均复制出最优纯文本／主交接文件；按住 `⌘` 单击、`Shift` 连选、右键批量菜单及 `⌘A`／`Escape`／`⌘⌫` 快捷键符合访达多选习惯；同一回合内交替的思考与工具执行折叠为单条摘要行，左侧横线时间线在五类 Agent 下均可悬停预览并准确跳转定位；在 Antigravity 发起包含后台命令或子代理的真实长任务，确认仅在整个线程彻底收敛后才收到一次完成 Bark 提醒。

在本机与远端机器切换下打开 Antigravity 和 DeepSeek Harness 工作区，核验全部／已归档列表、体积标注、分页详情、用户时间线、思考／工具折叠、⌘F 搜索，以及右键／更多菜单的五项动作（改名、归档或恢复、复制 ID、复制路径、永久删除，确认无“复制恢复命令”）、双击／F2 改名与多选批处理；用专门测试线程核验 DSH 运行态锁与宿主退出后的归档／恢复／删除保护，以及浅／深色 ChatGPT macOS App 风格视觉表现。

在本机 Probe 查看 Antigravity 和 DSH 开关，分别触发完成、失败、提问及授权并确认手机收到对应提醒；回答问题、处理审批或恢复执行后，确认旧提醒不再重试。Antigravity 原生服务须保持运行，DSH 使用标准 v4 文件存储；自动测试不代替手机实收和真实交互。

在本机和远端“更新与维护 → DSH 归档清理”预览归档，退出对应机器的 DSH 后清理专门测试线程，重开确认归档消失且其他线程保留。使用 Claude 时确认恢复执行后不再收到旧失败提醒；真实线程删除及手机实收由用户验收。

在三个 Provider 中测试 ⌘／Shift 多选、复选框及菜单；用专门测试线程检查空线程、待回复、回退 rollout 和整树删除，确认 Claude 运行时仍能管理同目录的其他历史会话。

随后在本机 Probe 设置中确认安装 Claude hooks，验证完成、问题、授权和失败提醒，完成后应无空闲重复提醒；Codex 同回合多个独立问题仍应分别通知。手机实收与日常交互未由助手代验。

1.3.7 真实验收：关闭占用目标的 codex 写入者后，预览并删除线程 `01a16116`，确认主 rollout、writer lock、shell snapshot 和 visualization 旁路均消失；验证原冲突 Claude 会话仍可改名和删除。浏览器回归与 macOS 有头性能检查以 GitHub Actions 为准，确认验收过程不需要或启动系统 Google Chrome。
