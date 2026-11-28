# AgentHub（AgentHub）当前状态（维护者）

## 当前版本

当前源码版本为 **2.1.42**。API 协议 2，数据库 schema 3。完成跨平台内核级进程／打开文件探针与六大 Provider 会话运行状态失准根治（消除 macOS `ps` 中文路径 Octal 转义失配与 `lsof` 64KiB 截断／阻塞，修复 Claude Code 同工作目录多会话归属、`--resume` 历史未结会话所有权保护、版本目录进程、已核验 `busy`/`waiting`/`idle` 原生状态与未决提问及流式半行防抖、僵尸 PID 清理，补齐 Codex `thread-writer-locks/<id>.lock` 排他锁与宿主 Worker 存活核验及子代理流式半行防抖，修正 Grok Build 进程存活与开回合判定、Antigravity 多窗口多实例端点并发枚举路由（含未水合冷会话多实例路由、未响应存活实例写门禁与 Linux `root` 跨 UID 探测）及 3600s 崩溃锁兜底，并将 DSH 与本机 Grok Bot 纳入 Probe 全平台实时态势）、全仓 `$de-gpt` 深度减法与六大平台缓存／检索／分页／防符号链接写基础设施收口（移除退役 `axum` 与 6 个转发空壳文件，清理 Probe 高级设置三个无效输入框与死分支，补齐 `JsonlCache` 读前前缀锚点与并发尾追加兼容）、服务端大响应零双缓冲流式透传（`write_raw_response` + `Box<RawValue>` 原文透传）与空闲 300s 窗口零 SQLite WAL 写入及 Claude `SUMMARY_CACHE` 增量追加，以及 macOS Swift 6 窄窗（`<880pt`）、`296pt` 顶栏零截断、中文工作目录无空隙排版、多行问答动态高度与 120Hz 阅读器纯追加／流式末行裂变／宽屏缩放局部刷新优化。存量升级库保留遗留 `codex_desktop_submissions` 表与钥匙串项，二进制回退目标保持为 **v2.1.30**。

