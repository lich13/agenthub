# AgentHub（AgentHub）当前状态（维护者）

## 当前版本

当前源码版本为 **2.1.15**：修复 Codex 文件引用摘要校验及带注释 TOML 表头保存；Claude 原生清理帮助检查限时 5 秒、rm 限时 30 秒，同批复用成功的支持检查；统一清理复用 Codex／Claude／Grok 批量入口，单项失败后继续处理其他独立目标及平台；安装提示明确 Python 3.11+。API 协议保持 2、数据库 schema 保持 3。

最近正式版本为 [**v2.1.15**](https://github.com/lich13/agenthub/releases/tag/v2.1.15)，发布提交 [`942b3fa`](https://github.com/lich13/agenthub/commit/942b3faa63908ec5c5a5e9a01b6342c633bfe075) 的[标签 CI](https://github.com/lich13/agenthub/actions/runs/43581632139) 已全部通过，八项正式资产已发布。服务端已通过部署脚本升级为 agenthub-webd 2.1.15，服务为 active、健康为 ok；Mac 已使用同版正式 DMG 从 2.1.14 原地更新至 2.1.15 并启动，未保留旧版副本。未执行回退，成功安装已清理临时备份；真实原生删除与日常界面操作待用户验收。

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