- **v2.1.42** 本地实现、前后端契约、Rust／Swift 6 Debug＋Release WMO 编译、离屏全场景深浅色光栅渲染与合成基准验证已完成；标签发布、远端 Linux `agenthub-webd 2.1.42` 部署、本机 `AgentHub.app 2.1.42` 安装及跨平台中文路径真实会话状态、多实例 Antigravity、窄窗 Probe 与长会话阅读手感待推送发布与用户验收。
- **v2.1.41** 已发布八项资产。Linux 服务为 `agenthub-webd 2.1.41`，状态 `active`，健康检查 `ok`。Mac App 已由 2.1.40 更新为 2.1.41 并启动；Probe 文案与能力胶囊、子智能体摘要与「复制答复」提示、附件缩略图失败状态与滚回重试、工作区总览反馈列颜色、设置页分页条及长列表滚动手感待用户验收。[提交 de0ee8c](https://github.com/lich13/agenthub/commit/de0ee8c67fe1860907c73e02a551736882643e9d) · [CI 优化提交 fb9a66a](https://github.com/lich13/agenthub/commit/fb9a66ac0fb7176d632fdefd1983deab8542625f) · [Release](https://github.com/lich13/agenthub/releases/tag/v2.1.41) · [Release CI](https://github.com/lich13/agenthub/actions/runs/45950183365)
- **v2.1.40** 已发布八项资产。Linux 服务为 `agenthub-webd 2.1.40`，状态 `active`，健康检查 `ok`。Mac App 已由 2.1.39 原地更新并启动；GitHub CI `260pt` 有界内部滚动与边界滚轮透传、Probe 核心配置区默认展开与内联事件胶囊、深浅色气泡与紧凑代码块交互及 Bark／macOS 系统通知实收待用户验收。[提交 a4a8339](https://github.com/lich13/agenthub/commit/a4a833942590025788dd846fd81202d60892b8b5) · [Release](https://github.com/lich13/agenthub/releases/tag/v2.1.40) · [Release CI](https://github.com/lich13/agenthub/actions/runs/45878849794)

- **v2.1.39** 已发布八项资产。Linux 服务为 `agenthub-webd 2.1.39`，状态 `active`，健康检查 `ok`。Mac App 已由 2.1.38 原地更新并启动；单屏可折叠 Probe 控制台（分面板独立保存与新异常单次展开）、`1000×680` 窄窗阅读与 `⌘F` 检索及 Bark／系统通知实收待用户验收。[提交 dff186f](https://github.com/lich13/agenthub/commit/dff186f5970c5944aa9ac2ecfc30f7848e05e6c5) · [Release](https://github.com/lich13/agenthub/releases/tag/v2.1.39) · [Release CI](https://github.com/lich13/agenthub/actions/runs/45759917895)

- **v2.1.38** 已发布八项资产。Linux 服务为 `agenthub-webd 2.1.38`，状态 `active`，健康检查 `ok`。Mac App 已由 2.1.37 原地更新并启动；Codex 纯只读会话布局（移除新建线程、悬浮输入协作区与「在 Codex 中打开」）、单层自绘表格、跨平台工作区交互及 Bark／macOS 系统通知实收待用户验收。[提交 cbc0319](https://github.com/lich13/agenthub/commit/cbc0319b0e01cc6b3e59128cd3752b68d402838e) · [Release](https://github.com/lich13/agenthub/releases/tag/v2.1.38) · [Release CI](https://github.com/lich13/agenthub/actions/runs/45627291894)

- **v2.1.37** 已发布八项资产。Linux 服务为 `agenthub-webd 2.1.37`，状态 `active`，健康检查 `ok`。Mac App 已由 2.1.36 原地更新并启动；超大 Claude 会话向上滚动与时间线跳转、Codex 表格首行显示与伴侣页快照净化待用户验收。[提交 2ed804c](https://github.com/lich13/agenthub/commit/2ed804c56adeea46287f9225a8d8feb32f289baf) · [Release](https://github.com/lich13/agenthub/releases/tag/v2.1.37) · [Release CI](https://github.com/lich13/agenthub/actions/runs/45540691647)

- **v2.1.36** 已发布八项资产。Linux 服务为 `agenthub-webd 2.1.36`，状态 `active`，健康检查 `ok`。Mac App 已由 2.1.35 原地更新并启动；长会话思考块与工具循环中间过渡说明默认折叠、跨会话与反向翻页折叠态保持，以及合并后的工作区与菜单交互待用户验收。[提交 3e0812d](https://github.com/lich13/agenthub/commit/3e0812d6e8a084f8ceb514e45a695f097b654e53) · [Release](https://github.com/lich13/agenthub/releases/tag/v2.1.36) · [Release CI](https://github.com/lich13/agenthub/actions/runs/45489940549)

- **v2.1.35** 已发布八项资产。Linux 服务为 `agenthub-webd 2.1.35`，状态 `active`，健康检查 `ok`。Mac App 已由 2.1.34 原地更新并启动；Antigravity 用户气泡零系统提示词占位、未索引与斜杠指令会话标题、子智能体角色名与委派内容提纯待用户验收。[提交 acb398d](https://github.com/lich13/agenthub/commit/acb398d7a194ce665539e0dc252976c4bc24d288) · [Release](https://github.com/lich13/agenthub/releases/tag/v2.1.35) · [Release CI](https://github.com/lich13/agenthub/actions/runs/45446575906)

- **v2.1.34** 已发布八项资产。Linux 服务为 `agenthub-webd 2.1.34`，状态 `active`，健康检查 `ok`。Mac App 已由 2.1.33 原地更新并启动；跨平台「工作区」总览与按工作区筛选、本机与远端 `agy` CLI 全功能及 Bark 推送、全平台运行状态与子代理防漏待用户验收。[提交 14dbc5f](https://github.com/lich13/agenthub/commit/14dbc5fe14e24d31ecc75401c8c34701aeac6e6c) · [Release](https://github.com/lich13/agenthub/releases/tag/v2.1.34) · [Release CI](https://github.com/lich13/agenthub/actions/runs/45389419356)
- **v2.1.33** 已发布八项资产。Linux 服务为 `agenthub-webd 2.1.33`，状态 `active`，健康检查 `ok`。Mac App 已由 2.1.32 原地更新并启动；Antigravity 在线／CLI 运行态、多级子智能体抽屉及统一清理待用户验收。[提交 e8ebad5](https://github.com/lich13/agenthub/commit/e8ebad55a2a48d7f87624da78e29b8f28c3e38fc) · [Release](https://github.com/lich13/agenthub/releases/tag/v2.1.33) · [Release CI](https://github.com/lich13/agenthub/actions/runs/45322327152)
- **v2.1.32** 已发布八项资产。Linux 服务为 `agenthub-webd 2.1.32`，状态 `active`，健康检查 `ok`。Mac App 已由 2.1.31 原地更新并启动；本机与远端 `agy` CLI 会话管理及 `Antigravity.app` 2.34.0 在线交互待用户验收。[提交 2f18fc0](https://github.com/lich13/agenthub/commit/2f18fc0dfa228a37761856be0dbc6ed80fdcae7f) · [Release](https://github.com/lich13/agenthub/releases/tag/v2.1.32) · [Release CI](https://github.com/lich13/agenthub/actions/runs/45268133405)

- **v2.1.31** 已发布八项资产。Linux 服务为 `agenthub-webd 2.1.31`，状态 `active`，健康检查 `ok`。Mac App 已由 2.1.30 原地更新并启动。[提交 a1401a7](https://github.com/lich13/agenthub/commit/a1401a7d29163b8838c23d90e8a398eb25227bd6) · [Release](https://github.com/lich13/agenthub/releases/tag/v2.1.31) · [Release CI](https://github.com/lich13/agenthub/actions/runs/44788221382)
- **v2.1.30** 已发布八项资产，具备两种密文格式的读取能力，继续写旧格式。Linux 服务在该版部署后状态为 `active`，健康检查 `ok`。Mac App 已由 2.1.29 原地更新并启动。[提交 3fb2cdf](https://github.com/lich13/agenthub/commit/3fb2cdf361a5f62198bfd168bcd13132c4c57aec) · [Release](https://github.com/lich13/agenthub/releases/tag/v2.1.30) · [Release CI](https://github.com/lich13/agenthub/actions/runs/44777552143)

## v2.1.42 实现与验证

1. **全平台会话运行状态根治与跨平台内核进程探针（`platform.rs` + 六大 Provider）**：
   - **跨平台内核级进程／打开文件探针**：新增 `platform::{pid_is_alive, resolve_kernel_exe, resolve_kernel_args, resolve_kernel_process_identity, resolve_kernel_cwd, collect_kernel_open_vnodes}`，macOS 通过 `libproc`（`proc_pidinfo(PROC_PIDVNODEPATHINFO)`、`PROC_PIDLISTFDS`、`proc_pidpath`、`sysctl(KERN_PROCARGS2)`）直接读取原始 UTF-8 `cwd`、`argv` 与打开普通文件描述符，Linux 通过 `/proc/<pid>/{exe,cwd,cmdline,fd,stat}` 直读普通文件并排除 `"Z"`/`"X"`/`"x"` 僵尸与垂死态，彻底消除 `ps` 将中文目录（如 `/程序开发/`）转义为 `\347\250...` 导致的路径匹配失败，以及 `lsof` 64KiB 输出截断与反向 DNS 阻塞（`lsof -n -P -F pfn` 仅保留为降级兜底）。
   - **Claude Code（C1–C5）**：无显式 `sessions/<pid>.json` 时按同 `cwd` 已打开 `.jsonl`、显式 `--resume`/`-r`/`--session-id`（历史未结会话未产生新事件前保持 `Ownership::Unknown` 阻止改名且不误判 `running`）、唯一开回合（`turn_open || has_open_questions`）与最近写入时间唯一绑定归属，不再整目录一刀切打成 `unknown`；放宽版本目录进程校验至受信任安装前缀下的 `versions/<ver>`；按已核验 `native_status` 判定实时状态（`"busy"` 结合 `statusUpdatedAt` 收敛陈旧状态、`"waiting"` 及未决开问题统一输出 `"waiting"`、`"idle"` 在无未决开问题时收敛 `"recent"`），并在流式未换行尾行（`has_partial_tail`）且无前置致命错误时保持真实运行／等待态免闪烁 `"unknown"`（改名与通知扫描继续保留半行门禁）；同 `sessionId` 多残留 `sessions/<pid>.json` 时忽略已死 PID；只读列表与详情复用 2s TTL `READONLY_SNAPSHOT_CACHE` 并在缓存过期时通过 `CAPTURE_SINGLEFLIGHT` 互斥锁单飞同步采集。
   - **Codex 与子智能体（X1–X2）**：补齐 `thread-writer-locks/<id>.lock` Writer 排他锁（`probe_writer_locked`）、已打开 rollout vnode 及宿主 Worker 存活核验（兼容 Linux 15 字节截断 `comm` 与 Node 包装器、排除纯 `codex app-server --managed-daemon`），无 `turn_complete` 尾部实时判定 `running`；子智能体在开回合中遇到流式半行（`info.partial`）时保持 `Running` 防闪烁。
   - **Grok Build、Antigravity（A1–A3）、DSH 与 Probe 全平台态势**：Grok Build 补齐 `kill(pid, 0)`／`ESRCH` 存活性与僵尸剔除并按活跃开回合或最近更新唯一判定 `running`；Antigravity 并发枚举同 UID（含 Linux `root` 服务按 `ANTIGRAVITY_HOME` 属主 UID 跨用户核验）全部在线 Language Server 端点（`LiveEndpointSet`）合并会话并按归属端点路由详情／改名／归档／删除（未水合冷会话优先经在线端点处理、收到 `404` 回退离线元数据，永久删除遍历全部在线实例），离线 CLI `presence/*.lock` 继承 `probe_advisory_lock` + `3600s` 无心跳崩溃兜底，在线轨迹空响应不再静默伪装已结束；DSH 结合 `session.lock` 与未结回合／提问／审批输出 `running`/`reply_needed`/`waiting`/`failed`；`ProbeLiveOverview` 完整纳入 DSH 与本机 Grok Bot 活跃及待处理会话。
2. **`$de-gpt` 深度减法与六大 Provider 基础设施收口**：彻底移除工作区与 `agenthub-webd` 退役 `axum` 依赖，删除 6 个零逻辑转发空壳文件（`codex/mutations.rs`、`webd/src/{audit.rs, api/claude.rs, api/cleanup.rs, api/grok.rs, api/sessions.rs}`），清理 Probe「高级设置」三个后端不消费的无效输入框（`Hook 事件行数`、`冷却行数`、`日志上限 MiB`，Swift DTO 保留 `decodeIfPresent` 默认值、服务端保留旧请求与存量配置无损兼容）及 Rust／Swift 零引用死分支；将通知与定位缓存收口至共享 `JsonlCache`（补齐读前前缀锚点与并发尾追加兼容）／`ReadCache`／有界 LRU，合并 Antigravity 5 源父子图指纹缓存、`history_navigation::paginate_window`、`services/search` 零分配 `contains_folded_str`、Hook 命令生成与防符号链接保权限原子替换。
3. **服务端大响应流式透传、SQLite WAL 节流与增量缓存优化**：SSH RPC 服务端改用 `write_raw_response` 三段流式写出帧头与 `RawValue` 字节并引入单连接 `Semaphore::new(16)` 背压，FFI `remote_invoke` 改用 `read_raw_response`（`Box<RawValue>`）原文透传至 Swift `RpcSink`；`store_probe_error_monitor_status` 在状态／游标／扫描行数／新增异常变化时立即写库、完全空闲未变 `"ok"` 心跳按 300s 节流写库，单轮 daemon tick 在 `question_monitor`／`async_question_monitor`／`provider_monitor` 间共享一次 `Arc<Config>`（实际投递前再读取最新凭据，无新异常时跳过重读），`probe_error_monitor` 改用行值索引 `(ts, ts_nanos, id) > (?1, ?2, ?3)` 与条件读取正文，`maintain_probe_events` 合并为单次 `Immediate` 事务并幂等补建 `idx_jobs_kind_started_at`；`session_storage::scan` 复用 `canonical_root` 与 `WalkDir` 元数据并用 `HashSet<(u64, u64)>` 硬链接去重；Codex 分页 `DetailState` 省去常驻 `Vec<CodexMessage>`（`limit.is_none()` 按需生成）与未读 `PendingToolCall.payload`、消除 `MessageBlockBuilder` 两处 `O(N²)` 扫描并为 `subagents::read_graph` 接入 `StampedCache`；Claude `SUMMARY_CACHE` 改为 `JsonlCache` 增量累加与惰性 `SeenUuid`，跳过 `summary_only` 与检索中的无用附件 `lstat`；Antigravity `.db` 下推 `step_type` 过滤并改用零分配 `WireFieldIter`；补齐 `SecretBox` 预扩展 `Aes256Gcm`、`Zeroizing` CI token、`OnceLock<reqwest::Client>`、Codex `valid_history_cursor` 校验与多处 `O_NOFOLLOW` + `(dev, ino)` 防护。
4. **macOS Swift 6 界面打磨与 120Hz 阅读器局部刷新**：补齐 Claude 与 Grok 顶栏 `reply_needed`、`waiting`、`failed`、`unknown` 状态胶囊及跨平台「工作区」带机器修订号与请求序号守卫的左栏即时状态回写；精确核算 `296pt` 列表顶栏（`276pt` 内容对齐），完整展示 `DeepSeek Harness`、`Antigravity`、`Claude Code` 零截断；全端 `cwd` 路径统一 `DesignFonts.body(11.5)` + `.kerning(-0.001)` + `.truncationMode(.middle)`；修复选中行左上左下圆角毛刺、未展开 Claude 禁改名提示单行化、设置页重复标题、全部 `36pt` 输入框整框 `@FocusState` 聚焦热区、`HoverSummaryRowView` 悬停残留、「上下文」披露卡右对齐锚定、`<880pt` Probe 窄窗断点、多行问答选项与回答动态高度测量及 `TimelineRailView` 首帧高度；`ConversationTableCoordinator` 对纯尾部追加与流式末行裂变走局部 `insertRows`/`reloadData(forRowIndexes:)`，宽屏缩放且阅读列保持 `768pt` 时先 `tile()` 再仅更新可见行边距约束跳过全表重测，代码块保留 ASCII 快速估算并对非 ASCII 行逐行实测字形宽度取最大值，GFM FFI 返回带长度切片并在切机／淘汰时裁剪 `GfmLayoutCache`。

本机 macOS 同一合成负载在改动前（基线 `8eba404` `v2.1.41`）与改动后（`v2.1.42`）预编译后顺序各测一次，夹具生成不计入耗时／CPU，临时基准代码与基线 worktree 已移除：

| 合成负载／指标 | 改动前（`v2.1.41` `8eba404`） | 改动后（`v2.1.42`） | 关键状态／内存／效率变化 |
| --- | ---: | ---: | --- |
| `session_storage::scan` 6 层目录（1,024 文件＋340 子目录）×30 | `626.43 ms`（内核 `sys` `501.46 ms`／总 CPU `624.28 ms`） | **`216.36 ms`**（内核 `sys` **`179.40 ms`**／总 CPU `216.34 ms`） | 复用 `canonical_root`＋`WalkDir` 元数据，耗时下降 **65.5%（2.90×）**，内核系统调用 CPU **-64.2%** |
| SSH RPC `22.39 MiB` 大响应出站＋入站透传 ×5（独立子进程） | `4,086.97 ms`（CPU `4,068.70 ms`），峰值 RSS **`314.48 MiB`**（增量 `+254.89 MiB`） | **`2,167.85 ms`**（CPU `2,161.82 ms`），峰值 RSS **`174.16 MiB`**（增量 `+114.60 MiB`） | 三段流式写出＋`Box<RawValue>` 免 DOM 透传，峰值 RSS 省 **`140.32 MiB`（-44.6%，增量 -55.0%）**，耗时 **-47.0%（1.89×）** |
| Claude `18.04 MiB`（1,600 条记录）长会话单行追加列表刷新 ×15 | `25,909.30 ms`（CPU `25,545.83 ms`） | **`7.64 ms`**（CPU **`7.64 ms`**，单次 `0.51 ms`） | `SUMMARY_CACHE` 改为增量 `JsonlCache`＋惰性 `SeenUuid`，耗时下降 **99.97%（3,391×）** |
| 空闲 300s 窗口 `PanelDb` WAL 写入（19×15s 心跳＋2×120s 未决标记） | `.sqlite-wal` 增长 **`78,312 B`**（19 次写事务），`1.222 ms` | `.sqlite-wal` **`0 B`（0 次写事务）**，`0.560 ms` | 300s 内未变 `"ok"` 心跳节流＋未决标记去重，空闲窗口 WAL 写入 **100% 消除** |
| Swift 阅读器 300→340 行纯尾部追加轮询 ×40 | `420.24 ms`（CPU `424.17 ms`，单轮 `10.51 ms`） | **`25.63 ms`**（CPU `23.03 ms`，单轮 **`0.64 ms`**） | 局部 `insertRows` 免全表历史高度重测，耗时下降 **93.9%（16.4×）** |
| Swift 阅读器宽屏 `1160↔1440pt`（保持 `768pt` 列宽）缩放 ×40 | `1,023.34 ms`（CPU `1,023.10 ms`，单帧 `25.58 ms`） | **`13.56 ms`**（CPU `13.55 ms`，单帧 **`0.34 ms`**） | 仅更新可见行左右边距约束跳过全表重测，耗时下降 **98.7%（75.5×）**，稳守 120Hz 帧预算 |
| 本机 Release 产物（`agenthub-webd`／`libagenthub_ffi.a`／Swift WMO stripped `agenthub`） | `13,027,616 B` / `110,184,432 B` / `17,382,576 B` | `13,160,112 B` / `111,046,392 B` / `17,523,240 B` | 纳入跨平台内核进程／vnode 探针、多实例路由与流式 RPC，本机二进制分别变化 `+1.017%` / `+0.782%` / `+0.809%`（正式标签 CI 八项发布资产待打标生成后补录） |
| 仓库代码变更规模（不含文档） | 基线 `8eba404` | `138` 文件（`+10,159 / -4,630`，净删 6 个空壳文件与 `axum` 依赖） | 收口重复实现并补齐内核探针、流式 RPC、120Hz 局部刷新与回归／光栅单测 |

- **自动化验证证据**：`codex-ask review --base origin/main` 审查闭环（采纳 11 项阻断／重要修正并补齐回归断言），`cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets --locked -- -D warnings`、`cargo test --workspace --locked`、Release `libagenthub_ffi.a` + Swift Debug／Release `-O -whole-module-optimization` 构建、`AGENTHUB_RENDER_TESTS=1 swift test --package-path macos`（138 项 XCTest 与浅深色全场景离屏 PNG 光栅及非空白像素断言全部通过）、`node scripts/check-wire-types.mjs`、`node --test scripts/test-wire-types.mjs`、`node scripts/check-parity-vectors.mjs`、`bash scripts/test-install-script.sh`、`python3 scripts/check-versions.py --tag v2.1.42`、`python3 scripts/check-workspaces.py`、`node --test scripts/test-security-audit.mjs`、`python3 scripts/privacy-check.py --git-objects` 与 `git diff --check` 全部通过。

## v2.1.41 实现与验证

1. **`$de-gpt` 死代码清理与重复实现收口（P1／P2）**：删除 Probe 无数据库分支（`ProbeRuntime::status`／`status_with_options`、`aggregate_probe_status_with_options`、`open_readonly_panel_db`、`recent_probe_event_count`、`table_exists`）、Antigravity 只写不读字段（`ParsedExecutorMeta`、`parse_executor_metadata_blob`、`last_user_input_step_index`、`SubagentNodeMeta.sqlite_status`，保留 `cli_meta.rs` `executor_metadata` 强类型 SQL 校验）、仅剩测试调用的 `PlatformPaths::for_kind_with_home` 与 `ProbeEventInput::hook_stop`、退役标识专属断言及 Swift 零引用测试挂钩（`cachedCellCountForTesting`、`cachedCellForTesting`、`isAnimatingForTesting`、`readingHorizontalInsets` 未读纵向分量）与约 30 处退役 WebUI／CSS／`.ts`／`.tsx` 注释（保留协议常量 `desktop_embedded_tauri`）；精简 `axum`（关闭 `macros`）、`uuid`（关闭 `serde`）、`tokio/test-util`（仅留 `agenthub-webd` dev）与 `agenthub-ffi` 直接 `rustls` 依赖（共用 `agenthub_core::http_client_builder`）；收口三平台只读 `flock` 探测为 `file_stamp::probe_advisory_lock`、删除 `webd/src/api/provider_sessions.rs` 10 个转发壳并收口 `ffi/src/commands/dispatch/providers.rs` 12 个分发器共用辅助、合并 Antigravity 原生会话 SQLite 开库与 `status/not_fully_idle/raw_summary` 投影、合并 Claude 中断清理／Codex `user_input`／去重组件／`sha256_hex`／`github_ci` 仓库比对／`ADMIN_ID` 常量／`JobUseCases` 与 `ThreadUseCases` 入口／`config::write_bytes_atomic_0600`（FFI 显式保持 `0600`）及 Swift `SettingsPaginationBar`。
2. **六项空闲负载与可靠性缺陷修复（P3：B1–B6）**：
   - **B1（GitHub CI 空闲轮询重写状态）**：`github_ci::scan_due` 在与已存储切片逐项比对前先执行 `retain_summaries(&mut incoming)` 统一按 `(updated_at DESC, id DESC, run_attempt DESC)` 排序，消除创建顺序与更新顺序不一致导致的每 60s `github_ci_state` 全量重写，同时保持在途运行结束与空闲快照后新完成运行的通知语义不变。
   - **B2（WAL 读缓存 `ctime` 误失效）**：新增 `WalReadStamp`、`FileStamp::sqlite_read_pair`／`sqlite_read_pair_followed`、`FileStamp::without_ctime` 与 `optional_sqlite_pair_stamp`（主库完整比对，`-wal` 仅比对设备／inode、长度、纳米 `mtime` 及有界边缘哈希），应用于 `ProbeErrorSourceStamp::read`、Antigravity `CONV_DB_META_CACHE`／`OFFLINE_DB_STEPS_CACHE`／`CONV_DB_STEPS_CACHE`／`STORE_AUX_INDEX_CACHE` 及 `HISTORY_CACHE.read_sources` 离线 `.db-wal` 历史回退路径，消除 Linux `root` 只读开库时 SQLite `fchown(-wal)` 仅推进 `ctime` 导致的每轮缓存失效（Codex `verified_detail` 父链复核仍维持完整 `sqlite_pair`）。
   - **B3（只读锁探测吞错与门禁闭环）**：`probe_advisory_lock` 返回 `io::Result<bool>`（`NotFound -> Ok(false)`、`EWOULDBLOCK/EAGAIN -> Ok(true)`、`EINTR` 重试、非普通文件或其他 IO 错误显式报错）；写门禁（Codex `ensure_ids_not_running` 含 `table_columns` SQL 错误传播、Antigravity `ensure_offline_unlocked`／在线未水合目标删除前后锁校验／预检与执行门禁、DSH 主会话及子会话删除预检）转为目标失败，统一清理对已确认孤立子会话保留多跳缺失祖先链复核并将执行期锁探测 IO 错误逐项返回 `Failed`（预览继续排除不可验证项），纯列表／详情状态展示显式保留 `unwrap_or(false)`。
   - **B4（结构损坏与坏密文不阻断原生通知队列）**：`pending_native_deliveries_with_retired` 与 `recover_interrupted_native_deliveries` 改用 `ValueRef` 逐行流式读取（支持按 Provider 先过滤再解密，隔离跨源坏行），将非 `BLOB` 列类型、缺失密文／`nonce` 或非法 `nonce` 长度归为无条件结构损坏立即收尾，而 AEAD 解密失败仅在同轮存在可解密记录时收尾为 `failed`／账本终态或 `unknown`、清空密文并保留 `event_key` 去重身份（整轮无可解密记录时照常报错且零改错密钥密文），逾时 `delivering` 收尾在 `Immediate` 事务内原子查询 Bark 账本且不伪造 Probe 事件，仅在发生坏行收尾时记单条不含内容的 `warn`。
   - **B5（作业后备启动失败遗留永久 `running`）**：`jobs::spawn_job_task`／`spawn_fallback_job_with_builders` 在无 Tokio runtime 环境下的后备线程或单线程 runtime 创建失败时调用 `finalize_failed_job` 标记作业 `failed` 并释放 `JobRunner` `running` 互斥组。
   - **B6（附件缩略图失败无提示且视口无限重发）**：`AttachmentViewportManager` 新增 `failedKeys` 并在 `LoadedAttachmentState` 纳入 `failedLowMask`／`failedOverflowIndices`（覆盖 `>= 64` 下标），回调前先清理 `inFlightTasks`，RPC 失败或图像解码失败时立即触发行重配显示「图片不可用」并在视口内跳过重复请求，滚出视口（含同会话跳转至无图窗口触发 `evictOutside`）或切换作用域（`resetScope`）后清除记录允许重试。
3. **执行效率、前端去噪一致性与 CI 优化（P4／P5／P6）**：`sessions.search` 对 Grok Bot 采用读前后双重 `FileStamp` 核验、仅缓存截断后线程 ID 集合（省略全量 JSON 克隆与逐条体积统计）并在每条读取与返回前复核当前账户（中途切户立即丢弃重试防串号）；`ProviderWorkspaceStore.fetchSessionDetail`、`SubagentDrawerView` 与 `RuntimeConnectionStore` 2s 作业轮询仅在值变化时赋 `@Published` 属性；主线程 `JSONDecoder` 实测 Release WMO 下 500 条列表 `1.762 ms`、单页 120 块详情 `0.492 ms`（均 `< 2 ms`，按计划不拆 `Task.detached`）；`UnifiedWorkspaceHubStore.availableWorkspaceGroups` 改为惰性按需计算，5 处高频 `NSRegularExpression` 收口为静态常量；Probe 合并重复发送提示常量、移除能力胶囊旁重复等宽字与零状态重复句，`UnifiedWorkspaceHubStore.FeedbackNotice` 以显式布尔字段表达错误态，统一 `SessionStatusLabels`、「复制答复／已复制答复」、Plan 缺省标题「计划」与子智能体「`N 个运行中 · N 个已完成 · N 个创建中`」摘要，同步 `DESIGN.md` 8 项色值与 `22px` 时间线轨宽；`.github/workflows/ci.yml`（`fb9a66a`）将三类缓存拆分为仅在 `main` 写入的 `restore/save`、移除 `cargo-audit` 键中的 `Cargo.lock` 与 macOS 缓存中的 `macos/.build`，清理 `v2.1.39`／`v2.1.40` 标签作用域 5 项遗留缓存（`5.78 GB`），并新增 `scripts/ci-code-tree-hash.sh` 在非标签运行命中 `macos-verified-<hash>` 时跳过 macOS 原生构建测试步骤。

线上 Linux 生产服务（部署前 `v2.1.40` `a4a8339` vs 部署后 `v2.1.41` `de0ee8c`，不含前序未发布本地提交 `8771d50`、`22025e9`）、本机合成负载（基线 `22025e9` vs `de0ee8c`）与 GitHub Actions CI 实测前后对照如下（临时取样脚本已清理）：

| 实测场景／指标 | 改动前 | 改动后（`v2.1.41` `de0ee8c`） | 关键状态／效率变化 |
| --- | ---: | ---: | --- |
| 线上服务 130s 空闲 `github_ci_state`（`13,831 B`）写库 | `a4a8339`：`0→60→130s` 连续 **2 次重写** | `0→60→130s` **0 次变动**（哈希恒定） | 空闲 CI 状态重复全量写库 **100% 消除** |
| 线上服务常驻普通 15s tick `rchar`／`read` 次数 | `a4a8339`：`99.6–116.0 KB` / `15–19` 次 | **`87.2 KB` / `11` 次**（130s 总 `rchar` `-21.4%`、`write_bytes` `-36.9%`） | 错误监控在 `logs_2.sqlite-wal` 未变时不再每轮重开库 |
| 线上服务只读 RPC `antigravity.list` ×20（13 会话） | `a4a8339`：读 **`191.09 MB`**（`7,220` 次 `read`），CPU `290 ms`／墙钟 `372.1 ms` | 读 **`674.2 KB`**（`700` 次 `read`），CPU `170 ms`／墙钟 `267.9 ms` | 消除 root `fchown(-wal)` 缓存失效，读取量下降 **99.65%（283×）**，CPU **-41.4%** |
| 本机 Antigravity 30 个离线 WAL `.db` `list_sessions` ×20 | `22025e9`：`168.24 ms`（CPU `168.24 ms`） | `72.63 ms`（CPU `66.39 ms`） | 耗时下降 **56.8%**、CPU 下降 **60.5%** |
| 本机 Grok Bot `sessions.search` 120 线程 ×20 | `22025e9`：`876.42 ms`（CPU `875.96 ms`） | `598.50 ms`（CPU `541.95 ms`） | 耗时下降 **31.7%**、CPU 下降 **38.1%** |
| 本机 Swift 未变详情轮询 ×20 `objectWillChange`／主线程解码 | `22025e9`：`60` 次 / `1.762 ms`（列表）、`0.492 ms`（详情） | **`0` 次** / `1.741 ms`、`0.501 ms` | 未变轮询零重发布；主线程解码 `< 2 ms` 保留原线程 |
| GitHub Actions `cargo-audit`／`Backend` 作业耗时（锁文件变更轮） | `v2.1.40`：重装 `167s` / Backend `467s` | `de0ee8c`：命中缓存 **`0s`（跳过重装）** / Backend **`237s`** | `cargo-audit` 键解耦 `Cargo.lock`，Backend 耗时 **-49.3%** |
| GitHub Actions macOS 缓存体积／缓存总量／标签污染 | `v2.1.40`：`2.35 GB` / 总量 `11.55 GB`（含 5 项标签缓存 `5.78 GB`） | 冷存 `551.9 MB`（`2.1.41` 存 `805.9 MB`，还原 `18s`）/ 总量 **`9.60 GB`** / 标签缓存 **`0`** | 移除 `macos/.build` 缩减 **65.7%–76.5%**，总量回落至 10GB 配额内 |
| 仓库变更规模（`22025e9..de0ee8c`） | `334` 文件 / `198,009` 行 | `335` 文件 / `200,698` 行（`+4,250 / -1,561`） | 删减 `1,561` 行死码／重复壳，新增单测与 B1–B6 边界防护 |

下表比较 v2.1.40 与 v2.1.41 对应标签 CI 生成并正式发布的同类资产：

| 发布资产 | v2.1.40 字节（`a4a8339`） | v2.1.41 字节（`de0ee8c`） | 变化 |
| --- | ---: | ---: | ---: |
| macOS ARM64 DMG（Release CI） | 16,529,505 | 16,569,545 | +40,040（+0.242%） |
| macOS updater（Release CI） | 15,434,394 | 15,469,191 | +34,797（+0.225%） |
| Linux x86_64 webd（Release CI） | 7,364,329 | 7,386,763 | +22,434（+0.305%） |

- **自动化验证证据**：`codex-ask review --base origin/main` 审查闭环（采纳 7 项阻断／重要边界修正，`cli_meta.rs` 强类型 SQL 按计划保留），`cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets --locked -- -D warnings`、`cargo test --workspace --locked`、Release FFI + Swift Debug／Release WMO 构建、`AGENTHUB_RENDER_TESTS=1 swift test --package-path macos`（全量 XCTest 与离屏渲染断言通过）、`bash scripts/test-install-script.sh`、`python3 scripts/check-versions.py --tag v2.1.41`、`python3 scripts/privacy-check.py --git-objects`、GitHub Actions `Backend & contract checks`＋`macOS Swift 6 checks`＋`Release` 八项签名资产校验、远端 `agenthub-webd 2.1.41` 部署健康检查与本机 `AgentHub.app 2.1.41` 签名安装启动核验全部通过。Swift 侧全量审计未经 Codex（仅经本地静态／渲染测试与代码核查）。

## v2.1.40 实现与验证

1. **GitHub CI 有界内部滚动与 Probe 核心设置默认展开**：新增兼容 macOS 12 的 `BoundedPassthroughScrollView`（`NSScrollView` + `NSHostingView`），当内容高度 `<= maxHeight` 时紧贴实际高度零多余留白，当 `> maxHeight` 时锁定高度上限、支持内部触控板惯性滚动，并在首尾触边及外层已启动的飞掠惯性（`momentumPhase`）下无缝透传 `nextResponder`，同时按 `scrollView.contentView.bounds.width` 自适应系统滚动条宽度确保右侧时间戳零截断；`GithubCiCardView` 顶部筛选收敛为桌面单行 `36pt`，最多 20 条运行列表限高在 `260pt`（展开态卡片高度由 `1,432.0 pt` 降至 `387.0 pt`），收起且未开启 CI 监控时跳过 15s 轮询并单遍归并 `catalog`/`visibleRuns`；`ProbeDisclosureRules` 默认展开 `[.ci, .bark, .hosts]`，在已开启总开关的平台卡片内联展示 `22pt` 事件细项胶囊开关（`完成`／`需回复`／`需授权`／`失败`），压缩 `Bark` 与 `Hooks 与宿主接入` 纵向冗余，将 `refreshAll` 改为四路并发＋单调令牌防乱序，对齐 `.settings`/`.bark` 子集校验边界，保持 `高级设置` 及 `全平台实时态势`、`最近事件`、`执行记录` 默认收起。
2. **`$de-gpt` 全仓减法清理与共用基础设施收口**：删除无生产调用的 `ffi/src/services/jobs.rs`、`webd/src/api/jobs.rs`、`PanelDb::claim_probe_dedupe`／`claim_feedback_delivery`、`UpdateFailureCategory`、`ProbeFixedJobSpec.command`／`fixed_probe_shell_command`、退役 `auto_reply`／`hidden_desktop_control` 墓碑字段与 Swift 侧 `CodexMessage`、`PendingElicitation`、`DeletionEligibility`、破损未用正则等死代码，移除重复测试专用 `finish_uncompacted` 并统一复用生产 `snapshot_uncompacted`，将 `ThreadStateActionPlan` 的字符串动作匹配收口为强类型 `ThreadStateAction`；新增 `crates/agenthub-core/src/file_stamp.rs` 统一 6 套散落文件指纹为 `FileStamp`、`open_regular_nofollow`（`O_NOFOLLOW` + 常规文件校验）与带容量／空闲淘汰的 `StampedCache<K, V>`（替换 Antigravity 手写哈希缓存）；合并 Swift 16 套局部请求包装为共用 `RPCRequestEnvelope<T>`，并收紧 `agenthub-{core,ffi,webd}` 内部符号可见性。
3. **macOS Swift 6 视觉细节、算法内存与连接竞态修复**：按 `DESIGN.md` 新增 `palette.userBubble`（浅 `#F3F5F8`／深 `#262D3D`），为 `AppKitRunningRingView` 补齐窗口挂载态连续旋转动画（离屏快照保持静态确定性）与 `AccessibilityMotionObserver` 令牌持有；`TimelineRailView` 改用激活刻度坐标转换防长会话内部滚动预览漂移，披露行与刻度补齐键盘焦点环与手型指针；围栏代码块改为对称 `12pt` 内边距＋右上角悬停／聚焦半透明复制按钮；`SubagentDrawerView` 补齐 `HistoryLoadingBarView`、空状态、可下钻条目右箭头与未加载时间线跳转；`WorkspaceSidebarSearchField` 接入 `IMEGuardedInlineTextField` + `150ms` 防抖，弹窗关闭恢复 `firstResponder`；新增 `O(1)` `BoundedLRUCache` 统一替换排版／上下文／预览／披露缓存，为会话历史增加 `8 会话 / 16 MiB` LRU 上限（含 `userMessage` 请求正文、附件与 `contextBlocks` 字节计重）并在窗口隐藏时回收离屏单元格池与光栅缓存，修复 `ConversationRowCellView` `cachedRowId` 复用双键冲突、行内／围栏代码内未闭合 `<!--` 污染、`CodexAccountScopeSection`／`MaintenanceOpsView`／`RemoteConnectionSettingsView` 的切机 `revision` 竞态，以及 `UNUserNotificationCenter`／`NSWorkspace` 后台回调队列在 Swift 6 SE-0423 下误触发 `@MainActor` 断言崩溃（拉起新包失败时回跳 `@MainActor` 提示手动重新打开应用）。
4. **服务端与六大 Provider 降耗、安全边界与全量 LTO**：精简工作区与 `agenthub-{core,ffi}` 未用 `tokio` 特征，Release 开启 `lto = true`（保留 unwind）；`ReadCache<T>` 直接返回 `Arc<T>`，移除 `grokbot::SLICES` 重复 `32 MiB` 缓存，为 `session_storage::CACHE`、多槽有界 `LIVE_PROVIDER_SESSIONS_CACHE` 与 `O(1)` LRU `claude::LOCATED_SESSIONS` 补齐 300s 空闲淘汰；全新空库启用 `PRAGMA auto_vacuum = INCREMENTAL` 并在每小时维护执行 `incremental_vacuum(256)`，审计修剪按小时节流，循环 SQL 改用 `prepare_cached`，`stage_native_notifications_with_precheck` 在入库 `IMMEDIATE` 事务内严格保留当前配置复核，`ssh_server` 发送 `> 1 MiB` 响应后标记空闲 `malloc_trim(0)`；Antigravity 离线转录改为单次打开＋按回合滚动缓冲与末条延迟格式化（消除整文件 `Vec<Value>` 峰值）、离线 `.db` 倒序读末回合、仅在无锁／无读错时缓存完整 `StoreAuxiliaryIndex` 并在单会话／清理路径复用内存目录与结构化 `history.jsonl` 清理；Codex 将 `resolve_workspace_root` 移至归档与可见性筛选之后、单点边表改用 `EXISTS`、详情块免克隆归并并补齐 `.jsonl.zst` 压缩会话通知／提问扫描；Claude 后台通知改走轻量增量 `ClaudeProbeSnapshot` 免冲刷前端 `DETAIL_CACHE`；统一全平台纳米时间戳排序键，补齐 `read_attachment`／`dsh::rename_session` 等 `O_NOFOLLOW` + `(dev, ino)` 校验与保权限原子写回、macOS 更新作业互斥、`JobRunner` `Stdio::null()` 及 SSH `rpc-stdio` 三阶段取消安全（写帧中途取消立即关通道）。

本机 macOS 开发测试构建（`test` / `swift test` 未优化 profile）与 Release 构建，同一合成负载在改动前（基线 `d6c31c7`）与改动后（`v2.1.40` 最终源码）顺序各测一次，夹具创建不计入耗时／CPU，临时基准代码已移除：

| 合成负载／产物 | 改动前（`d6c31c7`） | 改动后（`v2.1.40`） | 关键状态／内存／体积变化 |
| --- | ---: | ---: | --- |
| Antigravity `31.7 MiB` 长转录（150 轮 × 30 步）`session_detail` ×3（独立子进程） | 1,963.33 ms（CPU 1,962.65 ms） | 992.51 ms（CPU 991.96 ms） | 耗时与 CPU 下降 **49.5%**（**1.98×**）；子进程峰值 `ru_maxrss`：**`65.34 MiB → 58.02 MiB`**（省 **7.32 MiB**，消除整文件 `Vec<Value>` 堆积） |
| Codex 320 线程（280 条 `.git` worktree 已归档＋40 条活跃）列表 ×30 | 277.53 ms（CPU 277.39 ms） | 110.50 ms（CPU 109.28 ms） | 归档与可见性筛选后再解析 `workspace_root`，下降 **60.2%–66.0%**（首轮实测 `94.48 ms`） |
| Claude 72 会话 15s 后台通知快照＋20 活跃详情轮询 ×15 | 460.63 ms（CPU 460.40 ms） | 315.62 ms（CPU 314.02 ms） | 轻量 `ClaudeProbeSnapshot` 免淘汰重读前端 `DETAIL_CACHE`，下降 **31.5%–37.9%**（首轮实测 `286.12 ms`） |
| GitHub CI 10 仓库运行列表响应解析 ×40（回环 HTTP） | 263.49 ms（CPU 249.02 ms） | 258.52 ms（CPU 243.83 ms） | 直解强类型 `RunsEnvelope` 省去全量 `serde_json::Value` 中转（末轮后台调度波动下为 `326.95 ms`） |
| Grok 80 会话 `limit = 20` 列表热读 ×40 | 84.05 ms（CPU 84.01 ms） | 84.01 ms（CPU 83.81 ms） | 保持每轮单次原生进程 `ps` 快照与 `truncate` 后计算交接路径（进程表负载较高时末轮为 `173.94 ms`） |
| Swift `GithubCiCardView` 20 条运行展开高度（`1020pt` 宽） | 1,432.0 pt | 387.0 pt | `BoundedPassthroughScrollView(maxHeight: 260)` 有界内部滚动，卡片高度压缩 **73.0%**（省 **1,045.0 pt**） |
| Swift 阅读器 100 条富文本＋表格＋代码构建测量 ×20（每次新建协调器） | 4,322.76 ms（CPU 4,347.19 ms） | 4,291.36 ms（CPU 4,250.03 ms） | 单轮内 `configure` 复用 `heightOfRow` 测量（新建 20 个独立协调器实例；末轮系统负载下为 `5,015.01 ms`） |
| 本机 Release `agenthub-webd`／Swift WMO stripped `agenthub` | 13,719,520 B / 17,856,728 B | 12,994,432 B / 17,314,104 B | `lto = true` 下 `agenthub-webd` 缩小 **725,088 B（-5.29%）**，Swift WMO stripped 缩小 **542,624 B（-3.04%）** |

下表比较 v2.1.39 与 v2.1.40 对应标签 CI 生成并正式发布的同类资产（未测量线上常驻 RSS 或用户历史会话磁盘回收）：

| 发布资产 | v2.1.39 字节 | v2.1.40 字节 | 变化 |
| --- | ---: | ---: | ---: |
| macOS ARM64 DMG（Release CI） | 16,845,326 | 16,529,505 | -315,821（-1.875%） |
| macOS updater（Release CI） | 15,689,495 | 15,434,394 | -255,101（-1.626%） |
| Linux x86_64 webd（Release CI） | 7,441,239 | 7,364,329 | -76,910（-1.034%） |

## v2.1.39 实现与验证

1. **Probe 控制台去臃肿与智能可折叠重设计**：新增共用非嵌套双区顶栏 `CollapsiblePanelCard`（左侧全矩形披露 `Button`＋右侧平级动作区，折叠态零挂载正文），删除全部 18 张装饰重复 `MetricCard`，顶部仅常驻精简 `通知配置`（`高级设置` 默认收起，仅在当前面板校验失败或点击时展开），将 `GitHub CI`（含 `36pt` 开关栏与可展开搜索仓库选择器）、`Bark`、`Hooks 与宿主接入`（5 平台卡片合一，按当前机器能力动态汇总就绪计数）、`全平台实时态势`、`最近事件` 与 `执行记录` 全部收敛为 `48pt` 单行摘要披露卡（健康默认态文档总高由 `2,396.0 pt` 降至 `662.0 pt`，`< 680pt` 单屏呈现）；新增 `ProbeDisclosureRules` 按 `(machineScope, section, issueSignature)` 仅在新异常出现时自动展开一次（手动收起后跨 15s 轮询保持闭合）；`ProbeSettingsDraft` 拆分为 `ci`、`settings`、`bark` 三个独立校验与局部补丁提交子集，保存与 Hook 确认操作严格绑定捕获时的 `revision` 并在切机首帧清空旧状态；修复 `DangerButtonStyle(soft: false)` 深色对比度（`palette.buttonFg`，深浅色均 `>7.7:1`）、关闭态 `CapsuleSwitch` `1pt` 边框（`>5.3:1`）及 `logMaxMiB` 超大有限浮点数强转陷阱。
2. **`$de-gpt` 全仓死代码清理与契约目录对齐**：删除零生产调用的 `contract_dtos.rs`、`codex/name_client.rs`、`codex::hidden_and_archived_thread_ids` 及重复门面测试 `architecture_plans.rs`／`use_case_facades.rs`（独有断言并入服务单测），拍平单文件目录 `ffi/src/services/threads.rs`；在 `services/settings.rs` 用 `#[serde(flatten)]` 拍平 31 字段 Probe 配置合并，收口 `JobUseCases`、`enrich_probe_status_from_db` 与双端只读 `UseCases` 参数解码；显式分离 Grok 直接／批量删除（不按活动或旧 `fingerprint` 阻断）与统一清理（逐项复核 `Inactive` 与 `grok_fingerprint`）；在 `contracts/agenthub-contract.json` 与 `contracts/wire-types.json` 登记 `ClaudeDeleteResult`、`GrokDeleteResult` 并修正 `claude.deleteExecute`／`grok.deleteExecute` 返回 DTO；清理 Swift 端未引用辅助方法、假兼容 wire 键与自对比像素测试，重命名 `InlineSearchIMETests.swift`。
3. **服务端 CPU、峰值内存、SQLite 占用优化与后端隐患修复**：为 Antigravity 离线双源（`.db` 含 `-wal` 与转录文件）建立带 `FileStamp` 的有界解析缓存（仅缓存 `Ok` 读取并将瞬时锁／读错记入 `scan.errors`）并透传已解析 `ParsedRawSummary`，离线删除改用单 ID 原子 SQLite 事务、确保磁盘移除后继续清理共享索引并支持主存储已删后的幂等重试；新增 `codex::notification_thread_headers_with_identities` 在单次只读 `state_5.sqlite` 连接中按 `thread_source`/`source_kind` 非空归并批量读取任务身份并由 15s 监控循环共享，将每轮最高 400 次开库降至 1 次；`codex::list_threads` 移至 `truncate(limit)` 后计算体积并提供 `list_threads_without_storage`；`sessions.search` 改用 `visit_search_blocks`／`visit_search_events` 借用缓存零克隆检索与单遍 `char_indices()` CJK 免全文小写扫描；DSH 明文与多帧 `.zstd` 改为定长缓冲 `DCtx` 增量流式解码（消除整帧压缩缓冲与重复帧长重扫，保留 `for_append` 逐行 JSON 语法与半帧拒绝校验），`claude.list` 冷读改走 `SUMMARY_CACHE` 轻量归约且 `JsonlCache::evict_idle` 保留 `lineage` 纪元环；补齐空闲轮询只读预检短路、更新作业 `latest_job_by_kinds` SQL 前置过滤、`user_version = 3` 下幂等补建 `idx_probe_events_dedupe_created` 等 3 项局部索引、在 `wal_checkpoint(TRUNCATE)` 前落库 `probe_event_retention_last_run` 使截断后 `.sqlite-wal` 归零，并补齐 `archive::hidden_visibility_rows` 的 `thread_spawn_edges` 子线程分类；修复 DSH `sha256` 64 位 ASCII hex 校验、Codex 提问原子认领入队（移除测试专用 `notification_delivery::codex` 并统一走 `codex_with_claims`）、来源复核显式区分 `ErrorKind::NotFound` 与瞬时权限错误、`due_notification_deliveries` 跨轮回绕游标分页越过 `1,000+` 不可读行（既避免正常通知饿死，又在错密钥打开时原样保留密文并返回错误）、Grok 统一清理批次逐项复核 `Ownership::Inactive`、`question_monitor` 瞬时读错保留旧指纹，以及 Claude `ps`→`lsof` 间短命进程已退出（`ESRCH`）引发的误判。
4. **macOS Swift 6 前端提速、窄窗适配与异步竞态修复**：`UserContextEngine` 改用连续 UTF-8 偏移扫描并提供含 `contextBlocks` 单条 `256 KiB` 上限与总计 `8 MiB` LRU 预算的 `projectAndSegmentUserMessage`，补齐侧栏／工作区分组快照缓存、`120Hz` 滚动 `userAnchorRowIndices` 二分查找、品牌图标 `CGPath` 缓存与 GFM 解码失败不污染 `GfmLayoutCache`；补齐 `1000×680` 窄窗用户多图流式折行与 `SessionSearchModalView` 自适应高度、`makeCapsulePill` 动态圆角、详情顶栏长标题／路径换行及改名失败保草稿；补齐 `ProbeSettingsSyncState` 单调读取栅栏防保存后乱序旧响应回滚草稿、缺失作业发布终态记录解除忙态、`ThreadMessageStore` 块收缩重置、`TimelineWindowController` 逐页 `epoch` 与空页止停、全量附件（含 `>=64` 下标）`LoadedAttachmentState` 单行刷新、`ConversationTableCoordinator` 生命周期解耦及子智能体 `navigationGeneration`／列表 `listFetchSeq` 保序。

本机 macOS 开发测试构建（`test` / `swift test` 未优化 profile），同一合成负载在改动前（基线 `f9ecbed`）与改动后（`v2.1.39` 最终源码）顺序各测一次，夹具创建不计入耗时／CPU，临时基准代码与基线 worktree 已移除：

| 合成负载 | 耗时 ms：前 → 后 | CPU ms：前 → 后 | 关键状态／内存／布局变化 |
| --- | ---: | ---: | --- |
| Codex 200 线程 15s 通知头＋身份扫描 ×20 | 658.35 → 18.40 | 657.99 → 18.39 | 单次只读连接批量查询＋双监控共享，下降 **97.2%**（**35.8×**） |
| Antigravity 离线双源 50 会话（25 `.db` + 25 转录）轮询 ×20 | 263.36 → 97.25 | 263.26 → 97.22 | `FileStamp`（含 `-wal`）双源缓存命中，下降 **63.1%**（**2.71×**） |
| DSH `64 MiB` 16 帧 `.zstd` 长行日志流式读取 ×3（独立子进程） | 4,680.39 → 3,830.06 | 4,678.36 → 3,829.04 | 子进程峰值 `ru_maxrss`：**`408.84 MiB → 115.58 MiB`**（下降 **71.7%**，省 **293.26 MiB**） |
| `PanelDb` 全关空闲轮询 ×60／启用空闲 ×60 | 4.38 → 0.93 / 3.62 → 1.35 | 4.38 → 0.93 / 3.62 → 1.35 | 只读预检短路立即写事务（下降 **78.8%**／**62.7%**）；维护截断后 `wal_bytes = 0` |
| `sessions.search` 50 缓存长会话 ×120 块 CJK 热搜 ×10 | 1,766.32 → 1,488.42 | 1,765.29 → 1,487.10 | 借用缓存零克隆＋CJK 免全文小写扫描，下降 **15.7%**（200 会话超 64 项缓存上限时两侧均约 2.79s 受磁盘重读主导） |
| Swift 200 条 `24 KiB` 多包装用户消息投影＋分段（冷读 1 遍＋热读 4 遍） | 1,930.88 → 329.57 | 1,930.07 → 329.40 | 冷读 `393.12 → 229.17 ms`（-41.7%），热读 `1,537.77 → 100.40 ms`（-93.5%），合计下降 **82.9%**（**5.86×**） |
| Swift `ProbeWorkspaceView` 初始布局 ×20（`1084×820`） | 333.71 → 138.18 | 335.93 → 139.76 | 耗时下降 **58.6%**（**2.42×**）；默认健康态文档高 **`2,396.0 pt → 662.0 pt`**（**`< 680pt`** 单屏呈现） |

下表比较 v2.1.38 与 v2.1.39 对应标签 CI 生成并正式发布的同类资产（未测量线上常驻 RSS 或用户历史会话磁盘回收）：

| 发布资产 | v2.1.38 字节 | v2.1.39 字节 | 变化 |
| --- | ---: | ---: | ---: |
| macOS ARM64 DMG（Release CI） | 16,423,927 | 16,845,326 | +421,399（+2.566%） |
| macOS updater（Release CI） | 15,316,022 | 15,689,495 | +373,473（+2.438%） |
| Linux x86_64 webd（Release CI） | 7,274,827 | 7,441,239 | +166,412（+2.288%） |

## v2.1.38 实现与验证

1. **彻底移除本机 Codex 桌面连接对话全链路**：净删 **46 个整文件**（`crates/agenthub-core/src/codex/desktop*`、`services/{codex_desktop,credentials}.rs`、`db/desktop_submissions*`、`crates/agenthub-ffi/src/codex_desktop*`、`commands/dispatch/collaboration.rs`、`macos/Sources/AgentHubCore/CodexDesktop/*` 及专属测试与向量夹具），下线 `codex.desktop.*` 全部 9 个动作（未知命令由正向白名单直接拒绝，桌面命令分发由 `73` 项收敛为 `64` 项）与 `codex_desktop` 能力位，移除 `agenthub_ffi.h` 4 个桌面流／粘贴／拖放 C ABI 导出、`agenthub-ffi` 的 `keyring`／`url` 依赖及 `macos/Package.swift` 的 `ImageIO`／`CoreServices` 链接框架；清理侧栏 `+` 新建空线程按钮、底部悬浮协作输入卡与右键菜单「在 Codex 中打开」入口，阅读列底部恢复全屏舒展。新 SQLite 数据库不再建立 `codex_desktop_submissions` 表，存量升级库不执行 `DROP TABLE` 并保留既有钥匙串项以无损兼容 **v2.1.30** 回退（`v2.1.38+` 全新库回退时旧版发送队列缺表不可用，会话阅读与通知正常）；100% 保留 Codex 会话只读阅读、子智能体、搜索、附件、Plan 导出、改名、归档／恢复、直接删除、统一／隐藏清理、账号隔离解除与 Probe 通知监测。
2. **`$de-gpt` 去重、孤立辅助清理与可见性收口**：提取 `WorkspaceGroupedSessionRows`、`WorkspaceArchivedExpandedSection`、`WorkspaceArchivedClosedFooter` 与 `WorkspaceEmptyMatchPlaceholder`，消除 `UnifiedProviderWorkspaceView` 与 `UnifiedWorkspaceHubView` 平行复制的工作区分组列表与归档抽屉模板，移除 `SessionBatchBarAndDialogView` 3 个未读死属性、`RuntimeConnectionStore.isWindowFocused` 未读发布属性（仅窗口可见性实际变化才通知根 Store）及协作浮层遗留的 `WindowFullContentModal`；清理无生产调用的 `security::hash_token`、`CleanupUseCases::unified_execute` 与 `isolation.rs` 退役常量；在 `ProviderWorkspaceStore` 收口 `makePollSignature`、`shouldSkipUnchangedDetailPoll` 与 `applyRenderedDetailSnapshot` 统一四类详情刷新管道，并将最新助手回复提取由全量逆序 `.compactMap` 改为命中首条即停；收紧 `agenthub-core`（`system`、`workspace_root`、`cleanup`）与 `agenthub-webd` 内部模块可见性。
3. **服务端与 FFI 的 CPU、内存、磁盘 I/O 与二进制体积优化**：`agenthub-webd` 仅对常驻 `Serve` 与 `Probe MonitorErrors` 启用 `worker_threads(2)` 多线程运行时，其余短时 CLI／Hook 子命令统一改用 `new_current_thread()`，并将 `probe_passive_scan_threads_with_paths`、`notification_thread_headers`、`scan_codex_turn_errors`、`async_question_monitor`（在阻塞任务内按线程即时过滤基线后仍待处理的提问）、`question_monitor`、`provider_monitor`（含空闲淘汰与原生扫描）及 `ci_notifications` 本地读取全部经有界 `read_tasks::spawn_blocking` 卸载；`agenthub-ffi` 常驻运行时收敛为 `worker_threads(2)`；`antigravity.rs` 改为 `OnceLock<reqwest::Client>` 复用连接池并为 `lsof` 补齐 `-nP` 抑制反向 DNS／端口名解析；修正 Codex `SCAN_CACHE` 单条虚高 `8192B` 基重为真实结构＋嵌套字符串／`AsyncQuestionTracker` 堆容量计重（使 2,000 活跃线程完整常驻 `8 MiB` 预算内免抖动重扫），消除 `rollout_pending_feedback` 逐行 `HashMap::clone` 与全文小写分配并将 `rollout_has_confirmed_inactive_state` 改为 `BufReader` 逐行原始 JSON 流式校验；将 `session_index` 缓存接入 5 分钟空闲淘汰，并在实际发生缓存淘汰时于 Linux glibc 下触发 `malloc_trim(0)` 归还空闲页（空轮询零锁竞争）；`sessions.search` 移除热路径 `serde_json::to_value` 递归建树与非可见内部标识字段，改为 `MessageBlock`／`ClaudeHistoryEvent`／`GrokHistoryEvent` 强类型可见语料构建、`strip_memory_metadata` 标记字节零分配快路径与 ASCII 大小写不敏感无分配预筛；`PanelDb` 配置 `cache_size=-2048`、`temp_store=MEMORY`、`journal_size_limit=4194304`（保留默认 `synchronous=FULL`）并幂等新增 `idx_native_probe_claude_session` 局部索引（存量索引增至 7 项），`cli_meta.rs` 限制 `cache_size=-512` 并单次读取 `sqlite_master` 表集合；工作区 `[profile.release]` 启用 `lto = "thin"` 与 `codegen-units = 1`。
4. **macOS 前端渲染、算法复杂度与潜在缺陷修复**：修复 `UnifiedProviderWorkspaceView.init` 在 `@StateObject(wrappedValue:)` 外急切构造 `ProviderWorkspaceStore` 的非惰性分配缺陷并在生命周期钩子同步外部回调与侧栏开关；`UnifiedSessionSummaryItem` 预计算 `updatedAtMs` 配合 `normalizePath` 纯 ASCII 规范路径零分配快路径消除排序与工作区分组热路径的 `ISO8601DateFormatter` 锁竞争；`VisibleMarkdownEngine.visibleMarkdown` 增加连续 UTF-8 缓冲标记快速返回，`ExecutionGroupingEngine` 移除循环内动态正则编译并在追加命令前移出分组所有权，彻底消除引用计数=2 引发的 `O(K²)` 写时复制重扫；`ConversationTableCoordinator` 改为仅对高度实际变化的行调用 `noteHeightOfRows`，`toggleDisclosurePreservingAnchor` 与 `scrollToPositionKey` 校验共享 `ActivityDisclosureStore` 修订号基线后按 `rowMayReferenceDisclosure` 精确局部刷新受影响行（外部修订越界时自动全量重算防陈旧行高残留）并在切会话与 `deinit` 时取消 `highlightClearTask`；GFM 表格改用单层自绘 `TableGridDocumentView` 配合 `ReaderSelectableTextView` 复用测量栈（超长文本测后自动清空释放）与单次排版高度读取；修复 `RunningIndicator` 在辅助功能「减少动态效果」切换或复用时的动画复位问题，并为 `BackdropGaussianBlurView` 开启 `masksToBounds = true`。

本机 macOS 开发测试构建（`test` / `swift test` 未优化 profile），同一合成负载在改动前（`v2.1.37` 基线 `3ea6891`）与改动后（`v2.1.38` 最终源码）顺序各测一次，夹具创建不计入耗时／CPU，临时基准代码已移除：

| 合成负载 | 耗时 ms：前 → 后 | CPU ms：前 → 后 | 关键状态变化 |
| --- | ---: | ---: | --- |
| Codex 2,000 线程活动扫描热读 ×5（10,000 次 `enrich_thread_from_rollout`） | 3,426.93 → 135.47 | 3,423.39 → 135.41 | 真实计重驻留 `8 MiB` `SCAN_CACHE` 免抖动重扫，下降 **96.0%**（**25.3×**） |
| `sessions.search` Codex 跨会话 200×60 事件热搜 ×5 | 4,531.90 → 3,032.37 | 4,516.92 → 3,030.58 | 强类型语料＋`strip_memory_metadata`／ASCII 快路径，下降 **33.1%** |
| `sessions.search` Claude 跨会话 200×40 事件热搜 ×5 | 2,571.13 → 1,701.36 | 2,544.13 → 1,700.51 | 消除 `serde_json::to_value` 建树与无效小写分配，下降 **33.8%** |
| Swift 跨平台 1,000 会话 `sortMergedItems` + `buildGroups` ×50 | 6,222.48 → 241.40 | 6,202.92 → 241.27 | 预计算 `updatedAtMs`＋ASCII 规范路径快路径，下降 **96.1%**（**25.8×**） |
| Swift 400 行长对话单过程组 `toggleDisclosurePreservingAnchor` ×20 | 1,909.13 → 39.25 | 1,871.71 → 39.22 | 精确局部行高失效与局部重绘（保持 `0.0pt` 锚点锁定），下降 **97.9%**（**48.6×**） |
| Swift 50×6 GFM 表格（含行内 `code` 胶囊）单元格构建布局 ×20 | 3,392.16 → 2,691.22 | 3,430.32 → 2,737.45 | 单层 `TableGridDocumentView`（`307 → 1` 图层）＋免重复测量，下降 **20.7%** |

下表比较 v2.1.37 与 v2.1.38 对应标签 CI 生成并正式发布的同类资产（及本机 stripped 主二进制体积；未测量线上常驻 RSS 或用户历史会话磁盘回收）：

| 发布资产／二进制 | v2.1.37 字节 | v2.1.38 字节 | 变化 |
| --- | ---: | ---: | ---: |
| macOS ARM64 DMG（Release CI） | 18,928,496 | 16,423,927 | -2,504,569（**-13.232%**） |
| macOS updater（Release CI） | 17,485,385 | 15,316,022 | -2,169,363（**-12.407%**） |
| Linux x86_64 webd（Release CI） | 8,002,995 | 7,274,827 | -728,168（**-9.099%**） |
| 本机 `Contents/Resources/agenthub-webd`（stripped） | 15,827,392 | 13,268,576 | -2,558,816（**-16.17%**） |
| 本机 `Contents/MacOS/agenthub`（Release WMO stripped） | 21,714,080 | 17,214,096 | -4,499,984（**-20.72%**） |

## v2.1.37 实现与验证

1. **超大 Claude 会话向上滚动与 JSONL 确定性历史纪元**：`JsonlCache` 将随机 UUID `epoch` 改为基于 `(dev, ino, scope, compressed, start_offset, rewrite_gen, first_record_digest)` 与 256 项 `Lineage` 环的确定性派生，使超容量／LRU／空闲淘汰与尾部追加保持同一 `epoch`（前端分页历史不被误判 `reset` 清空），同时在 `rename`、原地截断、同长覆写与非换行尾改写时立即轮换 `epoch`；`claude/reader.rs` 保留物理 JSON 行号 `self.records`（确保既有 SQLite `native_probe_streams.cursor` 零回退），引入 `retained_records` 计入 `200,000` 上限与缓存权重，跳过 14 类丢弃遥测记录（`progress` 等）的 `seen` 写入并将去重键收敛为零堆分配流式规范 JSON `[u8; 32]` 摘要，去掉 `attachment` 双重 `detail` 拷贝并将 `file-history-snapshot`／`file-history-delta` 压缩为紧凑路径列表。AppKit 阅读器新增 `ReaderOuterScrollView`（`verticalScrollElasticity = .allowed`），修正 `NSTableView` 视口填充下 `renderedContentHeight`（取末行 `rect(ofRow:).maxY`），使单条折叠过程组（`34pt < 视口`）向上滚轮／方向键直接触发历史续拉、顶部橡皮筋回弹不误清 `upwardScrollIntent`、展开态首行前置翻页锁定可见底边 `maxY`，并在折叠尾页自动升级 `limit = 500` 有界回填至可见对话及为 `TimelineWindowController` 的 `w:`／`n:` 游标传递原始事件 ID。
2. **Codex 表格首行截断、行内代码换行胶囊、Markdown 段高与 `<external_codex_apps_selected_page_part_*>` 清理**：`makeTableView` 改用顶原点 `FlippedDocumentView`（`isFlipped = true`）由 `y = 0` 自上而下排布表头与数据行，并为需要横向滚动的表格与代码块在 `NSScroller.preferredScrollerStyle == .legacy` 下补足滚动条高度（`horizontalScrollerExtraHeight`）及监听滚动条样式切换；`measureTableLayout` 按扩展字素簇与中文避头尾标点禁则保护不可拆分的 ASCII／代码词元列宽下限（上限 `168pt`）并在其上按 `maxWidth` 比例收缩超宽文本列，仅当列下限之和超出视口时启用横向滚动；`ReaderSelectableTextView` 行内 `code` 圆角背景改为固定等宽字体度量逐行片段（`enumerateLineFragments`）对齐字形基线绘制（折行断口收窄圆角防切字），杜绝跨行折行时合并成单块错位胶囊或因字体回退跳变高度；统一 `.assistant`、`.plan`、多段代码块及 `makeMonospaceDetailBox`（`max(106, width)`）测量与 Auto Layout 高度，移除 `GfmRenderedSegment.prose` 死参数 `isQuote` 并将 `GfmLayoutCache` 命中更新改为单调序号 `O(1)`；前后端与跨语言黄金向量同步剥离 `<external_codex_apps_selected_page_part_i_of_n>Companion Page snapshot part i of n:...` 包装（保留代码块与引用字面量），快照专属消息不再产生空气泡或时间线锚点；`ConversationRowCellView.swift` 按职责拆分为核心、`MarkdownViews` 与 `ProcessViews` 三文件。
3. **`$de-gpt` 代码审计瘦身与前端／RPC 执行效率**：`ProviderWorkspaceStore` 对未变化轮询按 `LastRenderedPollSignature` 短路行重建；`RuntimeConnectionStore` 移除 `RequestControlProbe` 二次编解码与手工 JSON 拼接，统一静态编解码器与强类型 `RemoteInvokeEnvelope`；`FFIRuntimeBridge` 原地借用 UTF-8 缓冲解析 Markdown；收口全端重复 `ISO8601DateFormatter`／`DateFormatter` 为线程安全 `SharedTimestampFormatters`，为工具预览 JSON 解析补充 512 项有界记忆化，补齐 `ConfirmDialogOverlay` 与 `SubagentDrawerView` 的输入法 `hasMarkedText()` `Escape` 误关守卫；`agenthub-webd` 统一 `grok.list`、`grok.detail`、`claude.list`、`probe.events` 的 `rpc_payload` 强类型反序列化（拒绝非法 `limit`），收口重复 `table_columns`/`first_existing`，由 `Config::to_toml_string` 收口配置序列化并移除 `agenthub-ffi` 直接 `toml` 依赖。
4. **服务端与本机 CPU／内存／SQLite 占用优化**：Antigravity 合并 `collect_store_known_subagents` 与 `list_offline_summaries` 为每库单次扫描、按 `archived` 过滤提前跳过无关会话树状态评估、跳过无锁且超过 60s 未改动的历史子代理 `.db` 检查，并为 `.db`（含 `-wal`）与转录文件建立有界文件指纹缓存（接入既有空闲淘汰）；Codex 只读查询统一启用 `SQLITE_OPEN_READ_ONLY | SQLITE_OPEN_NO_MUTEX`，列表单次复用 `canonicalize(home)` 与同路径 `cwd` 的 `workspace_root` 解析；Claude `Snapshot::capture_readonly` 增加单飞互斥消除并发 `ps`/`/proc` 风暴；SQLite 保持 schema 3，在迁移锁内幂等补齐 `idx_audit_log_created_at`、`idx_jobs_started_at`、`idx_native_probe_deliveries_updated_at` 三个索引，例行维护保留最近 200 条作业（不截断保留项 `output`）并清理超期终态旧作业，`native_probe_streams` 游标仅在会话经确认显式删除／统一清理成功时移除，绝不按时间清理仍存在会话的游标。API 协议保持 2，未新增第三方依赖、数据库表、后台任务、状态文件或 CI 门禁。

本机 macOS 开发测试构建（`test` / `swift test` 未优化 profile），同一合成负载在改动前（基线工作树）后（最终源码）顺序各测一次，夹具创建不计入耗时／CPU，临时基准代码已移除。Claude 负载为 `209.7 MiB` JSONL（1,800 轮对话、45,000 条 `progress`、1,800 条 `file-history-snapshot` 及 `attachment`，冷读 ×3 独立子进程、热轮询 ×3、追加 2 行后轮询 ×3）；Antigravity 为桌面与 CLI 双库离线活跃列表热读 ×20；Codex 为 8 个 `.git` worktree 下 200 条线程列表热读 ×20；Swift 为 160 条事件运行中会话无变更轮询 ×50：

| 合成负载 | 耗时 ms：前 → 后 | CPU ms：前 → 后 | 关键状态变化 |
| --- | ---: | ---: | --- |
| Claude 209.7 MiB 冷读 ×3 | 43,624.03 → 16,491.28 | 43,417.04 → 16,357.63 | 单次解析耗时与 CPU 下降约 62.3% |
| Claude 209.7 MiB 热轮询 ×3 | 45,316.58 → 0.40 | 44,264.38 → 0.40 | `epoch_preserved`: `false` → `true`（驻留 `160 MiB` 预算内免重扫） |
| Claude 209.7 MiB 尾部追加轮询 ×3 | 44,832.78 → 13.51 | 44,578.18 → 13.47 | 命中增量偏移追加，不再整文件重扫 |
| Antigravity 双库活跃列表 ×20 | 849.86 → 467.15 | 848.66 → 466.88 | 单遍扫描＋指纹复用，耗时下降 45.0% |
| Codex 200 线程多 worktree 列表 ×20 | 359.81 → 154.69 | 359.38 → 154.62 | 免重复 `canonicalize` 与 `workspace_root`，下降 57.0% |
| Swift 160 事件无变更轮询 ×50 | 170.28 → 18.80 | 170.34 → 19.24 | `LastRenderedPollSignature` 短路行重建，下降 89.0% |

未测量线上 Linux 常驻 RSS、物理磁盘 I/O 或真实用户会话体积变化，不将开发构建合成基准外推为线上固定比例。下表比较 v2.1.36 与 v2.1.37 对应标签 CI 生成并正式发布的同类资产（不能声称安装包体积缩减，也不代表用户磁盘历史空间回收）：

| 发布资产 | v2.1.36 字节 | v2.1.37 字节 | 变化 |
| --- | ---: | ---: | ---: |
| macOS ARM64 DMG | 18,749,090 | 18,928,496 | +179,406（+0.957%） |
| macOS updater | 17,329,738 | 17,485,385 | +155,647（+0.898%） |
| Linux x86_64 webd | 7,940,938 | 8,002,995 | +62,057（+0.781%） |

## v2.1.29 实现与验证

v2.1.29 修复全平台清理的预览及确认、隐藏线程分页和切机后的数据隔离。原生工作区同时修复搜索定位、子智能体分页、Probe 活动、账号隔离、作业输出与计划导出。API 协议 2，数据库 schema 3。

[v2.1.29](https://github.com/lich13/agenthub/releases/tag/v2.1.29) 已发布八项资产，对应提交 [`733bfeb`](https://github.com/lich13/agenthub/commit/733bfebf54e0845b4cc394b5fe33a8d516192585)。[Release CI](https://github.com/lich13/agenthub/actions/runs/44668220771) 通过。Linux 服务为 `agenthub-webd 2.1.29`，状态 `active`，健康检查 `ok`。Mac App 已用同版产物原地更新并启动。

## v2.1.28 实现与验证

[v2.1.28](https://github.com/lich13/agenthub/releases/tag/v2.1.28) 的发布提交为 [`eafbcd4`](https://github.com/lich13/agenthub/commit/eafbcd4c31574f0ed4efe8b09af90fed85668c65)。[Release CI](https://github.com/lich13/agenthub/actions/runs/44588237182) 发布八项资产，Linux 服务与 Mac App 均已完成当次更新。

1. **Rust `crates/agenthub-ffi` 自包含收敛与旧端退役**：将全部零 Tauri 耦合模块正式迁入 `crates/agenthub-ffi/src/`，彻底移除 `src-tauri/`、`webui/`、`package.json` 及 Tauri／Playwright 专属脚本，保留 `agenthub-core` 与 `agenthub-webd` 核心逻辑、API 协议 2、SQLite schema 3、系统钥匙串与 73 项桌面命令 AST 契约守卫（`drift_guard` + `wire_contract`）。剪贴板项数（`<=32`）、UTF-16／字节上限、C `ImageIO` 尺寸验界与无损 PNG 转码、选档 `Semaphore(1)` 与 `300s` 超时取消、Minisign `"ED"` 验签更新器及 `pulldown-cmark` GFM AST 解析留在 Rust；所有 Host 回调非阻塞返回，串流 `StreamSink` 单调递增 `seq` 并保证单次 `closed` 帧与六步确定性 `shutdown`。
2. **Swift 6 宿主外壳、偏好迁移、Probe 与设置页**：采用单一 `macos/Package.swift`（`swift-tools-version: 6.0`、最低部署目标 `macOS 12.0`、零外部依赖）。完整嵌入 53 个 Lucide 向量图标（含缓存化 AppKit `NSImage` 光栅输出）与六平台品牌图标，首次启动只读迁移旧版桌面端主题与导航折叠态，隐藏创建 `1280×820`（`minSize 1000×680`）主窗口并在首次展示时单次最大化至 `screen.visibleFrame`（后续揭示与切机不重改尺寸，跨屏自动夹紧），行内模态／确认框／灯箱／`<960pt` 抽屉开启期间拦截 `Edit → Select All (⌘A)` 穿透底层会话列表；`1060pt` 居中容器对齐 GitHub CI、Probe 6 指标与默认展开 8 项高级参数及中文化 Bark 状态标签、设置下划线双分页、`本机 App 更新`／`远程服务更新`、全平台 6 范围清理与 Codex 账号隔离 100 条分页。
3. **六平台工作区、AppKit 阅读器、搜索与子智能体抽屉**：统一实现 Codex、Claude Code、Grok Build、Antigravity、DeepSeek Harness 与 Grok Bot 列表轮询、六平台精确右键／顶栏／`Ctrl+Click` 自绘主题浮层菜单（`AgentHubTaskMenuPresenter`，对齐 `12pt` 圆角、`15pt` 图标、`--er` 危险项红底悬停、禁用原因提示及点击外部单次穿透关闭，Codex 列表右键归档／恢复走预览确认弹窗）、活跃列表与底部「已归档」抽屉严格互斥选择域、多选批次操作列、会话行右端等宽容量文字及 `13pt` 轨道旋转指示器。核心阅读列采用 `NSScrollView` + 无表头 view-based `NSTableView` + 显式 TextKit 1 栈只读 `ReaderSelectableTextView`，配合内层横向代码块／表格与到顶输出框滚轮穿透（`PassthroughInnerScrollView`）、`O(1)` 增量行高缓存、128 项已渲染单元格复用签名短路、独立 `TimelineActiveTracker` 局部订阅及向上意图＋合并别名锚点恢复；完整对齐过程组／工具组／元数据组、`2pt` 左侧引导线、行内 `code` 圆角胶囊、文件链接复制图标、完整高度代码块、自然列宽表格、`64pt` 历史折叠卡、`.plan-cell`、问答卡、`AGENTS.md`／`上下文` 折叠卡、右侧悬浮复制按钮（`0.0pt` 行高占用）、`.subagent-summary` 与 `<960pt` 抽屉焦点陷阱及时间轴窗口跳转。
4. **本机 Codex 协作区、CI 收敛与两端 Minisign 签名**：侧栏标题行紧凑 `28×28pt` 新建线程图标按钮；仅未归档本机 Codex 主线程展示 `768pt` 对齐的 `24pt` 圆角悬浮协作卡（按显式打开意图连接，聚焦切换 `--e` 与 `--a` 边框），无内层套框 `ComposerNSTextView` 使用 `15pt`／`1.55` 行高与右下对角调高手柄（`54pt–160pt`），提交前校验 `<=131,072` UTF-16 与 `<=256 KiB` UTF-8；实作三层输入法组字守卫，完整衔接 `$`／`@` UTF-16↔UTF-8 引用区间、`⌘V` 原始剪贴板快照、原生文件拖放、可展开目标栏、只读连接提示及最高占视口 `32%` 可滚动待处理审批／纯圆环单选问答／密文问答。Linux `webd` 与 macOS 归档统一经 `scripts/sign-minisign-archive.py` 生成带版本 trusted comment 的 Minisign 分离签名，GitHub Actions CI 收敛为 `Backend & contract checks`（`ubuntu-24.04`）与 `macOS Swift 6 checks`（`macos-15`），保持八项正式发布资产名称与 `latest.json` 完全一致。

本机自动化验证（`2.1.28`）：

- 统一 Cargo 工作区（`agenthub-core`、`agenthub-ffi`、`agenthub-webd`）全部单测与契约集成测试、`cargo fmt`、`cargo clippy --workspace --all-targets --locked -- -D warnings` 及 `security-audit.mjs cargo` 全部通过。
- Swift Debug 与 Release `-O -whole-module-optimization` 编译及 `AGENTHUB_RENDER_TESTS=1 swift test --package-path macos` **106 项 XCTest 测试方法**全部通过：覆盖 5 组跨语言 Golden Vectors、`ParityManifest.json` 全部 73 组共 641 项基线用例映射、全六平台反向分页与 100 项分批操作、离屏几何／矩形热区／右侧悬浮复制 `0.0pt` 行高与反向分页像素锚点断言、浅／深色及五平台锁定快照（`SnapshotBaselines.json`）、全量业务消息 `ReaderSelectableTextView` 字形非空白墨迹断言，以及合成 `setMarkedText` 输入法组字零误送断言。
- 契约、签名与隐私门禁：`check-wire-types.mjs`、`test-wire-types.mjs`、`check-parity-vectors.mjs`、`check-versions.py --tag v2.1.28`、`check-workspaces.py`、`test-install-script.sh`、`package-macos-swift-arm64.sh`、`codesign --verify --deep --strict` 与 `privacy-check.py`（`--payload` 及 `--git-objects`）全部通过。

下表比较 v2.1.27 与 v2.1.28 对应标签 CI 生成并正式发布的同类资产（updater `.tar.gz` 体积减少 3.361%，DMG 因 UDZO 分块与原生二进制对齐略增 1.182%，不代表用户数据空间变化）：

| 发布资产 | v2.1.27 字节 | v2.1.28 字节 | 变化 |
| --- | ---: | ---: | ---: |
| macOS ARM64 DMG | 17,394,151 | 17,599,695 | +205,544（+1.182%） |
| macOS updater | 16,814,579 | 16,249,420 | -565,159（-3.361%） |
| Linux x86_64 webd | 7,515,316 | 7,514,808 | -508（-0.007%） |

待用户验收：原地升级后的正式版 `/Applications/AgentHub.app`（`2.1.28`）与远端 `agenthub-webd 2.1.28` 连接、六大平台真实历史会话阅读与向上滚动、右键菜单、本机 Codex 桌面协作及 Bark／macOS 系统通知实收。

## v2.1.27 实现与验证

Codex 0.168 起在新会话 rollout 首行写入 `account_scoped`，其历史与 resume 列表按该行隐藏其他账号创建的会话，文件和数据库不受影响。新增共享动作 `threads.accountScopeStatus`／`threads.accountScopeClear`、可缺省能力 `codex_account_scope` 和「更新与维护」中的「Codex 账号隔离」卡片：检查只统计，解除把首行中 Codex 写出的精确标记替换为等长空白，替换结果必须等于原 JSON 去掉该字段。明文文件原地写入并恢复 mtime，inode、长度和偏移不变；压缩文件重编码到同目录临时文件，核对摘要后在 Codex 协调锁内原子替换。锁文件只读打开且从不创建，线程锁只在协调锁内探测；回退副本在原始文件属于该线程且已无标记时才处理；两个入口都不进入符号链接的会话根目录。响应与审计不含路径和正文，不读取 Codex 凭据，不修改原生数据库。单次调用约 20 秒，界面在仍有进展时继续调用。

`probe.hooks.clear_account_scope` 默认关闭；开启后已安装的 Codex Stop Hook 在建立通知事件之前处理本回合线程及已登记后代的明文原始文件，失败不影响 Hook 输出。卡片中的开关在未安装 Hook 的机器停用。

审计修复：已被原生压缩的线程可归档／恢复；归档执行每批读取一次清单并逐项短事务复核；Codex 活动扫描与 Grok 摘要缓存条目上限提高到 2,000，字节预算不变；Probe 聚合去掉两次重复排序；大小扫描期间的追加不再发布不可用；Claude 定位先比对路径键。Codex 通知在确定跳过前不再计算 rollout 摘要；Hook 安装／移除以 toml_edit 就地编辑 Codex 配置并只在实际备份时返回路径；错误监控每轮上限由 100 行提高到 1,000 行。桌面启动立即返回远程连接视图并在后台握手，11 个同步命令移入有界阻塞任务池。WebUI 删除 Markdown 缓存中从不读取的语法树副本，搜索 200ms 防抖，改名失败后失焦不重发，删除成功先从列表缓存移除，更新完成状态写入带机器范围的查询键。未新增第三方依赖、数据库表、索引、后台任务、持久化状态或 CI 阈值。

本机 macOS 开发构建，同一运行时合成负载在改动前（基线工作树）后（最终源码）各测一次，夹具创建不计入耗时／CPU，临时夹具代码已移除。Codex 列表为 200 个约 64KiB 的 rollout，先冷读一次再计 20 次热读；归档为 2,000 个线程中预览后执行 100 项；Claude 为 2,000 个转录文件中首次定位路径序最后一个，耗时含两侧相同的进程表快照；通知为同一 64MiB rollout 的可恢复事件重复记录 20 次，均在入队前按重复跳过。

| 负载 | 耗时 ms：前 → 后 | CPU ms：前 → 后 |
| --- | ---: | ---: |
| Codex 列表热读 ×20 | 7,352.1 → 406.6 | 7,348.8 → 406.4 |
| Codex 归档执行 100 项 | 3,820.8 → 115.9 | 3,812.2 → 112.4 |
| Claude 首次定位 | 2,781.5 → 64.4 | 2,738.9 → 22.9 |
| Codex 重复跳过通知 ×20 | 27,503.618 → 4.341 | 27,486.803 → 4.343 |

未测量 RSS、线上 Linux 负载、磁盘读写或真实数据，其余修复不作性能声明。Codex 活动扫描每项至少计 9KiB，8MiB 预算至多驻留约 910 个线程，超出部分仍逐轮重新解析；Markdown 缓存改按原文计重后同一预算可保留更多条目，不代表内存下降。

独立审查对照 Codex `rust-v0.170.0` 源码逐条核对后未发现损坏或丢失会话数据的路径，据其意见修正六处：被终止的解除遗留的临时副本由下一次解除清理（副本存续期间持有文件锁）；Stop Hook 不再穿过符号链接的会话根目录，Codex 数据目录本身为符号链接时与手动入口同样可用；时间预算改在扫描后起算；压缩文件的线程锁改在协调锁内探测；明文文件写入后恢复 mtime 失败不再误报为未解除；原始文件的 `payload.id` 不属于该线程时不放行回退副本。保留的低风险边界：Codex 的维护锁文件尚不存在时两次并发解除不互斥，各自复核、不会写坏文件；压缩文件在复核与替换之间、以及按路径重新打开之间仍有需刻意竞速才能触发的窗口；替换压缩文件不复制 ACL 与扩展属性；只检查 4MiB 以内的首行。

已知边界：解除改变 rollout 首行，该线程正在等待重试的 Codex 通知按来源已变处理、不再发送（重试窗口最长 10 分钟）；Codex 原生压缩维护进行中手动动作返回忙碌；Codex 的 resume 账号校验若日后启用，解除后列表可见但恢复仍可能被拒绝；自动开关不处理压缩文件和回退副本。

受影响 Core／webd／Tauri 模块完整测试、两套 Clippy 与格式、WebUI 类型检查与完整 Vitest、wire／契约检查、版本、工作区、隐私、安装脚本及发布说明生成检查通过。用例覆盖首行判定各分支、明文清除后的长度／inode／mtime 与追加写入、压缩文件内容／权限／mtime 与临时文件清理、遗留副本清理、线程锁与维护锁占用、回退副本、符号链接文件／根目录／数据目录、时间预算、子线程树、审计只记数量，以及本次构建 CLI 的 Stop Hook 开关两种行为。Playwright 用例随标签 CI 的前端检查通过。Claude 忙碌等待用例的假进程由 `/bin/sleep` 副本改为 ad-hoc 签名副本：macOS 27 会在启动后数毫秒内终止搬离系统卷的平台二进制，基线在本机 30 次中失败 9 次，修正后 40 次全部通过；产品逻辑未改。

下表比较 v2.1.26 与 v2.1.27 对应标签 CI 生成并正式发布的同类资产；三项均略增，不能声称包体积或用户数据占用下降。

| 发布资产 | v2.1.26 字节 | v2.1.27 字节 | 变化 |
| --- | ---: | ---: | ---: |
| macOS ARM64 DMG | 17,266,672 | 17,394,151 | +127,479（+0.738%） |
| macOS updater | 16,695,992 | 16,814,579 | +118,587（+0.710%） |
| Linux x86_64 webd | 7,493,793 | 7,515,316 | +21,523（+0.287%） |

待用户验收：「更新与维护 → Codex 账号隔离」检查并解除后，`codex resume` 列表出现此前隐藏的会话，远程机器重复一次；开启「回合结束后自动解除」后新会话跑完一回合再检查应为 0；归档／恢复一个已被 Codex 压缩的旧线程；远程不可达时启动 App 立即出现界面。真实界面、Codex 列表与通知实收未由助手代验。

## v2.1.26 实现与验证

Probe 的 Linux／Tauri 新鲜状态和被动扫描复用原配置文件缓存，CLI 状态、Hook 检查绑定实际 --config 路径。手动刷新、保存完成和 Hook 作业终结请求服务端新鲜状态，普通轮询继续缓存；配置错误明确返回。设置保存原有事务内磁盘重读保持。

压缩历史与普通 JSONL 共用各模块原容量预算、普通文件与原生身份检查，未变来源不再解压或解析，变化后完整重建，超预算正常读取但不缓存。详情删除重复分页实现并提供既有历史元数据，摘要移除 64MiB 截断；子线程复用原生压缩／归档路径定位，保留父链、符号链接、附件和前后身份核验，不写原生索引。Codex 8MiB 摘要和 Grok 2MiB 元数据缓存接入已有五分钟空闲淘汰。

批量首请求失败保留选择且只显示错误；部分成功后中断将当前批次记为结果未确认、后续批次记为未执行，保留全部未完成选择，不重发。Codex Hook Stop／completion 共用逐行读取，保持回合范围、问题／计划优先级、行号、候选数及读取错误行为。未新增依赖、数据库表、后台任务、持久化状态、解压长度门禁或 CI 阈值；Claude 独立后台完成与显式待处理规则保持。

本机 macOS 开发构建，同一运行时合成负载在改动前后测量；补齐子线程归档时间字段读取后仅复测压缩历史组，下表使用最终源码结果。主／子文件分别含 6,000 条约 1KiB 消息并压缩；列表、分页和子线程热读各 20 次。通知单独进程，20,000 条约 1KiB 消息，两个选择入口各 5 次。夹具创建不计入耗时／CPU；冷读只表示应用缓存冷，OS 文件缓存保留。

| 负载 | 耗时 ms：前 → 后 | CPU ms：前 → 后 |
| --- | ---: | ---: |
| 压缩列表冷读 | 181.896 → 193.240 | 181.707 → 192.678 |
| 压缩详情首次读取（摘要已热） | 883.496 → 712.320 | 883.190 → 710.460 |
| 压缩列表热读 ×20 | 3,707.151 → 5.658 | 3,703.815 → 5.659 |
| 压缩分页热读 ×20 | 18,134.189 → 8.606 | 18,108.286 → 8.606 |
| 压缩子线程首次读取 | 1,181.583 → 777.110 | 1,175.445 → 776.987 |
| 压缩子线程热读 ×20 | 23,350.874 → 37.926 | 23,305.994 → 37.866 |
| 通知正文选择 ×10 | 4,751.920 → 4,832.587 | 4,723.652 → 4,787.721 |
| 历史缓存空闲淘汰 | 0.051 → 4.143 | 0.051 → 4.143 |

压缩历史整组进程峰值 RSS 为 98,009,088 → 122,978,304 字节，新增快照复用使此负载峰值增加；通知进程为 33,325,056 → 10,452,992 字节，通知耗时／CPU 未降低。峰值包含测试框架及夹具，不能作为每次操作独立内存值。空闲释放用零期限调用既有入口，实际生产仍为五分钟：当前两份 Codex 摘要计重 18,432 → 0 字节，一份 Grok 元数据 456 → 0 字节；计重归零不代表 RSS 或磁盘空间回收。未测量线上 Linux CPU／RSS、物理磁盘读写或空间回收。

下表比较 v2.1.25 与 v2.1.26 对应标签 CI 生成并正式发布的同类资产；三项均略增，不能声称包体积或用户数据占用下降。

| 发布资产 | v2.1.25 字节 | v2.1.26 字节 | 变化 |
| --- | ---: | ---: | ---: |
| macOS ARM64 DMG | 17,250,835 | 17,266,672 | +15,837（+0.092%） |
| macOS updater | 16,688,512 | 16,695,992 | +7,480（+0.045%） |
| Linux x86_64 webd | 7,484,369 | 7,493,793 | +9,424（+0.126%） |

受影响 Core 缓存／Codex／Claude／Grok／native probe 模块、webd（含本次构建 CLI Hook 测试）、Tauri 设置入口及相关通知、分页、框架与 wire 契约集成测试通过；前端刷新、批量、分页、作业和切机测试及类型检查、两套 Clippy、格式、版本、工作区、源文件隐私、安装脚本及发布说明生成检查通过。用例覆盖自定义路径、外部原子配置替换与损坏、129MiB 压缩历史尾部及附件／搜索、旧索引定位、文件替换／截断／同大小同 mtime、缓存预算与空闲释放、首批失败及 150／201 项中断。[标签 Release CI](https://github.com/lich13/agenthub/actions/runs/43758972251) 的前后端检查、两端打包与发布边界检查通过，Tauri 集成测试因夹具未写入配置文件失败；仅修正测试夹具的 [`6a7d61d`](https://github.com/lich13/agenthub/commit/6a7d61de5854b70d88b199b5a8e940f32e3efa7e) 已通过本机该集成测试、格式与隐私检查，以及全部 [main CI](https://github.com/lich13/agenthub/actions/runs/43810502542)，包括完整回归、浏览器与 Tauri 检查；产品源码与标签一致。八项正式资产复用原标签构建结果，保留标签且未重新构建；服务端已通过部署脚本升级为 agenthub-webd 2.1.26，Mac 使用同版正式 DMG 从 2.1.25 原地更新并启动。

待用户验收：打开压缩主／子线程，检查分页、时间线、搜索与附件；Probe 修改开关或安装／移除 Hook 后状态应立即一致；批量中断显示已完成及全部未完成项。真实界面、业务与通知实收未由助手代验。

## v2.1.25 实现与验证

Codex 子智能体接受既有稳定分页、刷新及时间线游标，保持父链、文件身份、符号链接与读取前后核验。JSONL.ZST 的身份、状态、创建任务名、嵌套详情和附件共用解析；压缩历史不再在解压到 128MiB 时静默终止，完整流式读取，不生成磁盘副本。状态及任务名缓存接入既有五分钟空闲释放，容量和活跃快照规则保持。

关闭 Probe 后普通 Bark 待发项结束并清除正文，重新启用推进既有基线；显式 Bark 测试只受 Bark 开关控制。错误监控重新启用清除旧游标，首次扫描基线覆盖其前全部错误，包括重新启用后到首次扫描之间的错误；关闭错误监控仅取消对应待发项。设置保存事务同步 Codex 普通反馈及异步问题基线，扫描和 Hook 在同一事务内读取配置，来源复核只读取基线，保存后新问题可立即进入通知。

Linux 与 Tauri 共用原有文件指纹缓存，设置读取刷新内存；Linux 扫描及等待读取同一配置，原有两秒唤醒检查保持。服务启动配置路径贯穿设置、Hook 作业及生成的命令，Hook 保存原子替换并保留权限，后台作业终结刷新设置查询。Codex、Antigravity、DSH 批量处理中详情写入和改名框共用忙碌状态；多批执行中断保留前序已确认结果，停止后续批次且不重发不明请求。

单条 Codex 定位改用主键查询，三处通知来源复核不再构造整张线程列表，发现范围保持。Grok 删除预览批内只枚举一次工作区，执行继续重新定位所有同 ID 目录并保留逐项错误。没有新增依赖、协议字段、数据库表、索引、后台任务或 CI 门禁。

本机 macOS 开发构建，同一合成负载在改动前后各测一次。计时与进程 CPU 包围业务调用；2 万条 Codex 头部中执行 100 次单 ID 定位及 25 次通知来源定位，Grok 为 100 个工作区中 40 个目标的 10 次预览，缓存为 50 个文件各 100 个创建调用。临时夹具代码已移除。

| 负载 | 耗时 ms：前 → 后 | CPU ms：前 → 后 |
| --- | ---: | ---: |
| Codex 单 ID 定位 ×100 | 190.503 → 14.521 | 181.752 → 14.419 |
| Codex 通知定位 ×25 | 2416.911 → 2.939 | 2229.973 → 2.918 |
| Grok 批量预览 ×10 | 97.829 → 51.390 | 97.427 → 51.375 |
| 子智能体缓存加载与空闲淘汰 | 93.292 → 87.729 | 91.369 → 87.573 |

按相同顺序串行执行整组负载，进程累计峰值 RSS 为 349,175,808 → 19,054,592 字节，包含测试框架及夹具，不能将它作为每项操作的独立内存收益。测试调用现有淘汰入口、使用零空闲期限，改动前保留 1,464,700 字节计重，改动后归零；生产仍为五分钟，计重释放不等于 RSS 或磁盘空间回收。未测量线上 Linux CPU／RSS 或物理磁盘写入。

下表为对应标签 CI 构建并正式发布的资产；压缩包体积变化不代表用户历史或数据库空间回收。

| 发布资产 | v2.1.24 字节 | v2.1.25 字节 | 变化 |
| --- | ---: | ---: | ---: |
| macOS ARM64 DMG | 17,235,991 | 17,250,835 | +14,844（+0.086%） |
| macOS updater | 16,672,644 | 16,688,512 | +15,868（+0.095%） |
| Linux x86_64 webd | 7,493,706 | 7,484,369 | -9,337（-0.125%） |

受影响 Core 单元模块、webd、Tauri 设置模块及通知、设置、分页、子智能体、框架／wire 契约集成测试已通过。新增用例覆盖启停与旧配置复核、显式 Bark 测试、错误首次扫描基线、自定义配置路径、压缩嵌套与附件、129MiB 历史尾部、201 项中断及切机隔离；前端相关单测与类型检查、两套 Clippy、格式、版本、工作区、隐私和发布说明生成检查通过。标签 Release CI 的前后端完整测试、Chromium／WebKit 功能回归（含新增三 Provider 批量预览／执行用例）、macOS 性能诊断与 Tauri 检查、正式打包发布均已通过。本机未启动验收浏览器或重复打包。

待用户验收：子智能体滚动、时间线与附件应完整；设置开关与 Hook 状态及时刷新，新问题可推送而旧错误不补发；批量处理中详情写入禁用，中途请求失败保留已完成项。真实通知实收和跨平台界面按这些入口验收。

## v2.1.24 实现与验证

保存设置在原有事务内同步五种原生来源及 Bark 启用时间，扫描入库与通道同步在同一事务内读取最新文件配置；单会话 Hook 不覆盖全量扫描状态。重新启用只重建该平台流游标，保留既有通知身份；Antigravity 已被 Hook 观察的原地变更步骤继续参与后续扫描。现有等待循环每两秒检查配置变更，轮询频率未变时不增加扫描。Mac helper 单独更新时对已加载服务执行一次 kickstart -k，plist 变化仍重新加载，普通保存保持非强制启动。

Claude 完成候选已明确撤销、且尚未被原生发送者或 Bark 领取时静默结束并清空正文，不写可见完成记录；独立完成回合、有效 Hook、读取失败的有限重试保持。无关通知不再读取 Claude Hook 配置。改变选择清除旧预览／执行错误，移除弹窗内不可达的重复执行错误展示；Claude／Grok 详情改名、单项删除及已打开改名框提交共用批量 busy 状态。

Codex 搜索和主线程附件使用现有未压缩完整历史，界面初始折叠保持；普通和 Zstandard 历史均覆盖。Codex 归档／恢复预览在批内复用数据库、目录、索引和反向引用；Antigravity 复用一次实时列表及父子图；DSH 删除预览复用一次进程、索引和完整会话扫描。执行仍重新核验目标，保持指纹、运行／锁定、后代归属和逐项失败规则。

原生未完成队列增加局部索引，probe_events 增加 JSON 事件键索引，错误事件增加 event_id 索引；Bark 结果仅按实际变化键更新关联记录。现有 schema 3 在迁移锁内幂等补建索引，保留旧队列与加密数据。不新增数据库表、依赖、后台任务、持久化状态文件或 CI 门禁。

本机 macOS 开发构建，以修改前代码为基线，对同一合成负载前后各测一次。各预览负载从 2,000 个会话选取 100 项，Antigravity 使用离线摘要；通知为 100,000 条历史及 100 条待发记录，分别读取队列 100 次、领取并确认结果 100 次。内层耗时围绕业务调用；进程 CPU 与峰值 RSS 包含测试框架，通知进程另包含夹具复制、打开数据库及索引创建。DSH 最终实现调整后复测其对应负载。

| 负载 | 内层耗时 ms：前 → 后 | 进程 CPU s：前 → 后 | 峰值 RSS 字节：前 → 后 |
| --- | ---: | ---: | ---: |
| Codex 归档预览 | 4,622.552 → 172.854 | 4.59 → 0.14 | 14,123,008 → 16,793,600 |
| Codex 恢复预览 | 4,730.202 → 113.893 | 4.72 → 0.11 | 14,909,440 → 16,809,984 |
| Antigravity 删除预览 | 330.985 → 136.770 | 0.32 → 0.03 | 10,780,672 → 15,335,424 |
| DSH 删除预览 | 7,589.383 → 367.037 | 7.24 → 0.12 | 13,582,336 → 13,615,104 |
| 原生队列读取／Bark 结果回写 | 1,281.172／61,010.052 → 403.081／33.988 | 61.96 → 0.59 | 13,598,720 → 15,319,040 |

批内复用及索引显著减少该负载的 CPU 和扫描耗时，峰值内存没有一致下降。通知测量结束并 checkpoint 后数据库为 48,132,096 → 53,202,944 字节，增加 5,070,848 字节（约 4.84 MiB）；结束时 WAL 为 4,136,512 → 5,125,312 字节，不能当作累计磁盘写入量。未测量线上 Linux CPU／RSS、物理磁盘读写或空间回收。临时性能测量源码已移除。以下大小来自对应标签 CI 构建并正式发布的资产，三项均略增，不能声称包体积下降。

| 产物 | v2.1.23 字节 | v2.1.24 字节 | 变化 |
| --- | ---: | ---: | ---: |
| macOS ARM64 DMG | 17,203,920 | 17,235,991 | +32,071（+0.186%） |
| macOS updater | 16,634,882 | 16,672,644 | +37,762（+0.227%） |
| Linux x86_64 webd | 7,457,964 | 7,493,706 | +35,742（+0.479%） |

本机已通过受影响 Core／webd／Tauri 模块与 Hook 集成测试、前端选择／Claude／附件／历史定位相关测试、类型检查及构建、两套格式与 Clippy、wire 契约、版本、双工作区、安装脚本、源码隐私和发布说明生成检查。[标签发布 CI](https://github.com/lich13/agenthub/actions/runs/43720481922) 的完整回归、Chromium／WebKit 交互、macOS Tauri 检查、两平台构建与正式资产发布全部通过。已复用对应 CI 产物完成服务端部署和 Mac 原地升级，未重新打包；未进行真实通知实收、界面验收或用户会话删除。

待用户验收：保存启用通知后立即完成一次任务，应收到首条通知且不补发旧历史；缩短轮询周期及时生效；Claude 后台工作未收敛不显示误完成。批量预览失败后改选应清除旧错误，批量处理中详情改名／单删禁用；长 Codex 线程搜索及早期附件应正常访问。

## v2.1.23 实现与验证

已确认仍处于 busy 的 Claude 当前完成事件留在原生加密待发队列，不领取 Bark 发送次数；有效 Stop hook 可先投递。读取错误继续使用既有有限重试，终态、子代理、去重及真实网络重试规则保持。Claude 删除在执行前登记整批目标，缺失主文件的预览与共享清理归属对齐；任一副本或项目索引失败继续保留共享数据及后台作业。

批量执行范围与筛选选择分离，搜索在忙碌时禁用；取消预览立即恢复操作且忽略迟到回调，重新预览清除旧错误，归档区折叠后保留失败及残留反馈。CI 快照新增可缺省的 repository_observed_ms，各仓库只在结果变化或首次成功跨过来源基线时更新，其他仓库刷新与读取失败不推进该证据；设置接口及直接修改配置均处理每次取消 Bot 过滤。

Codex 批量预览一次读取父子关系和文件清单，重复路径体积在批内复用；Claude 搜索按原有页面顺序访问解析快照，省去详情及子代理列表构造。Mac 普通启动／设置保存不强杀现有 monitor；helper 内容升级或 plist 变化时重载一次。不新增依赖、数据库表、持久化状态文件、后台任务或 CI 门禁。

本机 macOS 开发构建，以修改前实现为基线，同一合成负载前后各测一次。Codex 为 2,000 个 rollout 中的 100 个显式删除预览目标；Claude 为 32,000 条历史事件和 64 个子代理，列表定位后执行无命中的线程搜索。两项使用独立测试进程；计时与 CPU 用时只包围业务调用，峰值 RSS 包含夹具及测试框架。

| 负载 | 耗时 ms：前 → 后 | CPU ms：前 → 后 | 峰值 RSS 字节：前 → 后 |
| --- | ---: | ---: | ---: |
| Codex 批量删除预览 | 636.304 → 10.005 | 635.857 → 10.006 | 9,125,888 → 9,437,184 |
| Claude 线程搜索 | 1631.490 → 1448.624 | 1599.636 → 1420.011 | 77,135,872 → 76,431,360 |

上述单次结果显示重复扫描耗时减少；峰值内存没有一致下降，不能推断线上常驻资源占用。没有测量线上 Linux CPU／RSS、物理磁盘读写或空间回收。临时性能夹具代码已移除。以下体积来自对应标签 CI 构建并正式发布的资产；三项均略增，不能声称本版缩减了发布包体积。

| 产物 | v2.1.22 字节 | v2.1.23 字节 | 变化 |
| --- | ---: | ---: | ---: |
| macOS ARM64 DMG | 17,171,864 | 17,203,920 | +32,056（+0.187%） |
| macOS updater | 16,602,149 | 16,634,882 | +32,733（+0.197%） |
| Linux x86_64 webd | 7,437,779 | 7,457,964 | +20,185（+0.271%） |

本机已通过前端 69 个测试文件的 860 项测试、类型检查及构建，受影响 Core／webd／Tauri 模块回归、使用本次 Cargo 构建 CLI 的 Hook 集成测试、两套格式与 Clippy、wire 契约、版本、双工作区、源码隐私及发布说明生成检查。[标签发布 CI](https://github.com/lich13/agenthub/actions/runs/43695246470) 的完整前后端回归、Chromium／WebKit 浏览器检查、macOS Tauri 检查、两平台构建及发布边界检查全部通过。已使用对应 CI 产物完成服务端部署和 Mac 原地升级，未重新打包；本机未运行界面验收。

待用户验收：Claude 完成后的通知及时性与手机实收；取消预览、批量执行、归档折叠后的结果反馈；新 CI 完成通知；专用 Claude 测试会话删除。真实会话未执行删除。

## v2.1.22 实现

归档抽屉只拦截已处理按键，折叠清空选择，分区切换清空过期反馈，保留执行中 busy 和完成刷新。守护进程按 TOML 与 sibling env 的文件身份缓存，首次自行加载；Claude 拒绝 JSONL 目录，未索引文件不授权共享清理，缺失已知主文件保留专属残留清理与跨项目保护。CI 对 Bot 筛选切换当秒的同一已观察尝试补足完成判断，仓库基线与历史排除保持。

Antigravity 每轮复用父子索引，通知扫描在现有事务中复用准备语句，RPC 借用请求值，Codex 子详情复用第二次关系核验结果，完整脱敏直接构建字符串。不新增依赖、持久化状态、CI 门禁或子代理数量上限；数据库写入及保留策略保持，未执行磁盘空间回收。

本机 macOS 开发构建，以 `1d5b3b1` 为基线，同一合成负载前后各测一次，各项在独立测试进程中顺序运行。通知扫描为 500 条 Claude 流、预热后 100 次无变更扫描；Antigravity 为 5,000 条摘要、64 个子代理、10 次完整根读取（650 次本机模拟 RPC）；Codex 为父子孙关系及 2,000 条无关记录，预热后读取子详情 50 次；RPC 为含 5,000 个会话键的包装请求解析 200 次；完整脱敏为 20,000 行、1,661,890 字节，处理 50 次。

| 负载 | 内层耗时 ms：前 → 后 | 进程 CPU s：前 → 后 | 峰值 RSS 字节：前 → 后 |
| --- | ---: | ---: | ---: |
| 通知扫描 | 757.351 → 162.083 | 0.78 → 0.17 | 10,502,144 → 10,436,608 |
| Antigravity 子代理 | 403.779 → 117.192 | 0.37 → 0.10 | 23,019,520 → 23,052,288 |
| Codex 子代理详情 | 743.707 → 509.348 | 0.77 → 0.53 | 16,236,544 → 15,056,896 |
| RPC 请求解析 | 61.434 → 67.209 | 0.08 → 0.07 | 12,533,760 → 12,091,392 |
| 完整脱敏 | 7740.226 → 7692.569 | 7.74 → 7.69 | 13,713,408 → 12,648,448 |

CPU 为整个测试进程 user + system，RSS 包含夹具与测试框架；内层耗时不含夹具构造。RPC 与脱敏未测得明确提速，不能以单次微小差距推断线上性能。未测量线上 Linux CPU／RSS、物理磁盘读写或空间回收。临时性能测试代码已移除。

本机受影响核心单元与集成测试、守护进程全部测试、Mac CI 观察器、前端相关测试与类型／构建、两工作区 Clippy／格式、版本／wire／工作区、源码隐私、安装脚本与发布说明生成检查通过。[标签流水线](https://github.com/lich13/agenthub/actions/runs/43677632431)的 Linux 回归及两平台产物构建通过，但新增浏览器用例错误点击执行中禁用行，SSH 测试也以固定 20ms 等待退出而偶发失败。

测试修正提交 [`fca787c`](https://github.com/lich13/agenthub/commit/fca787cac618)改为断言禁用状态并检查忙时全选、在原有超时范围内等待子进程退出；[完整 CI](https://github.com/lich13/agenthub/actions/runs/43678834923)已通过，包含 Chromium／WebKit 与 Mac 回归。与标签的差异仅为浏览器测试和 `#[cfg(test)]` 内代码，产品实现相同，因此复用上述标签流水线的正式产物完成发布，没有移动标签或重新打包。

| 产物 | v2.1.21 字节 | v2.1.22 字节 | 变化 |
| --- | ---: | ---: | ---: |
| macOS ARM64 DMG | 17,188,832 | 17,171,864 | -16,968（-0.099%） |
| macOS updater | 16,626,232 | 16,602,149 | -24,083（-0.145%） |
| Linux x86_64 webd | 7,440,388 | 7,437,779 | -2,609（-0.035%） |

以上为对应标签 CI 构建并正式发布的资产，压缩包体积变化不代表用户历史或数据库空间回收。已完成服务端部署与 Mac 原地安装，版本、服务健康及 App 启动检查通过。

待用户验收：在普通／归档列表间切换选择、折叠后再展开归档抽屉，并从关闭的归档按钮按 Escape，旧选择与反馈应正确清除；执行中的批次应继续完成和刷新。新 CI 完成通知的 Mac／Bark 实收以及 Claude 删除、子代理详情日常使用仍由用户验收。

## v2.1.21 实现与验证

八项改动已实现：Claude 列表、缓存／回退定位及删除索引统一跳过会话专属目录，含主文件符号链接；CI 使用最新配置加载器，在同一现有 SQLite 事务内保存配置与设置状态，并复核扫描结果；Bark／Mac 识别当前基线内观察到的同秒完成。普通／归档选择通过统一入口互斥，归档按钮按展开状态处理全选；摘要仅移除完整图片，链接文字复用内联清理。Antigravity 保留提取后的待处理信息并共享最终状态表，Codex 适配移除重复筛选排序，摘要在完整脱敏后按原有预算提前结束。未增加依赖、持久化状态或 CI 门禁。

本机 macOS 开发构建，基线 `f87a80e`，相同合成输入前后各测一次。Codex 为 500 条记录的查询列表，预热后读取 100 次；Antigravity 为 32 个子代理、每个 128 条约 8 KiB 文本步骤，完整读取三次；摘要为 20,000 行正文整理 50 次，仅计已脱敏文本的格式处理。CPU 为进程区间 user + system，RSS 为含夹具准备的累计峰值；Antigravity 与摘要测量各自在独立测试进程中并行运行。

| 负载 | CPU：前 → 后 | 峰值 RSS：前 → 后 |
| --- | ---: | ---: |
| Codex 列表 | 3,771.943 → 3,676.723 ms | 10,944,512 → 10,354,688 字节 |
| Antigravity 子代理树 | 3,308.928 → 2,316.718 ms | 68,534,272 → 25,870,336 字节 |
| 超长通知摘要整理 | 3,280.593 → 1.792 ms | 12,763,136 → 10,338,304 字节 |

Codex 单次对照差距较小，不能据此保证线上 CPU 降幅；摘要仍需完整脱敏，这组数字不代表通知端到端开销。未测量线上 Linux CPU／RSS、物理磁盘读写或空间回收。临时测量代码已移除。以下为对应标签 CI 构建并正式发布的资产大小，Mac 包略减、Linux 包略增；包体积不代表用户历史或数据库空间回收。

| 产物 | v2.1.20 字节 | v2.1.21 字节 | 变化 |
| --- | ---: | ---: | ---: |
| macOS ARM64 DMG | 17,197,876 | 17,188,832 | -9,044（-0.053%） |
| macOS updater | 16,627,294 | 16,626,232 | -1,062（-0.006%） |
| Linux webd | 7,432,009 | 7,440,388 | +8,379（+0.113%） |

本机已通过 Core／webd 模块测试、Tauri 单元测试、受影响架构／设置／通知／契约集成测试、前端选择和工作区测试、类型检查与构建；两套格式及 Clippy、wire 契约、版本、双工作区、源码隐私与发布说明生成检查通过。[标签发布 CI](https://github.com/lich13/agenthub/actions/runs/43670045041) 的完整前后端回归、Chromium／WebKit 浏览器检查、macOS Tauri 检查、两平台构建、隐私与发布边界检查全部通过，八项正式资产已发布。使用对应 CI 产物完成服务端部署和 Mac 原地升级，未重新打包；本机未运行界面验收。

待用户验收：在普通／归档列表、归档按钮和浮动栏使用 Command+A／Edit → Select All，确认只有一处选择栏；切换 CI 配置后观察新完成提醒与真实通知实收；使用专门测试会话验证 Claude 清理及子代理查看。界面、实机与真实业务验收不等同于自动化验证。

## v2.1.20 实现与验证

已实现批准的十一项。原生全选按输入文字、列表和弹窗范围处理，空列表不选择整页；CI 凭据核验失败不延长有效期，目录错误按正常间隔或 Retry-After 调度。来源持久化各仓库首次成功扫描和取消 Bot 筛选的基线；请求完成后在短事务中复核配置与基线并合并已确认事件，旧响应不覆盖扫描期间的新设置。未增加 RPC、数据库表、依赖或 CI 门禁。

Grok 统一清理只接受 Inactive，未知占用记失败，普通显式删除保持。Claude 按主 JSONL 识别专属附属目录，跨项目副本与最后一次共享清理规则保持。Grok Bot 的升级截止时间只初始化一次，覆盖非零游标并避免后续扫描误吞新消息。通知摘要仅消耗完整有效的链接目标。Codex 先筛归档状态再补充正文；Antigravity 根会话及全部相关子代理共用一次按需进程快照，下一次读取重新获取。

以下为本机 macOS 开发构建及从对应源码提取的进程解析比较，基线 `547f878`。使用相同合成输入，不读取真实会话。Codex 有 50 条未归档、500 条已归档记录，各含约 8 KiB 消息，每种列表连续读取三次；保留操作系统缓存。CPU 为区间 user + system，RSS 为含夹具准备的进程累计峰值。

| 负载 | CPU：前 → 后 | 墙钟：前 → 后 | 峰值 RSS：前 → 后 |
| --- | ---: | ---: | ---: |
| Codex 默认列表三次 | 823.766 → 42.021 ms | 950.639 → 42.020 ms | 17,465,344 → 17,743,872 字节 |
| Codex 归档列表三次 | 799.931 → 713.508 ms | 805.947 → 713.729 ms | 17,580,032 → 18,202,624 字节 |
| Antigravity：2,000 行进程、12 个核验、100 轮 | 约 4.43 → 2.51 s | 4,435.849 → 2,515.971 ms | 2,686,976 → 2,719,744 字节 |

Antigravity 比较仅包含进程表解析与匹配，不包含实际 ps、网络轨迹请求和异步调度。默认列表收益包括避免无关归档会话挤占已有缓存。两项峰值 RSS 未下降，不能据此宣称内存降低；未测量线上 Linux CPU／RSS、物理磁盘读写或空间回收。临时测量源码已移除。对应标签 CI 的正式产物大小如下，本版包体积略有增加；包体积不代表用户历史或数据库空间回收。

| 产物 | v2.1.19 字节 | v2.1.20 字节 | 变化 |
| --- | ---: | ---: | ---: |
| macOS ARM64 DMG | 17,158,955 | 17,197,876 | +38,921（+0.227%） |
| macOS updater | 16,586,848 | 16,627,294 | +40,446（+0.244%） |
| Linux webd | 7,411,281 | 7,432,009 | +20,728（+0.280%） |

本机已通过受影响 Core、webd、Mac CI 观察器、前端单元测试及类型检查；框架、用例、配置、通知、Provider 快照和 wire／registry 契约检查通过。两套 Clippy、格式、前端构建、版本、工作区、安装脚本、源码隐私与发布说明生成检查通过。[标签发布 CI](https://github.com/lich13/agenthub/actions/runs/43663165626) 的完整前后端回归、Chromium／WebKit 浏览器检查、macOS Tauri 检查、两平台构建、隐私与发布边界检查全部通过，八项正式资产已发布。使用对应 CI 产物完成服务端部署和 Mac 原地升级，未重新打包。

待用户验收：在列表、输入框、弹窗与空筛选列表使用 Command+A／Edit → Select All，确认选择范围；切换 CI 仓库与 Bot 筛选后只接收新完成提醒，观察 Grok Bot 新消息及摘要括号文字。真实通知实收、界面交互和跨平台实机验收由用户完成。

## v2.1.19 实现与验证

CI 对有符合筛选条件的未结束运行的仓库按 5 秒轮询，空闲仓库保持 60 秒，gh 凭据／账号核对 60 秒、仓库目录刷新 10 分钟。Mac 每 5 秒只读当前来源缓存，详情缓存回收仍为 60 秒。新增来源筛选和成功扫描仓库集合；新增、重新加入及空仓库保持各自基线。ETag 只缓存解析后的摘要，持久化仓库集合使用有序集合，避免哈希重排造成无变化写入。

本机开发构建使用同一合成 10 条运行负载，预热后 500 次 304 请求，测量区间 CPU（user + system）从 134.104 ms 降至 64.991 ms；501 次请求（含预热）保持一致。缓存内容的序列化大小从 109,929 降至 3,261 字节，这不是实际堆内存或 RSS 测量。数据库触发器验证未变快速轮询不更新 CI 状态。活动仓库请求频次增加，不能据单请求结果推断服务端总 CPU 降低；未测量线上 CPU／RSS、物理磁盘写入或回收。

对应标签 CI 的正式产物大小如下，本版包体积略有增加：

| 产物 | v2.1.18 字节 | v2.1.19 字节 | 变化 |
| --- | ---: | ---: | ---: |
| macOS ARM64 DMG | 17,110,642 | 17,158,955 | +48,313（+0.282%） |
| macOS updater | 16,540,452 | 16,586,848 | +46,396（+0.281%） |
| Linux webd | 7,386,970 | 7,411,281 | +24,311（+0.329%） |

Antigravity 的九个相关子代理用例完成一次根轨迹及九次子轨迹读取，重复引用和循环不重复请求；增加的必要读取不计为性能收益。Claude 共享数据按真实删除和项目索引结果保护，普通删除不读取正文。保留 Grok Bot 消息级行为，不新增 draftSendState 白名单，不改变已由连接修订隔离的 CI 卡片 QueryClient。

本机受影响 Core／webd 模块、Mac CI 观察器、前端选择及原生菜单转发逻辑、框架及 wire 契约测试通过；类型检查、前端构建、两套 Clippy、格式、版本、工作区、源码隐私及发布说明生成检查通过。[标签 CI](https://github.com/lich13/agenthub/actions/runs/43652656357) 的发布边界、后端检查和两平台构建成功，前端与 macOS 检查因旧点击事件测试夹具缺少 currentTarget 而失败，自动发布作业跳过。仅补齐测试事件并验证聚焦行为的提交 [`9fa8517`](https://github.com/lich13/agenthub/commit/9fa851793f992192b3386139b3abc3f0b6c723a7) 通过本机完整前端单测及类型检查，随后[完整 CI](https://github.com/lich13/agenthub/actions/runs/43654038845) 的前端、后端、浏览器回归和 macOS 检查全部通过。产品源码未变，复用原标签构建产物发布，未移动标签或重复打包；原失败运行记录保留。

待用户验收：在线程列表及多选操作栏按 Command+A，应只全选当前范围已加载的筛选结果，输入框仍全选文字，弹窗不扩大列表选择。开启原有 CI／Bark／Grok Bot 通知后，观察运行完成提醒时效、长期未回复交互不重复提醒，以及附件通知只显示文件名；Grok Bot 消息级「完成」仍不代表整轮彻底结束。

## v2.1.18 Grok Bot 通知恢复

Grok Bot 恢复 v2.1.16 的消息识别与通知筛选，文字、附件、待回复和待授权继续按原有开关推送。消息级 completion 仍可能早于整轮结束，不承诺整轮只推送一次；可靠整轮终结信号仍未取得。保留账户隔离、首发与重试复核、消息游标、去重和启用基线，v2.1.17 已扫描的消息不会因恢复而重放。

本次只恢复 Grok Bot 通知路径；v2.1.17 的脱敏、配置、增量读取、缓存回收、重试、DSH／Antigravity 及体积优化保留，不改原生会话或通知配置。新增暂停版本升级后的消息基线回归，原有启用、重启、缓存淘汰、附件及交互通知测试继续保留。

本机 Grok Bot、原生通知入队和 provider monitor 测试、工作区 Clippy、格式、版本、双工作区、wire 契约、源码隐私及发布说明生成检查通过；标签 CI 的完整回归、浏览器、跨平台构建与发布通过。已使用对应 CI 产物完成服务端部署和 Mac 原地升级。

待用户验收：在本机原有 Probe、Bark、Grok Bot 开关已开启时，等待新的文字、附件或交互请求，手机应按原有规则收到提醒；不补发暂停期间已扫描的消息。消息级「完成」仍不代表整轮彻底结束。

## v2.1.17 源码与资源比较

Grok Bot 保留阅读、原生交互和消息基线，暂停 Bark；脱敏版本化样本只证实消息与请求结构，未取得整轮终结信号。不能由合成数据承诺真正完成推送。Claude 正文不再使用日志脱敏器，日志与通知继续匹配凭据前缀及键值。Hook 行数统一为 10–10,000，日志上限为 4,096–8,388,608 字节；sound/url 的缺省保持、null／空值删除及非空写入已贯通两层配置补丁。

Codex 来源摘要保持流式校验，并移入现有有界阻塞任务；提问重试按原回合和调用确认，允许同轮并行工具。DSH 移除单次扫描累计解压上限，保留原文件、单帧、记录和缓存预算。Antigravity 读取完整子轨迹，读取不完整时不认定完成。旧加密分片队列、已确认分片、租约和有限重试保留。Mac 复用现有 60 秒循环释放五分钟闲置缓存，不新增后台任务。

仅删除无生产调用的 system.version／system.platform／system.providers；Claude/Grok 单会话删除预览仍由页面使用，保留原接口及删除流程。清理 stoppedEarly、作业 thread_id／turn_id 和事件 handled_at 的响应传递，SQLite 原列继续保留；不迁移表、不宣称回收数据库空间。两个 Rust 工作区只新增 release 符号剥离，沿用 stable、锁文件和原有构建参数。隐私检查补充 WebSocket、固定主机模板边界和公网 IPv6，保留前端打包依赖字面量的既有扫描范围。

以下为本机 macOS 开发构建，基线 `1ac1312`，相同合成负载前后各测一次。异步问答日志为 69,402,835 字节，追加 20 次共 42,360 字节；CPU 为执行区间 user+system。

| 异步问答读取 | CPU：前 → 后 | 墙钟：前 → 后 |
| --- | ---: | ---: |
| 冷读 | 977.10 → 994.69 ms | 977 → 998 ms |
| 未变热读 20 次 | 4.36 → 0.09 ms | 4 → <1 ms |
| 追加读取 20 次 | 19,754.58 → 9.67 ms | 19,982 → 9 ms |

追加读取的进程峰值 RSS 为 6,733,824 → 6,914,048 字节，未下降；收益来自复用解析状态，冷读基本持平。该样本空闲回收调用两版均为 8 μs，未测量实际桌面缓存释放后的 RSS。临时测量代码已移除。

正确性修复增加部分读取成本：Antigravity 3,000 步样本从尾部 256 步／282,379 字节改为完整 3,000 步／3,309,047 字节，本机模拟请求为 21 → 119 ms。67,109,291 字节来源摘要为 1,667 → 1,679 ms；135,266,731 和 537,919,915 字节原先直接拒绝生成摘要，修复后分别耗时 3,396 和 13,513 ms。它们不属于性能收益，摘要仍使用固定缓冲区。未测量 Linux 服务 CPU／RSS、物理磁盘写入或回收；包体积以标签 CI 产物为准。

以下为对应标签 CI 构建并正式发布的资产大小，未在本机重复打包。包体积减少不代表用户历史或数据库空间回收。

| CI 产物 | v2.1.16 字节 | v2.1.17 字节 | 变化 |
| --- | ---: | ---: | ---: |
| macOS ARM64 DMG | 18,840,784 | 17,113,322 | -1,727,462（-9.17%） |
| Mac 更新包 | 18,262,203 | 16,541,404 | -1,720,799（-9.42%） |
| Linux x86_64 服务端包 | 8,098,569 | 7,383,553 | -715,016（-8.83%） |

本机已通过受影响 Core、webd、Tauri 单元及集成测试、前端定向测试和类型检查、两端 Clippy、前端构建、格式、契约／wire、版本、双工作区、源码隐私及发布说明生成检查。标签 CI 的完整回归、浏览器检查、跨平台构建、发布检查和八项资产发布均通过；使用同版正式产物完成服务端部署和 Mac 原地升级。

待用户验收：在本机及 SSH 的 Claude 会话查看含 task-1／max_tokens 的正文，保存合法 Probe 行数与日志上限，再使用 Claude/Grok 右键或详情菜单预览删除，内容与原有流程应保留；Grok Bot 阅读及待处理交互仍显示，过程文字、附件和等待授权不再发送 Bark。真实来源通知、SSH 和原生界面仍未验收；本次没有可承诺的 Grok Bot 整轮完成推送。

## v2.1.16 源码与资源比较

已实现批准的六项：合法表头保存、引号键保存、多行字符串保护、Claude 子智能体只读查看、Codex 子活动索引匹配、通知事件数组共享。配置沿用写前解析、权限保留与原子替换；不新增状态文件、数据库、后台任务或 CI 门禁。Claude 使用 2.1.323 脱敏原生 fixture，主列表／主 sidechain 过滤保持；经父键及子 ID 读取，同名子 ID、源替换、符号链接、删除、追加和分支均有回归。子状态 unknown、工具仅显示记录、附件仅显示名称，旧服务无能力时隐藏入口。

以下为本机 macOS 开发构建，基线 `58fa233`，相同合成负载前后各测一次。CPU 为测量区间 user+system；RSS 是含夹具准备的进程累计峰值；累计分配不是常驻内存。Codex 为 1,201 条线程、100 个直属子代理、1,000 条活动，预热后刷新 20 次；通知为 200 个快照、每个 100 条 2 KiB 正文，复制读取 50 次。

| 负载 | CPU：前 → 后 | 墙钟：前 → 后 | 峰值 RSS：前 → 后 | 累计分配：前 → 后 |
| --- | ---: | ---: | ---: | ---: |
| Codex 子活动匹配 | 1,198.380 → 425.409 ms | 1,207.580 → 427.602 ms | 23,887,872 → 23,707,648 字节 | 286,094,220 → 286,142,220 字节 |
| 通知快照复制 | 283.693 → 1.849 ms | 285.554 → 2.005 ms | 96,862,208 → 52,068,352 字节 | 2,187,593,500 → 1,633,500 字节 |

Codex 分配量基本不变，改进来自减少逐条活动扫描全部线程；通知只测快照复制，不包含原生解析、数据库入库与 Bark 网络。未实测 Linux 服务 CPU／RSS、物理磁盘写入或空间回收。临时测量源码和本机预览文件已移除。

以下为对应标签 CI 构建并正式发布的资产大小，未在本机重复打包；三个包均略增，不能宣称交付体积下降。

| CI 产物 | v2.1.15 字节 | v2.1.16 字节 | 变化 |
| --- | ---: | ---: | ---: |
| macOS ARM64 DMG | 18,639,911 | 18,840,784 | +200,873（+1.08%） |
| Mac 更新包 | 18,033,851 | 18,262,203 | +228,352（+1.27%） |
| Linux x86_64 服务端包 | 8,076,949 | 8,098,569 | +21,620（+0.27%） |

本机 Core 库、Codex 子代理、分页与来源快照、服务端 provider monitor、前端测试及类型检查通过；Tauri 测试目标编译通过。两套 Clippy、格式、前端构建、契约／wire、版本、双工作区、源码隐私及安装脚本检查通过。使用现有浏览器核查浅色并排、深色覆盖和窄屏正文，修正标题状态间距与列表过早截断，返回焦点、Escape、未知状态及无横向溢出通过。标签 CI 的完整回归、浏览器回归与跨平台构建已通过。macOS WebKit 会话切换测量前两次 P95 为 103／126 ms，高于既有 100 ms 门槛；同提交 main CI 为 78 ms，发布流程第三次为 58 ms。仅重跑失败作业，未修改源码、门槛或重建已成功的资产；不能据此宣称性能波动已解决。

服务端部署与 Mac 安装已完成。待用户验收：在本机及 SSH 的 Claude 主会话打开“本会话子智能体”，选择已有记录并返回；应能阅读文本、工具和分页历史，未见结果不转圈，附件只显示名称，主会话位置保留。真实原生会话、SSH 入口与通知实收仍未验收，不能据合成预览宣称实时状态。

## v2.1.15 源码与资源比较

已修复的内容摘要路径保留 uid／gid／mode、O_NOFOLLOW、前后文件身份及现有大小限制；旧引用和大文件不新增全量哈希。配置保存识别目标表与下一表的行尾注释、引号及引号内的 #，仍先解析 TOML 并原子替换，未知内容保留。安装引导尝试安装发行版 python3，版本不足仍在依赖阶段拒绝，不新增软件源或解析器。

Claude 列表右键删除并确认、多选删除与统一清理均使用原生清理入口；匹配后台登记才执行已有 CLI 的 rm，没有匹配记录不调用 CLI。帮助输出非阻塞读取并限制为 64 KiB；超时或读取失败回收本次 helper、保留历史，不向原生会话或进程组发送信号，不自动重放。成功的支持检查仅在本批复用，下一批重新核验。统一清理保留固定确认目标、预览指纹、活动归属、原子共享索引写入及 DSH 原生运行和共享索引保护；失败或残留继续逐项显示。

以下为本机 macOS 开发构建，同一合成负载前后各一次，基线 `814bdd7`。每个平台准备 1,100 条会话，预览后固定清理 100 条；CPU 为执行区间 user+system，累计峰值 RSS 包含夹具准备，不是线上 Linux 服务数据。共享索引写入量为原子替换文件的逻辑字节，不包含 SQLite 数据库及物理块写入。

| 统一清理 | CPU：前 → 后 | 墙钟：前 → 后 | 峰值 RSS：前 → 后 | 共享索引写入：前 → 后 |
| --- | ---: | ---: | ---: | ---: |
| Codex | 1,361.34 → 35.63 ms | 2,085.30 → 48.18 ms | 21.08 → 21.41 MiB | 20,360,300 → 194,000 字节 |
| Claude Code | 4,342.96 → 689.26 ms | 9,023.06 → 4,644.76 ms | 22.36 → 21.52 MiB | 20,361,700 → 194,014 字节 |
| Grok | 11,159.47 → 6,962.67 ms | 13,936.52 → 9,284.71 ms | 19.80 → 21.77 MiB | 11,439,750 → 109,002 字节 |

三类 CPU 分别下降约 97.38%、84.13%、37.61%，共享索引逻辑写入均下降约 99.05%。Codex、Grok 的峰值内存略增，不能宣称总体内存下降；写入减少不代表已回收磁盘空间，未测量线上服务 CPU／RSS。临时测量源码和计数器已移除，不新增性能断言或 CI 门禁。

以下为对应标签 CI 构建并正式发布的资产大小，未在本机重复打包；Linux 包略增，不能宣称所有交付体积均减少。

| CI 产物 | v2.1.14 字节 | v2.1.15 字节 | 变化 |
| --- | ---: | ---: | ---: |
| macOS ARM64 DMG | 18,649,581 | 18,639,911 | -9,670（-0.05%） |
| Mac 更新包 | 18,049,845 | 18,033,851 | -15,994（-0.09%） |
| Linux x86_64 服务端包 | 8,070,843 | 8,076,949 | +6,106（+0.08%） |

内容摘要碰撞和注释表头回归均先确认旧实现失败。自动化覆盖 helper 挂起、过量输出、原生拒绝、成功及回收，失败后下一管理请求继续；右键确认固定点击的目标，本机与远程入口均有回归验证。受影响核心模块、服务端、Tauri 远程模块及前端测试通过，两套 Clippy、格式、类型与前端构建、契约、版本、工作区、源码隐私及安装脚本检查通过；标签 CI 的完整回归、浏览器回归与跨平台构建均通过。真实原生 rm 正常耗时未实测，30 秒上限仍需实际工作树删除验证。待用户验收：使用专用测试会话在 Claude 列表右键删除并确认，匹配后台登记时应执行来源机器的原生清理；确认统一清理的失败项、其他成功项及残留结果符合预期。

## v2.1.13 源码与资源比较

源码版本为 2.1.13。Claude 精确匹配后台原生 ID／sessionId／resumeSessionId 后执行已有 CLI 的 `rm`，然后移除所选历史；没有匹配记录时不要求 CLI，不解析普通删除的正文、不检查或终止会话进程。计划及中途回复在原位置显示，工具配对跨正文保留，各折叠段独立保存选择。共享索引保留属主、属组和权限，并在替换前核对读取前的源文件身份。

删除无生产写入的作业／线程关联读取、旧 RPC 别名与递归转换、退役 Probe CLI、更新状态的 capabilities、开发默认密钥入口和废弃脚本；测试显式提供 fixture 密钥。新库不创建 thread_followups，已有表与数据保留。Grok 通知 JSONL 复用既有增量缓存，回合匹配使用单次读取内的时间索引。统一清理预览允许切机；部署脚本保留常驻服务权限边界及归档验签，直接二进制安装不依赖 Minisign，引导依赖检查要求 Python 3.11+。协议 2、schema 3 不变。

以下是本机 macOS 开发构建同一负载前后各一次，基线 `1f317f5`；CPU 为进程 user+system，累计峰值 RSS 包含夹具准备，不是线上 Linux 服务数据。Grok 为 400 个回合、8,400 条更新；匹配执行 10 次，冷读一次、热读 100 次，再追加一条，追加后文件 1,838,108 字节。Probe 为同一空原生线程库及含 1,000 条普通作业的面板库，读取 1,000 次。

| 负载 | CPU：前 → 后 | 墙钟：前 → 后 |
| --- | --- | --- |
| Grok 回合匹配 10 次 | 55,621.41 → 788.79 ms | 55,781 → 789 ms |
| Grok JSONL 冷读 | 42.56 → 43.17 ms | 42.55 → 43.30 ms |
| Grok JSONL 热读 100 次 | 22.64 → 0.20 ms | 22.64 → 0.20 ms |
| Grok JSONL 追加一条 | 50.91 → 0.66 ms | 50.91 → 0.66 ms |
| Probe 线程读取 1,000 次 | 未单独计量 | 382.67 → 72.13 ms |

Grok 测试进程累计峰值 RSS 为 73,498,624 → 75,530,240 字节，增加约 1.94 MiB；没有观察到冷读提速或内存下降。临时时间索引减少匹配工作，但并非持久化索引。未量化真实服务 CPU、RSS 或物理磁盘读写；删除旧查询与停止新建退役表不等于已回收已有磁盘空间。测量源码及夹具不进入交付，未添加性能阈值或 CI 门禁。

以下为对应标签 CI 构建并发布的资产大小，未在本机重复打包：

| CI 产物 | v2.1.12 字节 | v2.1.13 字节 | 变化 |
| --- | ---: | ---: | ---: |
| macOS ARM64 DMG | 18,608,937 | 18,580,860 | -28,077（-0.15%） |
| Mac 更新包 | 18,020,766 | 17,990,368 | -30,398（-0.17%） |
| Linux x86_64 服务端包 | 8,066,475 | 8,039,029 | -27,446（-0.34%） |

本机受影响核心与集成测试、服务端及 Hook CLI 测试、Tauri 测试、前端测试与构建均通过；两套 Clippy、格式、Rust／TypeScript 契约、版本、工作区、隐私及安装脚本检查通过。[标签 CI](https://github.com/lich13/agenthub/actions/runs/43550503094) 的完整回归、浏览器回归与跨平台构建均通过。原生后台删除使用合成 CLI 验证，未删除真实会话；真实删除、界面操作及通知实收待用户验收。

## v2.1.8 源码与资源比较

源码版本统一为 2.1.8，已完成批准的 13 项调整：统一清理改用 NoRequest 和内部范围枚举；合并 CI 关闭分支；移除 Claude Hooks 无用配置读取和 Probe 恒成功包装；线程运行作业只合并一次；附件、清理允许集及失败摘要跳过无用大小；DSH 扫描直接返回结果、空候选不扫描；删除已核实无引用入口；Hook 队列按 Provider 先筛选后限量；通知统计合并分组查询；贯通当前配置回调；入库后释放无须保留的通知全文快照；简化更新构造链和 Probe 前端固定参数。

API 协议 2、schema 3、依赖和原有执行校验保持。DSH 的 1 万条限制仍仅用于通知扫描，不扩展普通列表；配置保存仍解析磁盘内容并读取所需密钥。没有新增后台任务、状态文件、CI 门禁、索引或迁移。

以下为本机 macOS 开发构建，同一合成负载前后各一次，基线 `8addaba`。CPU 为测量区间 user+system，峰值内存为进程累计 ru_maxrss、包含夹具准备，不是 Linux 服务实测。线程为 500 行、100 个运行作业、25 个隐藏项，合并 2,000 次；通知统计为 20,000 条、5 个 Provider、4% 失败或待发状态，查询 200 次；DSH 附件为已预热详情及大小缓存的同一 PNG，读取 500 次。

| 负载 | CPU：前 → 后 | 墙钟：前 → 后 | 峰值内存：前 → 后 |
| --- | --- | --- | --- |
| 线程合并 | 1094.79 → 475.53 ms | 1185.09 → 479.06 ms | 7.41 → 7.28 MiB |
| 通知统计 | 5113.84 → 495.41 ms | 5595.95 → 497.10 ms | 9.73 → 10.14 MiB |
| DSH 附件读取 | 95.48 → 51.37 ms | 120.27 → 51.43 ms | 7.63 → 7.56 MiB |
| 五来源扫描与入库 | 25950.85 → 29669.41 ms | 26584.65 → 32280.77 ms | 67.38 → 63.95 MiB |

附件读取的大小统计请求实测 500 → 0；这是省去已预热的大小路径调用，不表示此前每次都遍历目录。按相同 20,000 条数据另行执行两版源码中的生产 SQL 并记录 SQLite trace，200 次通知状态查询的 SELECT 数为 3,000 → 1,200，包含每个 Provider 的状态行读取；该计数不混入上表计时。上表测量区间物理块读写计数均为 0，不能据此推断真实磁盘 I/O 为零或已有空间已回收。

五来源补充夹具分别链接 `8addaba` 与 `dd96c8d` 核心源码，使用同一组 702 个原生格式文件，Grok、Grok Bot、Claude、Antigravity、DSH 各 100 条会话，真实解析后按生产顺序入库并保留所需快照，执行 20 轮。Antigravity 从合成原生 API 响应文件进入真实解析器，不含 App 发现与 RPC；其他四类按明确夹具路径调用真实扫描器，不读取用户数据。每轮入库后的快照保留策略与各版 monitor 相同，不含 Bark HTTP。两版均产生 500 条待发事件；SQLite update hook 记录 1,595 → 1,600 次行写入，新增 5 次为较长测量跨过更多秒导致的 Provider 扫描时间更新，不表示写入优化。物理块计数均为 0；峰值存活正文 18.98 → 12.50 MiB、结束时保留流 500 → 100。此次 CPU、墙钟均增加，只有峰值内存下降，不能宣称完整扫描提速。

多来源快照保留夹具依序构造 5 个 Provider、每种 100 条流、每条 64 KiB 正文，仅覆盖入库后的保留策略，不包含来源解析、入库和 HTTP。结束时保留流数 500 → 100，峰值存活正文 31.25 → 12.50 MiB；阶段墙钟 6.85 → 9.69 ms，没有观察到耗时改善。外层测试进程内存包含 Cargo／测试宿主，不能作为该阶段或线上服务的内存收益。通知统计峰值约增 0.41 MiB；未测量线上 CPU、RSS 或磁盘占用变化。

受影响核心模块与集成测试、服务端及三组 Hook CLI 集成、Tauri 更新测试、前端 Probe／维护及契约测试通过；两套 Clippy、格式、前端类型与构建、Rust／TypeScript 契约、版本、工作区、源码隐私和安装脚本检查通过。回归覆盖无参数及旧空清理请求、线程隐藏与归档、附件和删除预览、DSH 空候选／解析失败／重复身份、Provider 队列隔离、限量前筛选、统计状态口径、首发和重试的最新配置及来源复核。配置缓存检查确认未变化文件复用已有解析结果。

完整回归、Chromium／WebKit 浏览器检查与跨平台构建由现有标签 CI 通过。首轮标签 CI 的 macOS WebKit 缓存线程切换 p95 为 112 ms，超过既有 100 ms 门槛；同提交主分支 CI 为 60 ms 且全部通过。使用 `gh run rerun --failed` 后标签检查通过，WebKit p95 为 76 ms；没有修改源码、门槛或标签。

以下为 v2.1.7／v2.1.8 对应标签 CI 正式发布资产的大小，没有在本机重复打包：

| CI 产物 | v2.1.7 字节 | v2.1.8 字节 | 变化 |
| --- | ---: | ---: | ---: |
| Linux x86_64 服务端包 | 7,931,190 | 7,949,437 | +18,247（+0.230%） |
| macOS ARM64 DMG | 18,424,107 | 18,413,560 | -10,547（-0.057%） |
| Mac 更新包 | 17,828,092 | 17,823,539 | -4,553（-0.026%） |

服务端部署及 Mac 原地升级均通过既有脚本完成，版本、健康和启动核对通过；下载与部署暂存由脚本清理。全部临时测量源码和夹具已移除，发布源码恢复并与标签一致，未添加性能断言。包大小减少与访问减少均不代表已回收磁盘空间。

待用户验收：在专用测试会话检查清理与附件读取，检查不同 Provider 通知实收、关闭通知及更换目标后的行为，以及 SSH 管理与 Mac 日常使用。

## v2.1.7 源码与资源比较

- 删除桌面独立 CI Monitor 与交接锁，关闭 CI 时只读缓存且不持续显示刷新；通知观察器及远程 RPC 的共享 `cached_only` 保留。Mac 有待发事件才请求授权，由原生回调发送该批事件，通知 ID 使用稳定事件身份。
- 隐藏清理只走分页扫描，完成与停止均由扫描器回调；统一预览不再接收范围筛选。DSH 删除保留执行租约、inode 与后代归属检查，归档执行合并为一次扫描并保留逐项结果。
- Probe 复用本轮定位，只读候选会话通知状态且不统计目录大小；Antigravity／DSH 列表筛选、排序、截取后才补大小。后台通知仍扫描完整来源，首发与重试按目标复核；Grok Bot 纳入现有通知状态汇总。
- DSH 通知缓存取消 64 项限制，保留 32 MiB 字节预算及空闲释放；DSH／Grok 缓存计重只在条目变更时计算，Grok 通知日志共享不可变记录。
- 空补丁、相同完整表单和仅换 Key 的保存不替换未变化 TOML；Key 仍加密写入，业务审计保留。配置实际变化时直接原子替换配置路径并保留权限，不再维护符号链接；活动态势只消费 `active_sessions`。删除未使用的计划、字段、重复服务入口及前端辅助。

本机 macOS 开发构建，同一合成负载前后各一次；基线为 `87eefd7`。CPU 为进程 user+system，内存为累计峰值 ru_maxrss，包含夹具准备；不是线上服务实测。夹具包含各 1,000 个 DSH／Grok 会话，普通 DSH 场景半数归档。列表取 20 条、Probe 聚合与完整通知扫描各执行 10 轮；Probe 每轮仅清空聚合缓存，保留来源缓存。目标复核预热 1,000 个有效 Grok 通知来源后复核同一事件 10 次；配置相同补丁保存 100 次；清理为 1,000 个归档会话预览一次后执行 20 项。

| 负载 | CPU：前 → 后 | 墙钟：前 → 后 | 峰值内存：前 → 后 |
| --- | --- | --- | --- |
| DSH 列表 | 5929.20 → 480.43 ms | 3710.84 → 477.97 ms | 19.08 → 18.59 MiB |
| Probe 聚合 | 28439.58 → 14118.82 ms | 26551.89 → 14440.82 ms | 37.77 → 37.08 MiB |
| DSH 完整通知扫描 | 3162.12 → 301.55 ms | 3176.09 → 303.68 ms | 15.55 → 17.33 MiB |
| Grok 完整通知扫描 | 2505.02 → 2179.54 ms | 2554.35 → 2532.98 ms | 15.98 → 15.98 MiB |
| Grok 目标事件复核 | 2700.75 → 2.63 ms | 2702.56 → 2.63 ms | 18.38 → 17.70 MiB |
| 相同配置保存 | 127.82 → 25.17 ms | 535.42 → 25.19 ms | 12.67 → 12.64 MiB |
| DSH 归档清理（含预览） | 292.45 → 202.00 ms | 861.79 → 688.52 ms | 17.48 → 17.20 MiB |

同负载另行计数读取工作量，计数不混入上表计时：列表大小统计请求 10,000 → 200，摘要折叠 1,000 → 500；Probe 大小统计 10,500 → 0，DSH 通知正文读取 5,000 → 50 次（7,305,000 → 73,050 bytes），Grok 通知 JSONL 读取 2,000 → 100 次（1,561,000 → 78,050 bytes）。DSH 完整通知扫描正文读取 5,000 → 500 次（7,305,000 → 730,500 bytes）；Grok 完整扫描仍读取 2,000 次、1,561,000 bytes，墙钟耗时基本持平。目标事件复核的 JSONL 均命中已有缓存，收益来自减少定位及无关来源复核。

配置文件替换 100 → 0；DSH 清理目录扫描 3 → 2（包含用户预览，执行阶段为 2 → 1），归档索引写入均为 1 次。这些夹具没有数据库写入，物理块计数均为 0，不能据此推断真实磁盘 I/O 为零或已有空间已回收。DSH 缓存扩容使本次峰值增加约 1.78 MiB；32 MiB 是缓存预算，不是进程内存上限。DSH 读取与 Mac 授权优化属于 Mac；配置、Grok 定向复核及服务层精简也用于 Linux，但本次混合 Probe 数字不能直接外推至服务端。

以下为 v2.1.6 与 v2.1.7 对应标签 CI 构建并正式发布的资产大小，未在本机重复打包：

| CI 产物 | v2.1.6 字节 | v2.1.7 字节 | 变化 |
| --- | ---: | ---: | ---: |
| macOS ARM64 DMG | 18,534,722 | 18,424,107 | -110,615（-0.60%） |
| Mac 更新包 | 17,926,775 | 17,828,092 | -98,683（-0.55%） |
| Linux x86_64 服务端包 | 7,946,306 | 7,931,190 | -15,116（-0.19%） |

受影响核心、服务端、Tauri 及前端测试，格式、Clippy、前端类型／构建、契约、版本、工作区与安装脚本检查已通过。自动化覆盖 CI 启停与缓存、通知状态和身份复核、日志追加／替换、缓存释放、隐藏扫描完成／停止／换机、DSH 锁与归属／指纹／逐项结果、列表与详情大小，以及重复表单／空补丁／仅换 Key 不替换配置。完整回归、浏览器回归与跨平台构建由现有标签 CI 通过；未添加性能门禁。

部署首次传输遇主连接超时，切换本轮备用连接后通过同一部署脚本完成；失败的两端暂存和安装下载已清理，正式安装器回退材料保留。性能夹具与临时源码已清理，未修改用户监控开关。

待用户验收：通知授权允许／拒绝及未处理、实收与点击；关闭 CI 后只读缓存；隐藏扫描停止；普通列表大小与专用测试会话清理。真实业务与界面验收不等同于自动化验证。

## v2.1.6 已交付

[**v2.1.6**](https://github.com/lich13/agenthub/releases/tag/v2.1.6) 发布提交 [`ca49a3b`](https://github.com/lich13/agenthub/commit/ca49a3b44ae35efdcd23b8084df6f309ff5abe7d) 的[标签 Release CI](https://github.com/lich13/agenthub/actions/runs/42343884400) 全部通过，包含完整前端、后端及 macOS Tauri 检查，八项正式资产已发布。对应 CI 产物已部署服务端并原地升级 Mac：服务端 `agenthub-webd 2.1.6`、service `active`、health `ok`；Mac App 从 2.1.5 原地升级到 2.1.6 并启动，未保留旧版副本。服务端保留安装器回退材料，未执行回退。协议 2、schema 3 保持，未操作用户监控开关。

## v2.1.6 源码与资源比较

源码统一为 2.1.6：精简桌面 CI Monitor 交接与通知授权，临时索引替代运行／待发事件嵌套查找；隐藏清理仅保留分页扫描，来源租约保持至终态；两端配置共用保存入口，DSH 操作复用定位结果。关闭的 Provider／错误监控不再周期性写入相同状态，Claude Hook 配置按需读取，Probe 聚合移入已有后台读取任务；reqwest 统一为 0.13.4 并保留 ring。协议 2、schema 3 及已有基线、凭据、通知待发状态保持。

本机开发构建、相同合成负载前后各测一次。CPU 为进程 user+system，内存为进程累计峰值 ru_maxrss，包含夹具准备；不是线上服务实测。CI 为 100 仓库、每库 10 个运行，重复比对 500 次；DSH 为 1000 个会话、40 次归档预览；Probe 为 1000 个 DSH 会话、每轮清空相关缓存后聚合 10 次。关闭轮询以生产 SQL 在 SQLite WAL 数据库执行 500 次。

| 负载 | CPU：前 → 后 | 峰值内存：前 → 后 |
| --- | --- | --- |
| CI 比对 | 736.68 → 424.97 ms（-42.3%） | 8.141 → 8.953 MiB |
| DSH 归档预览 | 1403.65 → 15.41 ms（-98.9%） | 10.547 → 9.297 MiB |
| Probe 聚合 | 16717.12 → 17089.86 ms（+2.2%） | 28.484 → 28.344 MiB |
| 关闭监控轮询 | 17.94 → 0.94 ms（-94.8%） | 20.391 → 20.469 MiB |

关闭轮询数据库行更新 500 → 0，WAL 新增字节 2,060,032 → 0；这是减少写入，不是回收既有磁盘空间。CI 临时索引增加约 0.81 MiB 峰值；Probe 调度优化本次未测出 CPU 降幅。

以下为 v2.1.5 与 v2.1.6 对应标签 CI 构建并发布的资产大小，未在本机重复打包：

| CI 产物 | v2.1.5 字节 | v2.1.6 字节 | 变化 |
| --- | ---: | ---: | ---: |
| macOS ARM64 DMG | 18,954,156 | 18,534,722 | -419,434（-2.21%） |
| Mac 更新包 | 18,363,452 | 17,926,775 | -436,677（-2.38%） |
| Linux x86_64 服务端包 | 7,948,780 | 7,946,306 | -2,474（-0.03%） |

受影响模块测试、两工作区格式／Clippy、前端测试／类型／构建、契约、版本与安装脚本检查已通过；完整回归、浏览器回归和跨平台构建由标签 CI 通过。自动化覆盖 CI 基线、结果、attempt、手动刷新交接、通知投递／重试，隐藏清理互斥／取消／分页／执行边界，DSH 普通与压缩日志定位、子会话排除／路径／锁、最新摘要，配置权限／符号链接／env 及关闭状态写入。Mac／Bark 实收与点击、最小化通知、实际手动查询和清理／DSH 业务操作仍由用户验收。

## v2.1.5 已交付

新增 CI 成功、失败及超时的 Mac 系统通知，沿用 Bark 通道并将 CI 卡片置于 Probe 顶部。桌面运行期间观察当前机器缓存，首次观察不补发历史结果。数据库去重及标记清理、隐藏扫描、DSH 定位和线程列表移除重复工作；Probe 配置改为保留权限的原子替换，精简无用依赖及内部死代码。协议 2、schema 3 保持。

## v2.1.4 已交付

源码版本 2.1.4 基于 `main / a9fd7de`。批准清单、本机验证、界面检查、CI、发布、服务端部署及 Mac 原地升级均已完成。

- CI 每仓库仅取首页 10 条，保留仓库目录分页、公开／私有／fork 范围、ETag、连接复用与凭据同源检查；不再遍历运行历史或单查窗口外旧运行。刷新目录时只淘汰已消失仓库的运行缓存，移除 256 项整表清空。
- 列表先筛仓库再按更新时间最多展示 20 条；下拉框铺满内容宽度，统一 36px、13px 系统字体与箭头，运行数移到工具栏。扫描中的新刷新由本轮结果满足；限流结束 refreshing，等待时间只留内存，旧状态仍可读取。
- App／Hook 启动条件包含 CI；全部后台监控关闭时，桌面状态惰性复用一个串行 Monitor 查询首次读取／手动刷新，不启动额外守护进程、不发送通知。首次启用、重新启用及切号的基线轮跳过全部完成记录，在途任务后续完成正常通知。
- 待发事件保留持久化，tick 直接返回事件，成功项批量确认一次、失败项保留；保留过期过滤、首发有效性与加密队列重试复核，通知使用 html_url。Claude 摘要与 Codex 问答接入原有 5 分钟空闲释放；实际历史删除省去两次预览计数。
- 删除无生产调用的 CapabilityGatePlan、归档计划包装和三个会话／任务门面；保留 github_ci 能力识别。Antigravity 仅整理分支返回；前端筛选改派生值，Grok Bot 复制直接调用 copy，Finder 仍显示失败反馈。没有新增数据库表、迁移、定时器或 CI 门禁。

### 同负载资源对比

本机开发构建，基线与修改后各一次。CPU 为 user+system，内存为测量进程累计峰值 ru_maxrss；不是部署后的服务实测。CI 夹具为 16 仓库 × 250 个完成运行，每个 HTTP 响应延迟 5ms，包含夹具服务器；缓存为 200 个 Claude 摘要及 200 个 Codex 问答文件。

| 负载 | CPU：前 → 后 | 墙钟：前 → 后 | 峰值内存：前 → 后 |
|---|---|---|---|
| CI 冷启动 | 0.181974s → 0.046776s | 0.513865s → 0.161638s | 26.69MiB → 14.09MiB |
| CI 条件轮询 9 次 | 0.973438s → 0.349485s | 4.025078s → 1.327431s | 28.14MiB → 14.44MiB |
| 32 个事件校验、加密入队及状态确认 | 0.488755s → 0.037702s | 0.492206s → 0.038201s | 28.14MiB → 14.84MiB |
| 缓存初次填充 | 1.379834s → 1.375512s | 1.407433s → 1.403144s | 15.81MiB → 15.66MiB |
| 调用既有空闲释放入口 | 0.110ms → 0.202ms | 0.109ms → 0.201ms | 15.83MiB → 15.67MiB |
| 空闲释放后再次读取 | 0.222544s → 1.240009s | 0.247426s → 1.263397s | 15.91MiB → 15.81MiB |
| 清理 5 万事件 + 5 万过期去重记录 | 0.396808s → 0.231784s | 0.766639s → 0.234062s | 15.88MiB → 17.59MiB |

CI 冷启动请求 50 → 18，9 次条件轮询请求 441 → 153；10 轮累计状态写入均为 1 次。状态摘要由 1,000 条／314,263 bytes 降到 160 条／51,209 bytes，对应批准的每仓库 10 条范围。32 个事件的 CI 状态确认写入 32 → 1，不包含加密队列本身的必要写入，测量未发送网络通知。

缓存空闲释放使用零时长触发同一入口；测试确认释放缓存引用并可按原数据重建，未测量空闲时的即时 RSS。释放后的再次读取明显变慢，不能宣称所有读取更快。历史清理前后均实际删除 50,000 + 50,000 条，本次峰值内存反而增加。减少写入、缩小状态和释放引用均不等于已回收磁盘空间；本轮没有实际磁盘回收数字。

真实账号另用隔离状态各读一次，均识别 16 个仓库且 pending 为 0。墙钟 78.489s → 17.317s、CPU 0.606090s → 0.133754s、峰值 19.78MiB → 16.12MiB，状态 411,785 → 52,666 bytes；修改后保留 124 条，查询无错误。基线有一个仓库网络请求失败，两次网络条件和运行数量也会变化，这组只作实际观察，不作为严格提速比例；用户监控开关未改，未发送通知。

### 正式包体积

比较 v2.1.3 与 v2.1.4 对应标签 CI 的正式发布资产，三项压缩包均有增加，本版未实现压缩包体积下降：

| 产物 | v2.1.3 bytes | v2.1.4 bytes | 差值 |
|---|---:|---:|---:|
| macOS ARM64 DMG | 18,483,215 | 18,963,922 | +480,707（+2.60%） |
| macOS ARM64 updater | 17,870,979 | 18,373,541 | +502,562（+2.81%） |
| Linux x86_64 webd | 7,969,655 | 7,981,915 | +12,260（+0.15%） |

### 核查与验收

workspace 与桌面端完整测试、WebUI 测试／类型／构建、两端 Clippy、格式、Rust／TypeScript 契约、版本、工作区、安装脚本、隐私及差异检查通过。回归覆盖首页读取、目录分页、304、基线、重跑、启停／切号／重启、扫描中刷新及保存、查询失败／限流、通知批量确认中途失败与缓存重建。一次原生剪贴板测试异常在单测及随后桌面完整复验中未再出现。

已有浏览器完成 1440／1280／600px 深浅主题、长名称、36px 高／13px 系统字体、同宽对齐、30 条只显示 20 条、先筛仓库再显示 10 条、刷新／保存及切机隔离检查；Grok Bot 复制成功与 Finder 失败反馈通过。界面使用隔离传输夹具，不代表远端服务或 Bark 实收。正式资产体积比较见上表。

首次部署在文件传输阶段 SSH 超时，尚未替换服务；固定使用本轮备用连接后重跑既有部署脚本成功。部署／安装工具现支持显式复用已选连接；本轮预览、夹具、下载及失败传输暂存已清理，正式安装器回退材料保留。

待用户在本机及远端「Probe → GitHub CI」检查加载、列表与筛选；选择一端确认后续 CI 的 Bark 实收及链接。升级保持现有监控开关，真实通知实收不阻塞发布。

## v2.1.3 已实现范围

- GitHub CI：来源机器活跃 gh 账号，名下公开／私有／fork 仓库，仓库每 10 分钟刷新；独立扫描、60 秒轮询、手动刷新、连接复用、分页／条件请求和限流等待。首次／重新启用／切号建立基线，每个 run attempt 去重，成功／失败／超时接入加密 Bark 队列；关闭、切号或新 attempt 停止旧待发通知。
- 已批准的 12 项：Grok Bot 直接路径操作、交互状态／普通卡片通知修复及复用定位；Antigravity 分支错误收口；批量归档标题／指纹及清理死代码删除；更新仅查 Release 并设置超时；更新包装和任务广播精简；搜索范围与重复过滤调整；前端死辅助删除；空闲错误游标仅变化写入。保留原后代范围、统一校验、DSH 结果类型、日志排空、互斥和终态。
- 补充批准项：session_index.jsonl 每行解析一次 ID 并查集合，移除逐行遍历全部目标 ID 的字符串匹配；补齐 Grok Bot 通知开关单字段保存时的空补丁判断和顶层合并，覆盖嵌套／顶层 true 与 false 保存回归。

### 同负载资源对比

本机开发构建，前后各一次；CPU 为进程 user+system 累计时间，峰值为该测量进程 ru_maxrss，不代表部署后的整个服务占用。归档修正索引匹配后的最终数据替代中间结果。

| 负载 | CPU：前 → 后 | 墙钟：前 → 后 | 峰值内存：前 → 后 |
|---|---|---|---|
| 1,000 个归档线程、5,000 个其他文件、10 万行索引，预览 5 次 | 30.371s → 1.281s | 31.963s → 1.357s | 14.66MiB → 13.95MiB |
| Grok Bot 80 个会话 × 100 个事件，读取／扫描 100 轮 | 4.964s → 5.108s | 5.127s → 5.566s | 14.41MiB → 14.44MiB |
| 任务日志 3 × 10 万行，输出 6,000,000 bytes | 3.767s → 3.013s | 4.081s → 3.282s | 31.56MiB → 26.92MiB |
| 空闲错误游标 6,000 次检查 | 333.576ms → 44.508ms | 575.766ms → 44.554ms | 7.64MiB → 7.52MiB |

空闲游标 SQL 写入由 6,000 次降到 0；原状态心跳仍保留。Grok Bot 本次样本未测出提速，不能把减少定位和字段计算等同于整体性能提升。所有样本的物理块计数均为 0，不据此宣称磁盘空间回收。

CI 开关对比：同一 16 仓库 × 50 摘要夹具、10 轮扫描，关闭 → 开启的 CPU 为 2.339ms → 181.849ms，墙钟为 2.412ms → 196.531ms，峰值为 17.30MiB → 24.77MiB（增加约 7.47MiB）；请求 0 → 171、状态写入 0 → 1，开启后保留 800 条摘要。包含夹具 HTTP 服务，排除真实网络及 gh 凭据读取；开启样本含首轮及 9 次条件轮询，空闲不重复写入。真实本机账号另行只读验证：识别 16 个仓库，保留 1,000 条完成摘要，首次完整扫描 76.46s，无限流及错误、无待发通知；该验证使用隔离数据库，未启用真实监控。

正式包体积取两版对应标签 CI 发布资产，均有增加：

| 产物 | v2.1.2 bytes | v2.1.3 bytes | 差值 |
|---|---:|---:|---:|
| macOS ARM64 DMG | 18,240,460 | 18,483,215 | +242,755（+1.33%） |
| macOS ARM64 updater | 17,640,491 | 17,870,979 | +230,488（+1.31%） |
| Linux x86_64 webd | 7,800,372 | 7,969,655 | +169,283（+2.17%） |

### 核查与验收

受影响的 workspace／Tauri 模块完整测试、WebUI 测试、类型及线协议契约通过；安装脚本、版本与工作区检查通过。两端 Clippy 与补充的 CI／Grok Bot 单开关保存回归通过。

助手已用现有浏览器检查 1440／1280／600px、深浅主题、系统字体和 13px 控件、同宽面板、长名称、筛选、刷新、保存与机器切换。隔离传输夹具验证账号／开关／筛选在切机时隔离，不代表远端服务或 Bark 实收。真实验收入口：选择一台机器，在 Probe → GitHub CI 开启监控并保存，确认后续成功／失败通知收到 Bark、点击进入对应运行，关闭后停止通知。

## v2.1.2 已交付

[**v2.1.2**](https://github.com/lich13/agenthub/releases/tag/v2.1.2) 已交付 Grok Bot 只读历史、明确请求通知及已批准的 11 项修复与精简；协议 2、schema 3 保持。该版标签 CI 全部通过，服务端部署和 Mac 原地升级完成。真实通知实收、其他 Agent 路径接手及日常业务仍待用户验收。

v2.1.2 支持新安装及已由 v2.1.0 完成迁移的配置。更早版本先升级至 v2.1.0；早于 schema 3 的数据库须先经过 v2.0.10。协议保持 2、数据库 schema 保持 3。

历史实现与交付由 Git 和 [Release](https://github.com/lich13/agenthub/releases) 保留。完整回归、跨平台构建和既有性能检查以发布提交对应的 [CI](https://github.com/lich13/agenthub/actions/workflows/ci.yml) 与自包含 [Release](https://github.com/lich13/agenthub/actions/workflows/release.yml) 为准。

## 已实现范围

2.1.2 新增 Grok Bot 当前账户近期缓存、实际缓存路径复制、默认关闭的明确请求通知；删除旧 App Server 合并、前端文案替换、源码文本门禁、安装配置残留与旧清理入口，修复 SSH 原子保存、Antigravity 离线回退、后台日志、任务释放和搜索读取。

| 工作块 | 当前行为 | 核心提交 |
|---|---|---|
| SSH 恢复与配置删除 | 同轮共享握手，读取最多重试一次，写操作不重放；删除所选用户 Host／Include 显式定义，保留其他别名、注释与权限，当前目标删除后回本机。 | `v2.1.1` |
| v2.1.1 资源与代码精简 | 直接 RPC 派发、正向命令表、旧迁移与旧 followup 方法移除；CR／LF 日志脱敏批量写入；jobs.detail 摘要；代理单线程；分页允许追加且错误明确。 | `v2.1.1` |
| SSH 连接、安装与旧链路退役 | OpenSSH Host/Include 选择，系统认证及跳板复用，固定 rpc-stdio 转发私有套接字；仅当前机器维持通道，切换／退出释放；同版本签名安装，清理旧远程连接、API Key 与专属 Nginx 入口。 | `v2.1.0` |
| Probe 与实时态势 | 删除说明小字、副标题、平台事件解释和重复引导，保留真实状态与确认对象；标题为“全平台实时态势”，前端 12 条及后端限制不变。 | `v2.1.0` |
| 审计项 1–5、11：配置、加密与迁移 | 删除 DB 打开预检、配置阶段 DB 预开／锁／写后回读及旧开发密钥／明文迁移；空库和 schema 3 可用；有效密钥保留，空条目原子替换；支持 Base64／Base64URL／hex；helper 修复回到启动编排。 | `v2.1.0` |
| 审计项 6–10：缓存、基线与日志 | Antigravity 复用来源终态，保留身份／分页完整性；时间线随快照生成并实际计重，revision 包含正文；未变通知基线不写库；日志按完整行解码脱敏并保留换行及尾行。 | `v2.1.0` |
| 审计项 12–13：后台执行记录 | 列表只传摘要，展开读取完整日志；仅未返回写请求阻止切换，已受理作业继续在原机器运行。 | `v2.1.0` |
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

## 本轮合成测量

同一隔离负载前后各测一次：200 个 Grok 会话，每个 220 条、约 1KiB 消息，总历史 46,904,000 B；搜索固定返回 200 条。随后同进程运行输出 2,048 行、133,120 B 的日志任务。测量不含夹具生成时间。

| 指标 | 修改前 | 修改后 |
|---|---:|---:|
| 搜索耗时 | 14,919ms | 455ms |
| 搜索 CPU（user + sys） | 14,125.224ms | 476.322ms |
| 搜索阶段进程峰值 RSS | 50,954,240 B | 11,829,248 B |
| 日志任务耗时 | 16ms | 16ms |
| 日志任务 CPU | 8.940ms | 9.601ms |
| 日志阶段进程累计峰值 RSS | 53,608,448 B | 14,532,608 B |

日志阶段 RSS 包含之前搜索的峰值，不能独立归因于作业实现。相同完成任务的实测 JSON 全文为 135,389 B、摘要为 221 B；后台不再请求终态全文，展开仍读取全文。缓存文件系统下输入／输出块计数均为 0，不能据此量化物理磁盘读写或空间回收，线上资源未测。

对应标签 CI 正式产物比较：

| 产物 | v2.1.1 | v2.1.2 | 差值 |
|---|---:|---:|---:|
| macOS DMG | 18,294,699 B | 18,240,460 B | −54,239 B（−0.296%） |
| macOS 更新包 | 17,700,880 B | 17,640,491 B | −60,389 B（−0.341%） |
| Linux 服务包 | 7,835,627 B | 7,800,372 B | −35,255 B（−0.450%） |

## 必要验证

2.1.2 核心实现已通过受影响的 core、webd、Tauri 单元／集成测试、WebUI 测试、TypeScript、两端 Clippy、线协议契约及安装脚本测试。合成数据覆盖 Grok Bot 双格式、账户隔离、缓存缺失与替换、分页、未知状态、通知基线／去重／重新启用／重启及回答后停止提醒；回归覆盖 SSH 注释／原子保存／并发编辑／部分失败、Antigravity 离线回退、任务互斥与日志排空、搜索限额与较早线程。只读解析本机 16 个缓存线程、1,115 条可见事件，详情和通知扫描均无解析错误。

已有浏览器完成深浅主题、1440／1280 与窄窗口检查；导航保持单行，Probe 六个平台开关整齐排列，系统字体、13px 桌面控件和 15px 阅读正文一致。键盘菜单、复制动作、未缓存禁用、搜索定位和窄窗口滚动通过；复制路径内容由合成数据测试验证。标签 CI 的完整 Chromium／WebKit 浏览器回归、macOS 既有交互计时、Rust 完整回归和跨平台构建全部通过；真实 Bark 实收、其他 Agent 路径接手和日常业务待用户验收。

2.1.1 核心实现已通过受影响的 core、webd、Tauri 单元／集成测试、WebUI 测试、TypeScript、两端 Clippy、线协议契约及安装脚本测试。覆盖 SSH 并发恢复、认证失败、超时关闭、切机时迟到握手、读取单次重试和写操作不重放；临时配置夹具覆盖 Host／Include／多别名／重复定义、注释与权限保留、系统配置拒绝和写入失败恢复。日志记录分隔、UTF-8／脱敏／尾部排空、作业摘要与默认全文、通知基线与去重、Antigravity 追加与不完整分页检查通过。

已有浏览器完成深浅色、1440／1280 与窄窗口预览：远程输入框、下拉框和按钮均为 36px／13px 系统字体，对齐与换行正常；夹具确认取消删除保持原配置，确认删除刷新 Host 列表并返回本机。预览服务、页面及浏览器标签已清理。标签 CI 的 Chromium／WebKit 全量浏览器回归、跨平台检查及 macOS／Linux 正式构建均通过；真实断网恢复、常用业务和通知实收待用户验收。

2.1.0 已通过受影响的 Rust 单元／集成测试、两端 Clippy、TypeScript 与相关 Vitest、线协议契约及安装脚本测试。覆盖并发 SSH 响应、认证失败、服务断开与通道释放、后台作业断开后完成、旧库拒绝／schema 3 保留、完整日志、基线启停、正文同数量改写、不完整分页及签名／下载／安装失败。新库的 WAL 切换纳入现有初始化锁，并发建库连续 40 次回归通过；已有 schema 3 数据库不重复设置 WAL。浏览器完整回归、Linux 后端及 macOS 构建由标签 CI 验证通过；部署后的 SSH 健康检查通过，界面操作和手机实收仍待用户验收。

同一隔离合成样本比较：200 次 DB 打开 CPU 57.781 → 42.637 ms，1,000 次基线读取 CPU 19.034 → 8.733 ms、WAL 86,552 → 8,272 B；30 条任务列表 JSON 725,721 → 5,721 B；3,000 条历史冷读 CPU 232.051 → 216.078 ms，10 次热读 2.404 → 1.923 ms；进程峰值 RSS 19,169,280 → 17,055,744 B。样本不代表线上收益，未测量磁盘空间回收。

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

2.1.2 发布提交 [`013fb75`](https://github.com/lich13/agenthub/commit/013fb75ab71213f8acbe79a75b8f4a41e79d9a67) 的[标签 Release CI](https://github.com/lich13/agenthub/actions/runs/42125865955) 全部通过，已发布[八项正式资产](https://github.com/lich13/agenthub/releases/tag/v2.1.2)。使用对应 CI 产物先部署服务端，再原地安装 Mac App：服务端 `agenthub-webd 2.1.2`、service `active`、health `ok`；Mac App 2.1.2 已启动，未保留旧版副本。服务端保留安装器回退材料，未执行回退；本轮预览与临时资源已清理。

2.1.1 发布提交 [`5c51fd9`](https://github.com/lich13/agenthub/commit/5c51fd9cd8c29c55073daceca79b8ef790fbb704) 的[标签 Release CI](https://github.com/lich13/agenthub/actions/runs/42061502813) 全部通过，已发布[八项正式资产](https://github.com/lich13/agenthub/releases/tag/v2.1.1)。使用对应 CI 产物先升级服务端，再原地安装 Mac App：服务端 `agenthub-webd 2.1.1`、service `active`、health `ok`；Mac App 2.1.1 已启动，未保留旧版副本。标签 CI 覆盖的 Chromium／WebKit 浏览器回归和跨平台构建通过。真实断网恢复、五平台常用业务及手机通知实收待用户验收；线上资源占用未测量。

2.1.0 发布提交 [`2c72801`](https://github.com/lich13/agenthub/commit/2c72801) 的[标签 Release CI](https://github.com/lich13/agenthub/actions/runs/41996055611) 全部通过，已发布[八项正式资产](https://github.com/lich13/agenthub/releases/tag/v2.1.0)。使用对应 CI 产物先升级服务端，再原地安装 Mac App：服务端为 `agenthub-webd 2.1.0`、service 为 `active`、SSH health 为 `ok`；Mac App 从 2.0.10 升级至 2.1.0 并启动，未保留旧副本。部署暂存清理已修复并实跑通过。SSH 连接选择／切换／重连与安装入口、Probe 文案、五平台常用操作、完整执行日志及手机通知仍待用户验收，线上资源占用未测量。

2.0.10 实现提交 [`4ff11a0`](https://github.com/lich13/agenthub/commit/4ff11a032340) 与启动入口断言修复 [`deb081d`](https://github.com/lich13/agenthub/commit/deb081d5b050) 已推送，`v2.0.10` 指向 `deb081d`。[标签 Release CI](https://github.com/lich13/agenthub/actions/runs/41941803886) 全部通过，已发布[八项正式资产](https://github.com/lich13/agenthub/releases/tag/v2.0.10)。按先服务端后 App 的顺序使用对应 CI 产物交付：服务端为 `agenthub-webd 2.0.10`、service 为 `active`、health 为 `{"ok":true}`；Mac App 从 2.0.9 原地升级至 2.0.10 并启动，未保留旧副本。真实功能与手机实收仍待用户验收，线上资源占用未测量。

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

2.1.2：本机 Grok Bot 阅读／搜索／复制路径，开启 Probe 后新待回复或授权的 Bark 实收，回答或批准后停止重试；其他 Agent 使用复制路径接手及日常业务。模拟数据与本机缓存解析不替代这些真实验收。

2.1.1 安装后在远程会话阅读途中断开并恢复网络，再刷新，确认自动恢复且机器、页面和阅读位置保持；在「设置 → 远程连接」用专门测试 Host 检查删除确认及返回本机。继续核验五平台常用操作、作业完整日志和手机通知实收。

2.1.0 安装后在「设置 → 远程连接」选择已有 SSH Host，检查保存连接、切换、重连及安装入口；在 Probe 检查说明清理和“全平台实时态势”标题，验证五平台常用操作、执行记录完整日志和手机通知实收。已受理后台任务应允许切换，返回原机器后能查看执行结果。

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
