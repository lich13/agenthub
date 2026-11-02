#!/usr/bin/env python3
"""Render concise Chinese release notes for GitHub Releases and updater metadata."""

from __future__ import annotations

import argparse
import sys


HIGHLIGHTS: dict[str, list[str]] = {
    "2.0.2": [
        "修复 Probe 工作台边框令牌自引用、胶囊开关样式、全局 overflow-wrap 逐字竖排及 13 个缺失样式类回归，为 Codex 补齐黑白品牌令牌并将全局默认强调色切换为 AgentHub 星轨蓝",
        "根治 Antigravity Stop Hook 同步门禁期间 settled_count 游标抢跑导致的结尾通知漏发，新增 diagnose-antigravity 诊断命令、五平台通知端到端自检、通知分组自动迁移及 Hook 归属严格校验。",
    ],
    "2.0.1": [
        "全面统一 AgentHub 品牌文案，将 styles.css 深度重构并真实减半至 47.8 KB（单行最长 136 字符），提取 useProviderWorkspace、StandardProviderEventView 与 SessionDeleteConfirmDialog 精简全部七大工作区",
        "修复并自愈 Antigravity 顶层 agenthub Stop Hook 与 Grok Hook 宿主配置，消除 Antigravity／DSH／Grok／Claude 跨模块重复代码，并将 Codex 清理预扫描与 Probe 活跃扫描优化为单次流式读取。",
    ],
    "2.0.0": [
        "重绘无平台数量限制的「Command Nexus（多轨星枢）」图标，将 styles.css 彻底重写压减至 566 行纯净三层令牌，提取 WorkspaceSplitShell、useCopyFeedback 与五平台统一 AgentHookHostCard 重构全部七大工作区",
        "修复 Antigravity 顶层 agenthub Stop Hook 契约（stdout 输出 allow 放行决策）与 Grok 重复 Hook 清理，合并 Antigravity 端点缓存并消除存活进程瞬时负缓存，移除缓存 #[cfg(not(test))] 门禁、补齐环境隔离键与空闲回收，并将 Probe 扫描 I/O 减半。",
    ],
    "1.9.0": [
        "品牌正式焕新为 AgentHub（AgentHub.app 与全新钛黑五节点星枢图标），落地 --bg-rail／--bg-list／--bg-stage 三层纯正 ChatGPT macOS 桌面表面，Probe 升级为首屏控制台并新增 Grok Build 与 Antigravity 受管实时 Hooks",
        "修复 DeepSeek Harness 已打开完成线程常亮转圈与派生会话状态，新增五平台热路径内存缓存与 Codex O(K)／单次扫描清理加速，精简统一清理引擎并实现全部测试 100% 沙箱隔离。",
    ],
    "1.8.0": [
        "支持保存多台自定义名称远程机器并自动无损迁移钥匙串凭据，Probe 升级为五大平台一键通知开关、统一 Hooks 卸载与五平台状态对齐，全平台清理严格按预览目标指纹取交集",
        "提取本机五大 Agent 官方原生高保真品牌图标并全量重写三层 macOS 桌面界面，同时补齐 Grok／Claude 轻量巡检缓存、详情内存收敛与全仓库隐私边界加固。",
    ],
    "1.7.0": [
        "修复 Antigravity 线程中间提问 Bark 漏推与执行段原数据核验，整合覆盖五大 Agent 六大分类的「全平台线程清理」工作台并全面加固执行端删除边界。",
        "全面换装五大 Agent 官方原生品牌图标与高对比原生配色，深度重写 macOS 原生质感界面，并精简服务端 SQLite／内存缓存／HTTP 压缩与前端轮询包体。",
    ],
    "1.6.0": [
        "加固五类 Agent 交接文件路径，修复 Antigravity 闲置会话常亮运行态、超长会话在线／离线过程折叠、Bark 增量扫描与最终必达，以及 Claude Code 穿插元数据与错误结尾的过程折叠。",
        "整体前端深度重写为三层冷暖空气感表面与七大工作区专属宝石色调体系，并将标签发布流水线升级为自包含并行门禁以缩短发版耗时。",
    ],
    "1.5.0": [
        "恢复原生二进制公网 IP 隐私门禁，收紧 Antigravity 与 DSH 子会话删除归属校验、运行态拦截与残留报告，并统一五类 Agent 最优交接文件路径。",
        "移除顶部状态筛选栏并改为底部轻量折叠「已归档」，切换访达风格 ⌘ 多选与连续中间过程单框折叠，修复五类 Agent 时间线定位及 Antigravity Bark 过早提醒。",
    ],
    "1.4.0": [
        "新增 Antigravity 与 DeepSeek Harness 会话工作区，支持全部／已归档筛选、分页阅读、搜索、时间线、双击改名、归档／恢复与永久删除。",
        "整体前端界面深度重构为 ChatGPT macOS App 风格，统一七项导航、圆角气泡、代码块、工具披露、弹窗与设置卡片。",
    ],
    "1.3.11": [
        "新增本机 Antigravity 与 DSH 的完成、失败、需要回复、需要授权 Bark 提醒，沿用 Probe 轮询及现有通知配置。",
        "首发和重试前复核原生身份与状态，恢复执行或处理交互后停止旧提醒，首次启用不补发历史。",
    ],
    "1.3.10": [
        "修复 Claude StopFailure 首次发送与重试前的回合校验；新回合、恢复执行、成功或取消后不再投递旧失败提醒，同时保留有效旧回合的完成补发。",
        "新增本机和远端 DSH 归档清理，支持明文与 Zstandard 会话、可核验的 subagent 后代、预览确认、并发保护、逐项重试和自定义 DSH_HOME。",
    ],
    "1.3.9": [
        "修复 Grok 和 Claude 删除共享索引时的 systemd 只读错误，按配置开放数据根并继续保护凭据与配置。",
        "Claude 命令回显、通知和上下文归入默认折叠的系统活动，保留真实输入、搜索与分页定位。",
    ],
    "1.3.8": [
        "统一 Codex、Claude Code 和 Grok 的直接删除入口，移除活动、锁、版本、指纹和内容完整性限制，补齐线程专属旁路与共享索引清理。",
        "修复 Claude 空会话读取，并在 Grok 缺少 updates.jsonl 时读取原生 chat_history.jsonl，系统提示和加密推理不作为普通对话显示。",
    ],
    "1.3.7": [
        "兼容 Claude 原生身份与版本字段的并存记录，修复合法会话无法改名、删除和通知扫描的问题。",
        "Codex 官方删除成功后核验并清理 writer lock、shell snapshot 和 visualization 旁路；浏览器验收统一使用 Playwright 管理的浏览器。",
    ],
    "1.3.6": [
        "Codex 支持压缩 rollout 的只读核验和官方整树删除，补齐明文与压缩历史引用复检",
        "修复 Claude 会话归属与专属文件管理，并将同回合过程折叠、最终答复和搜索定位分开显示。",
    ],
    "1.3.5": [
        "统一三种 Provider 的多选与菜单，修复 Codex 整树删除和 Claude 会话管理",
        "Bark 改为简洁单条摘要，新增需手动启用的本机 Claude hooks，保留 Codex 逐问题提醒。",
    ],
    "1.3.4": [
        "按实际协议判断桌面兼容性，移除无意义的 build 提示，修复附件仅元数据变化时被误拒",
        "支持 Finder 文件、完整路径文本和 PNG/TIFF/JPEG 粘贴，保留文件安全校验与原生桌面执行。",
    ],
    "1.3.3": [
        "修复本机 Codex 应用目录、思考强度与提交等待，失败时提供安全诊断",
        "整理协作输入区，修复引用和附件提示，同步只读原生目标，保留桌面执行与结果不明不重发。",
    ],
    "1.3.2": [
        "本机 Codex 协作支持拖入文件与粘贴图片，发送后保留图片供原生历史读取",
        "审批卡按命令和文件整理，补齐协作状态与连接提示，保持原生桌面执行和协议 2。",
    ],
    "1.3.1": [
        "本机 Codex 支持输入 $ / @ 选择技能、应用和插件，保留原生桌面执行与加密队列",
        "改进目录重试、协作进度刷新与协议错误提示，支持已核实的命令和文件会话审批。",
    ],
    "1.3.0": [
        "本机 Codex 主线程支持新建、续聊、排队、补充与停止，仍由原生桌面执行",
        "接入模型、模式、引用及待处理请求，保留远程与其他 Provider 的现有阅读和管理功能。",
    ],
    "1.2.23": [
        "限制 API 回环监听、受控更新命令和 Bark 地址，保持现有功能与协议兼容",
        "Linux 更新增加分离签名验证，规范解包权限并强化依赖审计与构建门禁。",
    ],
    "1.2.22": [
        "加固迁移、管理操作和安装回滚，保留现有会话与兼容行为",
        "整理读取模块、资源预算与文档，统一远程机器文案并补强契约检查。",
    ],
    "1.2.21": [
        "自动上下文合并为一行默认收起，保留真实请求和附件，搜索排除自动字段",
        "缩窄时间线预览，补齐全部用户记录的历史定位与双向分页。",
    ],
    "1.2.20": [
        "修复 Grok 闲置线程因历史回合未闭合而被误保护的问题",
        "隐藏线程自动分批扫描，显示进度、支持停止，并限制每页显示数量。",
    ],
    "1.2.19": [
        "已回答问题保留原提问、选项和回答，搜索命中的提问文字可直接阅读",
        "延续长线程性能优化、历史分页和 100M／300M 线程颜色规则。",
    ],
    "1.2.18": [
        "搜索可展开并定位折叠的旧问答、工具结果与 AGENTS.md 指令。",
        "保留其他手动折叠选择，修复成功定位后仍显示旧失败提示的问题。",
    ],
    "1.2.17": [
        "减少长线程重复读取和历史轮询开销，改善页面切换、工具展开与滚动响应",
        "Grok 支持上滚分页，三种 Provider 按 100M／300M 统一线程大小颜色。",
    ],
    "1.2.16": [
        "向上滚动自动加载较早消息，保留当前阅读位置，搜索与分页协同工作。",
        "Codex、Claude Code、Grok 线程列表显示 K／M／G 大小，支持本机和远程服务器。",
    ],
    "1.2.15": [
        "Bark 网络异常后有限重试，重启继续未确认分段，减少漏推。",
        "细分安全错误与中文投递状态，统一 AgentHub 测试文案并移除旧配置导入。",
    ],
    "1.2.14": [
        "移除 Gotify 设置和推送能力，通知统一使用 Bark。",
        "升级清理旧通道配置与投递数据，保留 Bark 去重状态和 AGENTS.md 完整折叠修复。",
    ],
    "1.2.13": [
        "修复 AGENTS.md 原生指令只折叠开头的问题，完整正文默认收起，保留手动展开选择。",
    ],
    "1.2.12": [
        "精简子智能体汇总和清理面板，移除三处重复说明，保留状态数量与实际操作结果。",
        "延续 1.2.11 的开始活动、统一任务名称、直属统计及内部页面标记过滤。",
    ],
    "1.2.11": [
        "修复 Codex 子智能体开始活动漏显示及任务名称不一致，开始记录在任务完成后仍保留。",
        "线程详情新增直属子智能体状态汇总，可从列表进入详情并返回；父线程静止时仍更新数量。",
        "默认隐藏内部页面标记，显示、复制、导出和搜索同步清理，保留代码与引用示例。",
    ],
    "1.2.10": [
        "修复切换远程机器后列表和设置持续加载的问题，钥匙串访问改为后台串行执行。",
        "增加凭据读取超时反馈，保留连接事务与机器切换保护，已有连接无需重建。",
    ],
    "1.2.9": [
        "统一 Codex、Claude Code 和 Grok 的紧凑阅读布局，正文与活动摘要对齐，复制按钮不再占据额外一行。",
        "新增 Codex 子智能体活动与右侧只读详情，支持嵌套返回、分页、工具记录和附件。",
        "修复 Hook 集成测试的 Cargo 二进制路径依赖，保留真实 CLI 和通知去重验证。",
    ],
    "1.0.0": [
        "Codex 支持批量归档、恢复与删除，Grok 和 Pi 支持批量删除。",
        "扩展 Bark 完成和失败通知，并移除已退役的日志维护入口。",
        "完成隐私清理和仓库结构整理，远程部署参数继续从仓库外提供。",
    ],
    "1.1.0": [
        "移除 AgentHub 自有 Goal 与自动恢复链路。",
        "Codex、Grok、Pi 统一显示运行状态，并将连续命令整理为可折叠执行组。",
        "发布资产收敛为 macOS App、updater 和 Linux webd 服务包。",
    ],
    "1.1.1": [
        "修复 Grok 原生 Execute 工具未被归入命令执行组的问题。",
        "连续执行活动可合并查看，保留每项工具结果。",
    ],
    "1.1.2": [
        "改进 Grok 工具活动识别和连续命令分组。",
        "回复操作栏与双击改名交互更接近 Codex 桌面端。",
    ],
    "1.1.3": [
        "修复 Grok 连续工具折叠和长会话滚动空白。",
        "为 Codex 与 Grok Plan 增加复制和 Markdown 下载。",
    ],
    "1.1.4": [
        "隐藏结构化记忆元数据，显示、复制和导出使用同一清理规则。",
        "AGENTS.md 内容默认折叠，并保留用户手动展开状态。",
    ],
    "1.1.5": [
        "为 Codex 普通提问增加 Bark 提醒。",
        "改进 Markdown 文件路径复制、Finder 定位和更新维护设置。",
    ],
    "1.1.6": [
        "统一 Codex、Grok、Pi 的用户消息气泡和附件展示。",
        "保留原始换行与 Markdown 字符，图片支持按需预览。",
    ],
    "1.1.7": [
        "恢复历史线程中文本与工具活动的原始交错顺序。",
        "完成工具组默认收起、活动状态和滚动跟随优化。",
    ],
    "1.1.8": [
        "结构化问答显示为问题引用与答案气泡。",
        "用户消息中的 AGENTS.md 指令默认折叠并支持独立复制答案。",
    ],
    "1.1.9": [
        "识别 request_user_input_async 的未回答问题并发送 Bark 提醒。",
        "统一 Hook、后台 monitor 和去重记录，异步确认不会误清除待回答问题。",
    ],
    "1.2.0": [
        "下线网页界面，改为由 macOS App 通过 API Key 管理远程机器。",
        "增加本机与远程机器快速切换，并隔离各机器的查询和操作状态。",
    ],
    "1.2.1": [
        "修复 Plan 在 macOS WebKit 中下载停滞，改用原生文件保存。",
        "修复 Grok 改名进程在服务只读隔离下无法正常工作的路径。",
    ],
    "1.2.2": [
        "修复 Grok 已结束线程仍显示运行中的问题。",
        "统一本机 App 与远程服务的更新面板和按钮状态。",
    ],
    "1.2.3": [
        "新增 Claude Code 会话读取、搜索、附件查看和安全管理。",
        "未知格式或活动状态不明确时保持只读，远程服务器不安装 Claude。",
    ],
    "1.2.4": [
        "新增当前线程和当前 Provider 的会话搜索。",
        "增加 Codex 风格时间线，可定位消息、Plan、附件和工具组。",
    ],
    "1.2.5": [
        "修正桌面安装包的版本信息，使应用显示与发布版本一致。",
        "修复服务器更新在 systemd 只读隔离下无法写入的问题。",
    ],
    "1.2.6": [
        "修复本机与远程服务器切换后更新状态和能力缓存串机。",
        "统一本机 App 与远程服务器服务的更新反馈，保留远程 API Key 和监测配置。",
    ],
    "1.2.8": [
        "线程右键直接打开操作菜单，Codex 普通与归档线程支持预览后永久删除。",
        "隐藏线程清理逐项跳过受保护项目，修复列表宽度并优化一次性启动窗口适配。",
        "移除 AgentHub 的 Pi 集成，保留原生 Pi 程序与用户数据；远程 API 协议升级为 2。",
    ],
    "1.2.7": [
        "左侧时间线只标记用户发送指令的位置，滚动时高亮对应指令；助手回复和工具内容仍可搜索定位。",
        "修复 Claude Code 新版辅助记录被误报为读取错误的问题，兼容性提醒与致命错误分别显示，安全管理限制继续保留。",
        "修复 Claude Code 完成 Bark 漏推，支持空结果回退；工具执行中、取消或已有新指令时不误报完成，升级不补推历史通知。",
    ],
}


def highlights(version: str) -> list[str]:
    return HIGHLIGHTS.get(version, ["修复问题并改进稳定性。"])


def render(version: str, *, updater: bool = False) -> str:
    if updater:
        return f"AgentHub {version}：" + "；".join(highlights(version)[:2])

    if version in ("2.0.2", "2.0.1", "2.0.0"):
        sections = {
            "版本概览": [
                "重新设计无平台数量绑定的「Command Nexus（无界多轨星枢）」应用图标，将 styles.css 彻底重写并精简至 566 行纯净三层表面令牌体系，提取 WorkspaceSplitShell、useCopyFeedback 与五平台统一 AgentHookHostCard 深度重构全部七大工作区；修复 Antigravity 顶层 agenthub Stop Hook 契约（向 stdout 输出 allow 放行决策）与 Grok 遗留重复 Hook 清理，合并 core／webd Antigravity 端点缓存并消除存活进程瞬时失败 5 秒负缓存，移除缓存 #[cfg(not(test))] 门禁、补齐环境隔离键与 Grok 1 秒进程缓存并统一纳入空闲回收，将 Probe 活跃扫描磁盘 I/O 减半并将 DSH 事件解析降至 O(events) 线性复杂度；API 协议保持 2。"
            ],
            "功能调整": [
                "多轨星枢图标与全工作区深度重写：重绘平台无关的「Command Nexus（多轨星枢）」矢量图标并导出全套 macOS／Windows／Web 图标资产；彻底重写 styles.css（从 1,189 行压减至 566 行），废除全部历史兼容别名，确立 --bg-rail、--bg-surface、--bg-stage、--bg-elevated 与 ws-* 核心原语；提取 WorkspaceSplitShell 与 useCopyFeedback 统一重构 Codex、Claude Code、Grok Build、Antigravity、DeepSeek Harness、Probe 与设置七大工作区。",
                "Probe 单视图精简与五平台统一 AgentHookHostCard：移除 Probe 标题下无意义的运行态胶囊行与冗余三栏切换器，hooksHostMatrix 全局只渲染一次；后端直接返回真实宿主 config_path，前端零硬编码路径，五张宿主卡片统一采用轻量弹窗确认流，DSH 卡片改为只读原生直连状态条并移除永久禁用按钮。",
                "服务端 CPU／内存／磁盘深度减负：合并 core 与 webd 重复的 Antigravity 端点缓存，仅在无 language_server 进程时缓存 5 秒空结果；新增 Grok 进程快照 1 秒短缓存，移除 READONLY_SNAPSHOT_CACHE 与 LIVE_PROVIDER_SESSIONS_CACHE 的 #[cfg(not(test))] 门禁，补齐 sandbox_root／codex_home／paths.home 隔离键并全部纳入 evict_idle_detail_caches；在 aggregate_active_sessions 中单次扫描复用会话与通知快照，磁盘与子进程开销直接减半。",
            ],
            "问题修复": [
                "Antigravity 与 Grok Hook 契约对齐：Antigravity 改用 ~/.gemini/config/hooks.json 标准顶层 agenthub 结构（仅注册 Stop 事件）并自动清理旧版污染的顶层 hooks 键，agenthub-webd antigravity-hook 恒向 stdout 输出 {\"decision\":\"allow\"} 并以退出码 0 结束；Grok 安装／修复时自动清理同属 agenthub-webd 的遗留 claude-hook 重复条目。",
                "清理执行器、目录枚举与 DSH 复杂度优化：在 cleanup.rs 提炼 run_scope_targets 将五作用域执行段压缩至约 75 行；在 selected_codex/tree.rs 提炼 visit_codex_tree_files 让 inventory 与 estimate_bytes_batch 共享唯一路径发现真相源；DSH load_all_events 改用未决工具下标栈，将复杂度从 O(轮数×事件数) 降至 O(events) 线性时间。",
                "全仓测试沙箱化与新增单测覆盖：将剩余测试中的 Config::default() 全面切换为 TempDir 沙箱配置，并补齐批量体积估算、O(K) 隐藏删除、各缓存命中／失效／目录隔离及 Antigravity Hook stdout 契约的直接单元测试。",
            ],
            "兼容性与迁移": [
                "API 协议保持 2；底层 Bundle ID（com.lich13.agenthub）、配置文件、SQLite 数据库、macOS Keychain 凭据与远端 agenthub-webd 守护进程名称保持不变，无需手工迁移任何数据。",
                "升级后本机守护进程会自动将已安装的旧版 Antigravity／Grok Hook 配置平滑升级为 2.0.0 规范格式，并完整保留用户自定义第三方 hooks。",
            ],
            "支持平台与资产": [
                "macOS ARM64：AgentHub DMG／校验文件、updater 压缩包／签名及仅映射 darwin-aarch64 的 latest.json。",
                "Linux x86_64：API 服务包、SHA-256 校验文件与分离签名；共八项正式资产。",
            ],
            "升级提示": [
                "先升级远端 API 服务，再原地覆盖安装本机 macOS App（AgentHub.app）；随后核验多轨星枢图标与七大工作区新界面、Probe 单视图与五平台统一宿主卡片、Antigravity／Grok Hook 正常触发以及全平台线程清理。",
                "DSH 归档、恢复与清理前须退出目标机器正在运行的 DSH 进程；真实用户会话清理、Bark 手机实收与日常视觉交互待用户验收。",
                "macOS 包没有 Developer ID 签名或 Apple 公证；Minisign 验证归档来源，不能替代系统代码签名。",
            ],
        }
        lines = [f"# AgentHub {version}", ""]
        for heading, paragraphs in sections.items():
            lines.extend([f"## {heading}", ""])
            lines.extend(f"- {paragraph}" for paragraph in paragraphs)
            lines.append("")
        return "\n".join(lines)

    if version == "1.9.0":
        sections = {
            "版本概览": [
                "品牌正式焕新为 AgentHub（应用包 AgentHub.app 与全新 macOS Sequoia 钛黑五节点星枢图标），修复 DeepSeek Harness 已完成线程转圈误判并落地五平台热路径内存缓存，将 Probe 重构为首屏控制台并新增 Grok Build 与 Antigravity 受管实时 Hooks，瘦身统一清理引擎并实现 Codex 隐藏／归档清理 O(K) 定向校验与 O(N) 单次目录扫描，落地 --bg-rail／--bg-list／--bg-stage 三层纯正 ChatGPT macOS App 表面体系；API 协议保持 2。"
            ],
            "功能调整": [
                "AgentHub 品牌焕新与三层 ChatGPT macOS 界面重写：正式更名为 AgentHub（macOS 应用包 AgentHub.app、安装包 AgentHub-1.9.0-darwin-arm64.dmg，底层标识保持 com.lich13.agenthub 零迁移），重新设计高清深空钛黑五节点星枢图标；完整重写 styles.css 引入 --bg-rail、--bg-list、--bg-stage 三层纯正中性灰阶表面并收敛工作区共享 Hooks。",
                "Probe 首屏控制台与五平台统一 Hooks／Host 矩阵：进入 Probe 默认首屏直接呈现主控开关、五大 Agent 平台级通知开关、Bark 配置与测试推送、五平台统一 Hooks／Host 管理卡片、实时态势与最近事件；新增 Grok Build（~/.grok/hooks/agenthub.json）与 Antigravity（~/.gemini/config/hooks.json）受管 Hooks 一键安装／修复／移除与毫秒级实时通知接入，与 Codex、Claude Code 及 DSH 原生流形成统一管理体验。",
                "服务端热路径缓存与清理引擎提速：新增 DSH .zstd 折叠元数据内存缓存、Antigravity 本地进程端口探测缓存、Claude 活动快照 2 秒短缓存与 canonicalize 循环外提、probe.status 3 秒会话快照缓存；精简 execute_unified_with_capability 消除约 260 行重复代码，并将 Codex 清理预览与隐藏删除优化为 O(K) 定向校验与 O(N) 单次子树体积扫描。",
            ],
            "问题修复": [
                "DeepSeek Harness 状态精准识别：移除 dsh/sessions.rs 中 || locked 冗余分支，消除在桌面端保持打开但早已 turn/end 的完成线程持续转圈的问题；对齐 Seeded 派生会话事件边界并在 turn/end 时收口未闭合工具调用状态。",
                "测试 100% 沙箱隔离与前端后台轮询收口：将 agenthub-webd 全部剩余裸 Config::default() 测试调用统一替换为独立 TempDir 沙箱配置；统一在 shared.ts 收口页面可见性感知轮询，窗口隐藏时自动暂停轮询。",
                "文档全面重写与校准：从零重写 README.md 修正八处历史失实描述，同步更新 DESIGN.md 与 MASTER.md（修正 DETAIL_CACHE_IDLE_TTL 300 秒与五平台能力对齐记录）。",
            ],
            "兼容性与迁移": [
                "API 协议保持 2；底层 Bundle ID（com.lich13.agenthub）、配置文件、SQLite 数据库、macOS Keychain 凭据与远端 agenthub-webd 守护进程名称保持不变，无需手工迁移任何数据。",
                "本机安装脚本在安装 /Applications/AgentHub.app 时会自动检测并平滑替换旧版 /Applications/AgentHub.app，不保留旧版副本。",
            ],
            "支持平台与资产": [
                "macOS ARM64：AgentHub DMG／校验文件、updater 压缩包／签名及仅映射 darwin-aarch64 的 latest.json。",
                "Linux x86_64：API 服务包、SHA-256 校验文件与分离签名；共八项正式资产。",
            ],
            "升级提示": [
                "先升级远端 API 服务，再原地覆盖安装本机 macOS App（AgentHub.app）；随后核验新品牌图标与三层中性界面、DSH 已完成线程状态、Probe 首屏控制台与 Grok／Antigravity Hooks 一键安装，以及全平台线程清理。",
                "DSH 归档、恢复与清理前须退出目标机器正在运行的 DSH 进程；真实用户会话清理、Bark 手机实收与日常视觉交互待用户验收。",
                "macOS 包没有 Developer ID 签名或 Apple 公证；Minisign 验证归档来源，不能替代系统代码签名。",
            ],
        }
        lines = [f"# AgentHub {version}", ""]
        for heading, paragraphs in sections.items():
            lines.extend([f"## {heading}", ""])
            lines.extend(f"- {paragraph}" for paragraph in paragraphs)
            lines.append("")
        return "\n".join(lines)

    if version == "1.8.0":
        sections = {
            "版本概览": [
                "新增多台命名远程机器管理与钥匙串凭据无损迁移，升级 Probe 跨五平台实时态势、五大平台通知一键开关、Codex Hook 安全卸载与五平台工作区能力对齐，加固全平台清理预览目标指纹交集与受控残留防护，提取五大 Agent 官方原生品牌图标并全量深度重写前端界面；API 协议保持 2。"
            ],
            "功能调整": [
                "多远程主机管理：支持保存多台自定义名称远程主机、侧栏一键切换与独立编辑移除，API Key 按机器 ID 独立存储于系统钥匙串，旧版单远程配置首次启动时自动无损迁移，不同主机切换自动隔离前端查询作用域。",
                "Probe 与五平台向 Codex 对齐：移除顶部冗余横幅，通知配置精简为五大 Agent 平台一键开关并修复保留天数重复字段；新增 `probe.removeHooks` 支持安全卸载 Codex Hook 并统一整合 Codex／Claude Hooks 管理面板；补齐 Grok `reply_needed` 提问提醒、跨五平台实时活跃态势以及五工作区状态芯片、复制答复与本机 Finder 显示能力（清理前端废弃死代码与冗余轮询包装）。",
                "五大 Agent 原生品牌图标与全量界面深度重写：从本机官方应用包精确提取与矢量化 Codex、Claude Code、Grok Build、Antigravity、DeepSeek Harness 原生图标，全量重写 `styles.css` 三层 macOS 原生表面体系、七工作区官方品牌色盘、胶囊搜索、原生滑动拨杆开关、过程折叠卡片、多远程管理台与空间占比清理工作台。",
            ],
            "问题修复": [
                "清理安全边界加固：`cleanup.unifiedExecute` 强制将当下重算允许集与用户预览目标 ID 及非空指纹取交集（预览后新增项零误删，Codex 隐藏线程改为按目标 ID 定向重验）；移除 `archive.rs` 无条件孤儿文件放行分支，仅允许受控部分失败残留重试。",
                "服务端性能与资源收敛：为 Grok 与 Claude 巡检建立 32 MiB 轻量文件身份快照缓存，解除后台巡检对重型详情缓存的常驻占用；Grok 列表改为元数据排序截断；详情缓存总预算收紧约 68% 且闲置 TTL 缩短至 5 分钟；监控守护进程复用配置文件与数据库连接，通知历史裁剪后自动执行 `wal_checkpoint(TRUNCATE)` 回收磁盘体积。",
                "Antigravity、提问识别与去重隔离修复：Antigravity 允许已落定 `reply_needed` 闲置流命中缓存、增量拉取 `ExecutorMetadatas` 尾部段，并按进程树／窄启动时间窗／命令行精确匹配未跟踪子进程；`trailing_user_question` 收紧为仅检查末段尾句；为仍在最近 N 列表内的跨 7 天未答提问自动续期去重标记；补齐集成测试独立临时主目录隔离与全仓库历史对象隐私门禁。",
            ],
            "兼容性与迁移": [
                "API 协议保持 2；新增 `probe.removeHooks` 命令、统一清理 `targets` 指纹列表与多远程主机列表字段，连接旧版远端服务时自动兼容回退。",
                "旧版单台远程主机配置与系统钥匙串凭据在升级后首次读取时自动迁移至多主机结构，无需手工重新输入 API Key。",
            ],
            "支持平台与资产": [
                "macOS ARM64：DMG／校验文件、updater 压缩包／签名及仅映射 darwin-aarch64 的 latest.json。",
                "Linux x86_64：API 服务包、SHA-256 校验文件与分离签名；共八项正式资产。",
            ],
            "升级提示": [
                "先升级远端 API 服务，再原地覆盖安装本机 macOS App；随后核验五大 Agent 原生品牌图标与新界面、「设置 → 远程连接」多机器保存与切换、Probe 平台通知一键开关与 Hooks 卸载，以及「全平台线程清理」预览与勾选执行流程。",
                "DSH 归档、恢复与清理前须退出目标机器正在运行的 DSH 进程；真实用户会话清理、Bark 手机实收与日常视觉交互待用户验收。",
                "macOS 包没有 Developer ID 签名或 Apple 公证；Minisign 验证归档来源，不能替代系统代码签名。",
            ],
        }
        lines = [f"# AgentHub {version}", ""]
        for heading, paragraphs in sections.items():
            lines.extend([f"## {heading}", ""])
            lines.extend(f"- {paragraph}" for paragraph in paragraphs)
            lines.append("")
        return "\n".join(lines)

    if version == "1.7.0":
        sections = {
            "版本概览": [
                "修复 Antigravity 线程中间提问 Bark 漏推与执行段原数据核验，整合五大 Agent「全平台线程清理」并加固执行端删除边界，全面换装官方原生品牌图标配色与深度重写界面，同时精简服务端与前端资源占用；API 协议保持 2。"
            ],
            "功能调整": [
                "五大 Agent（Codex、Claude Code、Grok Build、Antigravity、DeepSeek Harness）全面切换为官方原生矢量品牌图标与浅／深色高对比官方品牌色盘（侧栏常驻品牌徽章、工作区标题徽章、空状态徽章与统一清理分类色带），并深度精修三层 macOS 原生表面层次、会话卡片、ChatGPT 风格折叠胶囊与用户气泡。",
                "新增 `cleanup.unifiedDryRun` 与 `cleanup.unifiedExecute` 并整合为「全平台线程清理」工作台，一键覆盖五大 Agent 六大分类（Codex 已归档／隐藏线程、Antigravity 已归档、DSH 已归档、Claude Code 空／废弃会话、Grok 空／废弃会话），支持按分类勾选、流式扫描隐藏线程、100 条分页候选预览、二次确认及删除数／回收空间／跳过失败项分开报告，同时完整保留既有单平台清理 RPC 兼容。",
                "系统与性能精简：服务端新增 SQLite 投递标记 90 天裁剪与空闲 WAL `TRUNCATE` 检查点、Codex 与 Antigravity 会话内存缓存有界淘汰、按修改时间失效的 Claude 列表轻量头部摘要缓存，以及 `/api/*` 自动 gzip 响应压缩；前端在窗口隐藏时暂停轮询并将演示桩模块改为按需动态加载。",
            ],
            "问题修复": [
                "Antigravity Bark 通知补齐 `ask_question` 活跃与挂起提问识别，以及执行完毕但以直接提问收尾且尚未收到用户新消息时的 `needs_reply` 提醒；当前轮增量拉取改为读取原生 `executorMetadatas` 核验真实段尾步下标与终止原因，并对无日志后台命令核查子进程存活，消除两段之间短暂闲置或长时静默构建引发的提前误发。",
                "加固 Claude Code 与 Grok 删除执行端防护：不信任调用方传入路径或预览列表，执行端重新枚举并校验真实会话根目录、逐项检查运行／占用状态，首错即停并报告已删与残留项；Claude 清理前先剔除 history 共享索引再删主文件与旁路。",
                "修复 Claude 连续过程组合并时丢失被吸收组 `sourceIds` 导致的手动展开状态与搜索自动展开失效问题；清理 Grok 过时死锁文件时仅移除已确认失效的 `.lock` 并容忍并发 `NotFound` 竞态。",
            ],
            "兼容性与迁移": [
                "API 协议保持 2，新增可选能力字段 `unified_cleanup`；连接尚未升级的远端服务时自动按能力矩阵降级提示，既有单平台清理接口、本地数据库、Bark 配置、Keychain 远端连接与各 Agent 原生会话目录无需迁移。",
                "服务端 `/api/*` gzip 压缩按客户端 `Accept-Encoding` 自动协商，不影响现有桌面端与脚本调用。",
            ],
            "支持平台与资产": [
                "macOS ARM64：DMG／校验文件、updater 压缩包／签名及仅映射 darwin-aarch64 的 latest.json。",
                "Linux x86_64：API 服务包、SHA-256 校验文件与分离签名；共八项正式资产。",
            ],
            "升级提示": [
                "先升级远端 API 服务，再原地覆盖安装本机 macOS App；随后核验五大 Agent 官方品牌图标配色、「设置 → 更新与维护 → 全平台线程清理」扫描预览与确认流程、Claude 合并过程组展开保持，以及 Antigravity 线程中间提问 Bark 推送。",
                "DSH 归档、恢复与清理前须退出目标机器正在运行的 DSH 进程；真实用户会话清理、Bark 手机实收与日常视觉交互待用户验收。",
                "macOS 包没有 Developer ID 签名或 Apple 公证；Minisign 验证归档来源，不能替代系统代码签名。",
            ],
        }
        lines = [f"# AgentHub {version}", ""]
        for heading, paragraphs in sections.items():
            lines.extend([f"## {heading}", ""])
            lines.extend(f"- {paragraph}" for paragraph in paragraphs)
            lines.append("")
        return "\n".join(lines)

    if version == "1.6.0":
        sections = {
            "版本概览": [
                "加固五类 Agent 交接文件路径、Antigravity 运行状态／超长会话过程折叠／Bark 双向可靠性与 Claude Code 复杂回合过程折叠，深度重写多色调宝石视觉界面，并将标签发布流水线升级为自包含并行门禁；API 协议保持 2。"
            ],
            "功能调整": [
                "五类 Agent「复制路径」交接来源边界加固：Codex 优先解析真实在盘的明文、归档或压缩 rollout 文件；Grok Build 过滤零字节占位文件并支持回退至非空 updates.jsonl、events.jsonl 或 summary.json；Claude Code、Antigravity 与 DeepSeek Harness 保持完整无损日志优先级。",
                "整体前端升级为三层冷暖空气感表面层次与七大工作区专属宝石色调体系：导航栏常驻七色宝石图标与激活柔光指示条，会话选中卡片、胶囊搜索框、云青色用户消息气泡、深海蓝行内代码与顶栏色带代码块、过程折叠胶囊、左侧大纲横线及多色顶栏仪表盘卡片在浅／深色双主题下均满足高对比易读性。",
                "标签发布流水线改为自包含并行架构：Release guard 前置核验六章节发布说明，随后并行启动完整前后端／Tauri CI 门禁与 Linux／macOS 双平台签名打包，支持主分支提交与发布标签同推。",
            ],
            "问题修复": [
                "Antigravity 列表与详情读路径彻底移除 presence 文件锁探测，在线状态严格对齐原生 RPC 活跃轨迹与活跃子代理（已结束但仍在窗口打开的会话不再常亮「运行中」或一直转圈），离线删除改用非阻塞只读共享锁探针避免抢写锁；子会话索引库读取失败时显式报错阻断删除，杜绝静默遗留孤儿子会话。",
                "本机 Antigravity 环回 RPC 响应上限提升至 256 MiB（远端 HTTP 仍保持 32 MiB），Bark 监测改用当前轮增量步拉取消除超长会话超限漏报；同时结合在线子代理运行态、后台任务日志句柄／修改时间与有界交接宽限窗口，既防止提前误发，也保证子代理或后台任务无系统消息退出时最终必达。",
                "修复 Antigravity 离线转录 GENERIC 工具输出误判为正文、多轮唤醒回合归属与过渡说明打断折叠的问题；修复 Claude Code 在工具调用间穿插自动记账／上下文记录及回合以网关错误加尾部记账收尾时整轮未折叠的问题。",
            ],
            "兼容性与迁移": [
                "API 协议保持 2；既有本地数据库、Bark 通知配置、Keychain 远端连接与五类 Agent 原生会话目录无需迁移。",
                "远端 HTTP 响应体积上限保持不变，新增 256 MiB 限额仅作用于本机环回 Antigravity Connect-RPC。",
            ],
            "支持平台与资产": [
                "macOS ARM64：DMG／校验文件、updater 压缩包／签名及仅映射 darwin-aarch64 的 latest.json。",
                "Linux x86_64：API 服务包、SHA-256 校验文件与分离签名；共八项正式资产。",
            ],
            "升级提示": [
                "先升级远端 API 服务，再原地覆盖安装本机 macOS App；随后核验五类 Agent 复制路径、Claude Code 与 Antigravity 长会话单框过程折叠、Antigravity 空闲状态与 Bark 完成提醒，以及多色调新界面。",
                "DSH 归档、恢复与永久删除前须退出目标机器正在运行的 DSH 进程；真实用户会话删除、Bark 手机实收与日常视觉交互待用户验收。",
                "macOS 包没有 Developer ID 签名或 Apple 公证；Minisign 验证归档来源，不能替代系统代码签名。",
            ],
        }
        lines = [f"# AgentHub {version}", ""]
        for heading, paragraphs in sections.items():
            lines.extend([f"## {heading}", ""])
            lines.extend(f"- {paragraph}" for paragraph in paragraphs)
            lines.append("")
        return "\n".join(lines)

    if version == "1.5.0":
        sections = {
            "版本概览": [
                "恢复原生二进制公网 IP 隐私门禁并加固子会话删除安全边界，统一五类 Agent 最优交接路径、访达风格 ⌘ 多选与连续中间过程单框折叠，并深度打磨整体视觉体验；API 协议保持 2。"
            ],
            "功能调整": [
                "统一五类 Agent「复制路径」最优交接文件：Antigravity 优先复制完整纯文本转录日志，Grok Build 补齐行菜单与顶栏复制路径（优先 chat_history.jsonl），Codex 覆盖活跃与归档 rollout，Claude Code 与 DeepSeek Harness 保持明文优先的最优日志路径。",
                "移除 Codex 五类状态栏及 Antigravity／DSH 顶部状态切换，活跃列表更纯净，底部提供按需加载的轻量折叠「已归档」抽屉；移除「多选线程」按钮与行首复选框，改为访达风格 ⌘ 点击切换、Shift 连选、多选右键菜单、⌘A／Escape／⌘⌫ 快捷键与底部轻量选择条。",
                "参考 ChatGPT macOS App 统一折叠同一回合内交替的思考与工具执行过程，保持单执行组 DOM 结构零膨胀；深度精修浅／深色侧边栏、会话选中层次、阅读排版、代码块、时间线刻度与浮层质感。",
            ],
            "问题修复": [
                "消除回环端口解析在 x86_64 机器码中产生的跨字面量立即数片段，恢复发布载荷对原生二进制的完整公网 IP 隐私扫描门禁。",
                "Antigravity 与 DSH 删除执行端重新计算真实子会话闭包并拒绝越界或跨根 ID，逐项核验根与全部子会话运行／持锁状态；在线 RPC 或文件系统首错即停，不误删磁盘文件并准确返回 residuals。",
                "修复带游标分页轮询抹掉首页 timeline、Antigravity 在线批注轮次漏提、Grok 首轮提示缺失及折叠历史定位导致的左侧横线时间线失效；修复 Antigravity 在后台任务、定时器或子代理尚未收敛时提前发送完成 Bark 提醒的问题。",
            ],
            "兼容性与迁移": [
                "API 协议保持 2；线程列表查询收敛为布尔 archived 参数（默认返回活跃会话），并支持直接打开 Codex 已归档线程阅读完整详情。",
                "既有本地数据库、Bark 通知配置、Keychain 远端连接与各 Agent 原生会话目录无需迁移。",
            ],
            "支持平台与资产": [
                "macOS ARM64：DMG／校验文件、updater 压缩包／签名及仅映射 darwin-aarch64 的 latest.json。",
                "Linux x86_64：API 服务包、SHA-256 校验文件与分离签名；共八项正式资产。",
            ],
            "升级提示": [
                "先升级远端 API 服务，再原地覆盖安装本机 macOS App；随后核验五类 Agent 复制路径、底部「已归档」抽屉、访达风格 ⌘／Shift 多选、连续中间过程折叠与左侧横线时间线定位。",
                "DSH 归档、恢复与永久删除前须退出目标机器正在运行的 DSH 进程；真实用户会话删除、Bark 手机实收与日常视觉交互待用户验收。",
                "macOS 包没有 Developer ID 签名或 Apple 公证；Minisign 验证归档来源，不能替代系统代码签名。",
            ],
        }
        lines = [f"# AgentHub {version}", ""]
        for heading, paragraphs in sections.items():
            lines.extend([f"## {heading}", ""])
            lines.extend(f"- {paragraph}" for paragraph in paragraphs)
            lines.append("")
        return "\n".join(lines)

    if version == "1.4.0":
        sections = {
            "版本概览": [
                "新增 Antigravity 与 DeepSeek Harness（DSH）完整会话工作区，并将整体前端界面深度重构为 ChatGPT macOS App 风格；API 协议保持 2。"
            ],
            "功能调整": [
                "侧栏与移动端导航扩展为 Codex、Claude Code、Grok Build、Antigravity、DeepSeek Harness、Probe 与设置七项；两种新工作区提供全部／已归档筛选、搜索、体积统计、运行指示、多选批量归档／恢复／永久删除、分页历史、用户时间线、思考折叠与连续执行组。",
                "Antigravity 与 DSH 线程行菜单及详情操作菜单支持改名、归档／恢复、复制 ID、复制路径与永久删除（不提供复制恢复命令），详情头部支持复制最新答复；Antigravity 兼容在线 Connect-RPC 与离线转录双模，DSH 兼容明文与多帧 Zstandard 会话日志。",
                "全局视觉体系重构为 ChatGPT macOS App 风格：浅／深色中性侧栏与画布、右侧紧凑用户胶囊气泡、无框助手 Markdown 与圆角代码块、胶囊搜索框与分段切换、悬浮协作输入舱、圆角菜单／弹窗及卡片化 Probe／设置面板。",
            ],
            "问题修复": [
                "DSH 列表与详情全程无锁只读；改名通过 session.lock 非阻塞排他锁、80 字节 UTF-8 规范化标题与带校验和的 Zstandard 帧追加完成；归档、恢复与含子代理闭包的永久删除在检测无运行中宿主 DSH 进程后原子更新 workspace.json v2。",
                "Antigravity 在线或离线均可分页读取并按确认目标清理 brain 目录、implicit／annotations／conversations／context_state 旁路及 SQLite 索引记录；确认范围包含已核验子代理后代。",
            ],
            "兼容性与迁移": [
                "API 协议保持 2，新增可选能力字段 antigravity_sessions 与 dsh_sessions；连接尚未升级的远端服务时平滑展示升级提示，不影响其他工作区。",
                "Linux systemd 为 Antigravity 与 DSH 默认及环境变量数据根补充精确写入路径，同时继续保护凭据与配置只读隔离；既有数据库、Bark 配置、Keychain 与原生会话无需迁移。",
            ],
            "支持平台与资产": [
                "macOS ARM64：DMG／校验文件、updater 压缩包／签名及仅映射 darwin-aarch64 的 latest.json。",
                "Linux x86_64：API 服务包、SHA-256 校验文件与分离签名；共八项正式资产。",
            ],
            "升级提示": [
                "先升级远端 API 服务，再覆盖安装本机 macOS App；随后在本机与远端分别检查 Antigravity、DeepSeek Harness 列表、归档切换、菜单动作与 ChatGPT 风格界面。",
                "DSH 归档、恢复与永久删除前须退出目标机器正在运行的 DSH 进程；真实用户会话删除与日常视觉交互待用户验收。",
                "macOS 包没有 Developer ID 签名或 Apple 公证；Minisign 验证归档来源，不能替代系统代码签名。",
            ],
        }
        lines = [f"# AgentHub {version}", ""]
        for heading, paragraphs in sections.items():
            lines.extend([f"## {heading}", ""])
            lines.extend(f"- {paragraph}" for paragraph in paragraphs)
            lines.append("")
        return "\n".join(lines)

    if version == "1.3.11":
        sections = {
            "版本概览": ["新增 Antigravity 与 DSH 的本机 Bark 通知，完成按主回合结束判断；API 协议保持 2。"],
            "功能调整": [
                "本机 Probe 设置增加两种来源的总开关及完成、失败、需要回复、需要授权四类开关，沿用现有轮询间隔。",
                "Antigravity 读取运行中原生服务的明确执行终态与待交互状态；DSH 读取标准 v4 JSONL 和多帧 Zstandard 日志。",
            ],
            "问题修复": [
                "升级 Zstandard Rust 依赖，修复上游披露的解压与上下文复用内存安全问题。",
                "每次发送前复核状态，恢复执行后淘汰旧失败，已回答问题和已处理审批不再重试；异步问题超时但仍可回答时保留待回复状态。",
                "重复扫描和重启沿用持久化认领；排除子代理及 fork 继承记录，首次或重新启用不补发历史。",
            ],
            "兼容性与迁移": [
                "新通知开关默认关闭，仅本机支持；旧服务缺少能力字段时隐藏入口，Linux 服务不运行这两种来源。",
                "复用现有数据库、Bark 配置、加密队列和有限重试；读取失败、未知身份或不支持格式显示异常。",
            ],
            "支持平台与资产": [
                "macOS ARM64：DMG／校验文件、updater 压缩包／签名及仅映射 darwin-aarch64 的 latest.json。",
                "Linux x86_64：API 服务包、SHA-256 校验文件与分离签名；共八项正式资产。",
            ],
            "升级提示": [
                "先升级远端 API，再安装 macOS App；在本机 Probe 中开启需要的通知来源，Antigravity 原生服务须保持运行。",
                "分别触发四类提醒，处理交互或恢复执行后确认旧提醒停止重试；手机实收和真实交互待用户验收。",
                "macOS 包没有 Developer ID 签名或 Apple 公证；Minisign 验证归档来源，不能替代系统代码签名。",
            ],
        }
        lines = [f"# AgentHub {version}", ""]
        for heading, paragraphs in sections.items():
            lines.extend([f"## {heading}", ""])
            lines.extend(f"- {paragraph}" for paragraph in paragraphs)
            lines.append("")
        return "\n".join(lines)

    if version == "1.3.10":
        sections = {
            "版本概览": ["本版修复 Claude 回合提醒校验并新增 DSH 归档清理；API 协议保持 2，macOS App 仍是唯一界面。"],
            "功能调整": [
                "Claude 按主回合终态发送提醒，StopFailure 作为 API 错误候选处理；首次处理、首次发送和每次重试均重新核对身份、当前回合、后续执行活动与终态。独立后台任务不阻挡主回合提醒。",
                "新增 `cleanup.dshArchiveDryRun` 与 `cleanup.dshArchiveExecute`，在本机和当前选定远端机器的更新与维护页提供 DSH 归档清理。预览读取 workspace v2 归档名单，固定确认 id/archiveId，支持明文和 Zstandard 会话及可核验的 subagent 后代。",
            ],
            "问题修复": [
                "新回合、同回合恢复执行、成功或取消会终结对应旧失败提醒并清空待投递记录；普通元数据追加和进程仍打开不再误判为恢复执行。",
                "DSH 清理在执行前要求退出 DSH，检测恢复、归属变化、并发索引或会话修改后停止相关写入；缺失文件按已清理处理，权限、损坏索引和无法确认归属逐项返回。",
            ],
            "兼容性与迁移": [
                "旧服务缺少 `dsh_archive_cleanup` 能力时按不支持处理，API 协议仍为 2；请求不接受客户端路径。",
                "DSH_HOME 默认使用用户 `.dsh`，自定义根只开放 sessions 和 storages 子目录；共享附件、项目文件、配置及其他线程保留。自定义持久化后端和无法识别格式明确提示不支持。",
            ],
            "支持平台与资产": [
                "macOS ARM64：DMG／校验文件、updater 压缩包／签名及仅映射 `darwin-aarch64` 的 `latest.json`。",
                "Linux x86_64：API 服务包、SHA-256 校验文件与 `.sig` 分离签名；共八项正式发布资产。",
            ],
            "升级提示": [
                "先升级远端 API，再安装 macOS App；安装不会自动启用 Claude hooks。",
                "在两台机器的维护页预览 DSH 归档，退出 DSH 后清理专门测试线程，确认归档消失且其他线程保留；使用 Claude 验证恢复执行后不再收到旧失败提醒。真实线程删除与手机实收待用户验收。",
                "macOS 包没有 Developer ID 签名或 Apple 公证；Minisign 用于验证归档来源，不能替代系统代码签名。",
            ],
        }
        lines = [f"# AgentHub {version}", ""]
        for heading, paragraphs in sections.items():
            lines.extend([f"## {heading}", ""])
            lines.extend(f"- {paragraph}" for paragraph in paragraphs)
            lines.append("")
        return "\n".join(lines)

    if version == "1.3.9":
        sections = {
            "版本概览": ["本版修复三种 Provider 删除共享记录时的写入边界，并整理 Claude Code 自动内容；API 协议保持 2，macOS App 仍是唯一界面。"],
            "功能调整": [
                "Codex、Claude Code 和 Grok 的单条、批量、归档清理及隐藏清理继续使用固定确认目标和逐项结果。主文件缺失时继续处理可定位旁路；权限或数据库错误明确报告并继续其他目标。",
                "systemd 为 Grok、Claude 的默认或自定义数据根提供同目录原子替换权限，同时将凭据和配置保持只读；自定义 Codex home 也纳入写入路径。共享 JSON／JSONL 没有命中目标时不重写文件。",
                "Claude 完整命令包装显示为斜杠命令和原始参数。命令回显、任务通知、系统提醒、自动上下文和压缩摘要不再制造用户气泡、用户时间线标记或新回合；相邻活动在原位置默认折叠，并保留搜索、复制、分页定位与展开状态。",
                "Probe 中的 Claude Hook 使用一致的状态、间距和操作样式，显示安装状态与命令数量，移除常驻说明；安装、修复、移除仍保留确认。",
            ],
            "问题修复": [
                "修复 Grok active_sessions.json 与 Claude 共享登记文件所在目录只读导致的原子替换失败；未命中索引时不再触发无效写入。",
                "自动 Claude 记录不再清除待处理工具或被当成新的用户输入；混合请求保留原始换行、附件、代码示例和空格。",
            ],
            "兼容性与迁移": [
                "API 协议保持 2，不增加 RPC，也不改写原生 Provider 会话文件。安装器读取 Provider 环境变量及 Codex home 配置；缺少 Provider 根目录不会阻止服务启动。",
                "Grok 和 Claude 配置／凭据继续受只读保护，服务保留现有 systemd 隔离。没有匹配记录的共享索引保持原样。",
            ],
            "支持平台与资产": [
                "macOS ARM64：DMG／校验文件、updater 压缩包／签名及仅映射 `darwin-aarch64` 的 `latest.json`。",
                "Linux x86_64：API 服务包、SHA-256 校验文件与 `.sig` 分离签名；共八项正式发布资产。",
            ],
            "升级提示": [
                "先升级远端 API，再安装 macOS App；安装不会自动启用 Claude hooks。保留现有配置、凭据与 Provider 会话。",
                "使用专用合成会话验证三平台删除；打开 Claude 会话检查真实输入、折叠的系统活动和搜索分页，再到 Probe 检查 Hook 样式。真实会话删除与日常视觉体验待用户验收。",
                "macOS 包没有 Developer ID 签名或 Apple 公证；Minisign 用于验证归档来源，不能替代系统代码签名。",
            ],
        }
        lines = [f"# AgentHub {version}", ""]
        for heading, paragraphs in sections.items():
            lines.extend([f"## {heading}", ""])
            lines.extend(f"- {paragraph}" for paragraph in paragraphs)
            lines.append("")
        return "\n".join(lines)

    if version == "1.3.8":
        sections = {
            "版本概览": ["本版统一三个 Provider 的直接删除和空会话读取；API 协议保持 2，macOS App 仍是唯一界面。"],
            "功能调整": [
                "Codex、Claude Code 和 Grok 的单条、批量、归档清理及隐藏清理都按确认目标直接执行。删除不依赖活动进程、writer 锁、终态、版本、身份冲突、指纹、外部引用或内容完整性，文件不存在视为完成。",
                "Codex 按原生父子关系去重遍历目标后代，覆盖普通／归档 rollout、压缩副本、writer lock、shell snapshots、visualizations、状态和分页历史数据库及名称索引；Claude 补齐 file-history、tasks、session-env、debug、todos、项目索引、共享历史和会话登记；Grok 删除各工作区目标目录并清理 active_sessions 登记。",
                "删除不设批量数量、文件大小、目录深度和文件数量上限，权限、文件系统或数据库错误逐项返回并继续处理其他目标；目录符号链接只删除链接本身，共享文件只移除对应记录。",
            ],
            "问题修复": [
                "Claude 命名记录、空文件和仅元数据会话在缺少工作目录或版本时正常显示“暂无对话”，读取问题不再禁用删除。",
                "Grok 保留 updates.jsonl 读取；文件缺失时使用 chat_history.jsonl 映射用户、助手、工具调用和结果，复用分页、搜索、追加更新和时间线。",
                "系统提示、system_reminder 和加密推理不作为普通对话展示；损坏摘要、活动登记和占用锁不再阻止已定位目标删除。",
            ],
            "兼容性与迁移": [
                "删除请求中的 fingerprint 和 expectedCount 均可省略并不参与判断；归档清理沿用 candidates 传递确认范围，API 协议仍为 2。",
                "改名、归档和恢复继续保留各自原有规则；不会调用 Codex 原生删除 helper，也不会终止 Provider 进程或启动后台重复删除任务。",
            ],
            "支持平台与资产": [
                "macOS ARM64：DMG／校验文件、updater 压缩包／签名及仅映射 darwin-aarch64 的 latest.json。",
                "Linux x86_64：API 服务包、SHA-256 校验文件与 .sig 分离签名；共八项正式发布资产。",
            ],
            "升级提示": [
                "先升级远端 API，再安装 macOS App；安装不会自动启用 Claude hooks。保留配置、凭据、会话及共享浏览器缓存。",
                "安装后打开两个指定真实样例，应显示“暂无对话”且删除入口可用；确认删除后由用户检查列表、主文件及对应旁路是否消失。",
                "本机不安装或启动 Chrome、Chrome for Testing 或 Playwright；浏览器回归和 macOS 有头检查由 GitHub Actions 执行。",
            ],
        }
        lines = [f"# AgentHub {version}", ""]
        for heading, paragraphs in sections.items():
            lines.extend([f"## {heading}", ""])
            lines.extend(f"- {paragraph}" for paragraph in paragraphs)
            lines.append("")
        return "\n".join(lines)

    if version == "1.3.7":
        sections = {
            "版本概览": ["本版修复 Claude 身份兼容、Codex 删除旁路收尾和 Playwright 浏览器来源约束；API 协议保持 2，macOS App 仍是唯一界面。"],
            "功能调整": [
                "Claude 每条记录优先使用非空 `sessionId` 和 `version`；同记录的 snake 字段可作为回退，双值不同只保留兼容警告，不再误报身份冲突。跨记录身份冲突、父子关联异常和活动进程保护继续阻断管理操作。",
                "Codex 删除预览纳入 `thread-writer-locks/<id>.lock`、`shell_snapshots/<id>.*` 和 `visualizations/**/<id>/`，分别展示路径、大小和预览指纹；官方删除成功并完成主数据复检后，按预览指纹有界清理旁路。",
                "Playwright 配置只使用 `browserName`，全局启动探针核对真实可执行文件位于 Playwright 缓存，拒绝品牌渠道、本机 `executablePath`、外部连接和符号链接越界。浏览器回归与 macOS 有头性能检查交给 GitHub Actions。",
            ],
            "问题修复": [
                "明文 helper 不可用时继续保留受限离线删除；官方请求失败或超时不回退本地 rollout，压缩历史仍禁止离线回退。",
                "空结果、非终态结果和工具过程使用独立 fixture，保留最终回复被后续非终态结果撤销完成标记时 `assistant_phase=None` 的断言。",
                "无关损坏 `.zst` 继续只产生警告；旁路残留、路径变化、权限错误或无法完整核验均返回失败和 residuals，不提前报告 succeeded。",
            ],
            "兼容性与迁移": [
                "不修改 Claude、Codex 原生文件格式；身份、版本和旁路指纹只用于本机管理核验，API 协议仍为 2。",
                "浏览器验收不依赖、不启动本机 Google Chrome；Playwright 内置发行物可能显示 Google Chrome for Testing，按实际缓存路径确认来源。",
            ],
            "支持平台与资产": [
                "macOS ARM64：DMG／校验文件、updater 压缩包／签名及仅映射 `darwin-aarch64` 的 `latest.json`。",
                "Linux x86_64：API 服务包、SHA-256 校验文件与 `.sig` 分离签名；共八项正式发布资产。",
            ],
            "升级提示": [
                "先升级远端 API，再安装 macOS App；安装不会自动启用 Claude hooks。保留现有配置、凭据、会话和回退材料。",
                "正式删除前关闭目标线程的写入者，先检查三类旁路预览；本版不自动终止 codex 进程。",
                "功能、视觉、原生选择器和真实会话删除仍待用户验收；浏览器回归和 macOS 有头性能由对应 GitHub Actions 运行。",
            ],
        }
        lines = [f"# AgentHub {version}", ""]
        for heading, paragraphs in sections.items():
            lines.extend([f"## {heading}", ""])
            lines.extend(f"- {paragraph}" for paragraph in paragraphs)
            lines.append("")
        return "\n".join(lines)
    elif version == "1.3.6":
        sections = {
            "版本概览": ["本版完成 Codex 压缩历史安全删除、Claude 会话管理修复及 Claude 单回合过程折叠；API 协议保持 2，macOS App 仍是唯一界面。"],
            "功能调整": [
                "Codex 识别普通、归档、明文和 `.jsonl.zst` rollout；压缩内容使用有界流式 zstd 解码，明文与压缩兄弟文件合并为一个逻辑 rollout，列表不会因明文消失而隐藏仍存在的压缩历史。",
                "Claude 详情在同一原生回合内将思考、明确的过程说明和工具活动按原顺序收进外层折叠，最终答复单独显示；内部工具调用与结果继续使用原有工具折叠。",
                "搜索命中过程或工具时自动展开外层和必要内层；刷新、向前分页、重新挂载及机器切换按机器、会话和回合身份保留折叠状态，应用重启后恢复默认折叠。",
            ],
            "问题修复": [
                "Codex 删除先确定目标候选，再分别完成目标文件和全局引用核验；覆盖回退 rollout 引用、明文与压缩共存、目标损坏或截断、指纹变化、数据库／索引／分页残留和不安全离线回退。",
                "Claude 区分会话进程、daemon 和 PTY 宿主，子进程、预热进程、打开目标文件及已关联专属子代理继续参与占用判断；增加本机只读归属诊断和中文原因代码。",
                "Claude 改名继续只追加一条原生 `custom-title`，增加尾部读回验证；删除预览覆盖已核验主 transcript、同 ID 专属目录、未知专属条目及空或纯元数据子代理文件，隔离后变化和残留会明确报告。",
            ],
            "兼容性与迁移": [
                "压缩 rollout 只读，不解压落盘、不主动压缩、不修改原生存储；含压缩目标或全局引用扫描跳过项时禁止旧格式离线回退。官方请求提交后的错误或超时不自动重试。",
                "`assistantPhase` 为可选显示字段。旧服务缺少字段时保守展示，缺少可靠终态证据的文字保持可见；通知终态、去重身份、用户指令时间线和内容清理规则保持原有语义。",
                "诊断入口仅限本机，不进入 Linux RPC 或远端转发；过程折叠状态只保存在内存，不新增磁盘状态文件。",
            ],
            "支持平台与资产": [
                "macOS ARM64：DMG／校验文件、updater 压缩包／签名及仅映射 `darwin-aarch64` 的 `latest.json`。",
                "Linux x86_64：API 服务包、SHA-256 校验文件与 `.sig` 分离签名；共八项正式发布资产。",
            ],
            "升级提示": [
                "先升级远端 API，再安装 macOS App；安装不会自动启用 Claude hooks。保留现有配置、凭据、会话和回退材料。",
                "用专门测试线程验收普通与压缩 Codex 整树删除；在 Claude 会话中验证改名、专属文件删除、过程折叠、最终答复、搜索展开和分页状态。",
                "功能、视觉、原生选择器和真实会话删除仍待用户验收；本轮测试使用合成 fixture，不发送真实 Bark 通知。",
            ],
        }
        lines = [f"# AgentHub {version}", ""]
        for heading, paragraphs in sections.items():
            lines.extend([f"## {heading}", ""])
            lines.extend(f"- {paragraph}" for paragraph in paragraphs)
            lines.append("")
        return "\n".join(lines)

    if version == "1.3.5":
        sections = {
            "版本概览": ["本版修复线程多选、菜单、Codex 删除、Claude 会话管理与 Bark 漏推；macOS App 仍是唯一界面。"],
            "功能调整": [
                "Codex、Claude Code、Grok 共用 ⌘ 点击多选、Shift 连续区间追加和 100 项上限；切换机器或筛选条件清除选择，轮询保留仍存在的选项。",
                "Claude 菜单提供改名、复制线程 ID、路径、恢复命令、本机 Finder 定位和删除；各项能力独立判断。Codex 列表移除重复的复制答复入口，头部按钮保留。",
                "本机设置新增 Claude hooks 的状态、安装、修复与移除；须明确确认，保留已有用户与插件 hooks，远端继续使用轮询。",
            ],
            "问题修复": [
                "Codex 支持空线程和无活跃写入者的待回复线程删除，预览完整子线程树及文件；优先使用原生删除接口，复检文件、索引和分页历史，部分失败或结果不明不会报告成功或自动重试。",
                "Claude 使用原生会话登记和进程身份判断占用，修复同工作目录历史会话被误保护；兼容警告不再阻止安全管理，专属目录与主文件一起核验、隔离和删除。",
                "Bark 标题统一为 Provider、状态和线程标题，正文去除 Markdown、代码及内部信息，最多四行、300 个 Unicode 字符，每个新事件生成一条摘要。",
                "Claude hooks 与本机轮询共享每回合每类一次的认领，保留有效旧回合完成事件以补发；完成后不再单独提醒空闲。Codex 多个独立问题仍分别通知。",
                "更新 TLS 证书验证与根证书依赖，修复上游披露的 emSign 中间证书信任问题。",
            ],
            "兼容性与迁移": [
                "API 协议保持 2；整树删除需服务声明能力并由客户端显式确认，旧服务提示升级，不静默级联或降级。",
                "压缩 rollout、未知存储、活动写入者及无法核验的引用仍阻止删除；归档、恢复和隐藏清理沿用原有条件。Claude 不增加归档，不沿续接关系删除其他主会话。",
                "保留 Bark 默认关闭、Provider 开关、启用基线、凭据及加密队列；旧在途消息保留原内容和已确认分段，升级不回放历史或重发成功项。",
            ],
            "支持平台与资产": [
                "macOS ARM64：DMG／校验文件、updater 压缩包／签名及仅映射 darwin-aarch64 的 latest.json。",
                "Linux x86_64：API 服务包、SHA-256 校验文件与 .sig 分离签名；共八项正式发布资产。",
            ],
            "升级提示": [
                "先升级远端 API，再安装 macOS App；安装 App 不会自动启用 Claude hooks。保留现有配置、凭据、会话和回退材料。",
                "用专门测试线程验收多选、菜单和删除；在本机设置页确认启用 Claude hooks 后，验证完成、问题、授权和失败提醒，以及 Codex 同回合多问题分别通知。",
                "Bark 已受理表示服务端确认，设备实收仍需核对；网络结果未知时有限重试仍可能重复。macOS 包未使用 Developer ID 签名或 Apple 公证。",
            ],
        }
        lines = [f"# AgentHub {version}", ""]
        for heading, paragraphs in sections.items():
            lines.extend([f"## {heading}", ""])
            lines.extend(f"- {paragraph}" for paragraph in paragraphs)
            lines.append("")
        return "\n".join(lines)

    if version == "1.3.4":
        sections = {
            "版本概览": ["本版修复本机 Codex 协作的兼容提示、附件身份误判与剪贴板输入，对话和工具仍由原生桌面执行。"],
            "功能调整": [
                "从 Finder 复制文件或粘贴完整路径可自动添加附件，支持多文件、中文、空格与成对引号；整段无法识别为有效文件时保留文字。",
                "补齐 JPEG 和 WebKit 缺少图片 MIME 时的原生粘贴入口；Finder 文件优先于预览缩略图。",
            ],
            "问题修复": [
                "移除按 build 号显示的常驻警告，连接与发送按实际协议和 owner 校验；补齐原生超时分类。",
                "新选择的小文件仅 ctime 变化时以内容摘要再次核验，避免扩展属性变化被误判；内容改写、文件替换、权限和符号链接变化仍拒绝。",
                "修复恢复焦点与关闭选择器后的附件绑定竞争，去除重复导入并丢弃旧线程结果；失败保留草稿，重新选择或移除附件后清除对应操作错误。",
                "修复系统截图复制后粘贴没有反应、无法显示附件标签的问题。",
            ],
            "兼容性与迁移": [
                "API 协议保持 2，复用现有本机附件命令并增加可选粘贴结果；客户端不能提交原生路径或图片字节，Linux 与远程不开放发送。",
                "旧引用和大文件仍严格核验；加密身份新增可选摘要，不迁移或改写旧队列。Keychain、私有附件与结果不明不重发保持。",
                "现用方法已对照桌面 build 20586 与 20770；build 号不作为发送限制，未来破坏性协议升级仍可能需要适配。普通文件保持已有原生兼容编码。",
            ],
            "支持平台与资产": [
                "macOS ARM64：DMG／校验文件、updater 压缩包／签名及仅映射 darwin-aarch64 的 latest.json。",
                "Linux x86_64：API 服务包、SHA-256 校验文件与 .sig 分离签名；共八项正式发布资产。",
            ],
            "升级提示": [
                "正式升级先远程 API、后 macOS App；保留配置、凭据、会话、暂停队列与私有附件，不回滚业务数据。",
                "在原生桌面打开同一主线程并连接，分别通过加号、Finder 复制、路径文本和截图粘贴发送附件；检查普通文本编辑及窗口切换后立即粘贴。",
                "macOS 包没有 Developer ID 签名或 Apple 公证；Minisign 验证正式归档来源，不能替代系统代码签名。",
            ],
        }
        lines = [f"# AgentHub {version}", ""]
        for heading, paragraphs in sections.items():
            lines.extend([f"## {heading}", ""])
            lines.extend(f"- {paragraph}" for paragraph in paragraphs)
            lines.append("")
        return "\n".join(lines)

    if version == "1.3.3":
        sections = {
            "版本概览": ["本版修复本机 Codex 桌面协作的目录、设置、提交和连接恢复问题；对话与工具仍由原生桌面执行。"],
            "功能调整": [
                "整理添加菜单、引用列表、模型摘要和发送按钮；同步原生 Goal 的目标与状态，创建、修改、暂停和恢复仍在 Codex 中完成。",
                "连接失败时可复制安全诊断并看到复制结果；诊断仅含固定错误分类、方法与版本信息，不包含正文、路径或凭据。",
                "打开或新建线程后等待原生 owner 并自动连接；普通浏览不会启动协作，桌面 owner 改变后须点击连接确认写入。",
            ],
            "问题修复": [
                "应用目录使用无线程请求，同一 helper 完成分页；模型、模式、技能、插件和应用独立读取与重试。",
                "思考强度以原生模型目录为准，支持目录中的 max 等值；模式失败保留模型结果，设置失败显示原生真实选择。",
                "按方法设置提交等待期限，等待时继续处理进度；失步和断线有界恢复，丢弃旧连接动作与附件结果。",
                "区分附件数量、大小、格式、身份和容量错误；成功或重连后清除过期提示，结果不明仍由队列保留。",
                "修正 $ / @ 引用的原生链接编码和附件上下文，消除窗口失焦时后台附件更新产生的错误提示。",
                "跟随最新消息时减少线程切换的无效位置测量；保留向上阅读、搜索与分页的滚动锚点。",
            ],
            "兼容性与迁移": [
                "API 协议保持 2，新增诊断和可选连接字段仅用于本机 macOS；Linux、远程、归档及子线程不开放发送。",
                "保留原生引用、空 text_elements、旧加密队列正文、完整审批绑定及会话审批门控；不确定的 start/steer 结果不自动重发。",
                "协议方法以 build 20586 为基线，已核对 build 20770 的现用方法一致并收到用户候选流程通过反馈；其他版本仍按实际方法与状态核验，不猜测新方法或决策。普通文件继续使用既有兼容编码。",
                "开发和 CI 统一使用固定 Rust 1.99.0，取消旧 Rust 1.88 兼容检查；运行安装包无需自行安装 Rust。",
            ],
            "支持平台与资产": [
                "macOS ARM64：DMG／校验文件、updater 压缩包／签名及仅映射 darwin-aarch64 的 latest.json。",
                "Linux x86_64：API 服务包、SHA-256 校验文件与 .sig 分离签名；共八项正式发布资产。",
            ],
            "升级提示": [
                "正式升级先远程 API、后 macOS App；保留配置、凭据、会话、暂停队列与私有附件，不回滚业务数据。",
                "在原生桌面打开同一主线程，检查目录、模型与 effort、引用和附件、进度、两侧审批以及重启后的 owner 确认。未覆盖的桌面版本和使用场景不宣称已验证。",
                "macOS 包没有 Developer ID 签名或 Apple 公证；Minisign 用于验证正式归档来源，不能替代系统代码签名。",
            ],
        }
        lines = [f"# AgentHub {version}", ""]
        for heading, paragraphs in sections.items():
            lines.extend([f"## {heading}", ""])
            lines.extend(f"- {paragraph}" for paragraph in paragraphs)
            lines.append("")
        return "\n".join(lines)

    if version == "1.3.2":
        sections = {
            "版本概览": ["本版完善本机 Codex 协作的附件输入、审批阅读和状态呈现；对话执行、进度、审批与问答仍由原生桌面持有。"],
            "功能调整": [
                "文件可拖入协作输入区，图片可直接粘贴；保留原生文件选择器及 $ / @ 技能、应用和插件引用。",
                "审批命令使用等宽文本，文件 diff 按文件折叠并在展开后加载；协作区显示执行、待处理请求和队列状态。",
            ],
            "问题修复": [
                "审批投影有界处理大内容，保留完整请求与文件变化身份；内容截断或无法完整审阅时仍需在 Codex 处理，不放宽批准条件。",
                "附件只从原生拖放或明确粘贴进入，界面不接收文件路径或图片字节；切换、隐藏、发送和选择文件时隔离迟到结果。",
                "协议不兼容、owner 不可用与状态失步在连接处显示固定提示，注明已核验 build 20586，保留历史阅读与草稿。",
            ],
            "兼容性与迁移": [
                "API 协议保持 2，新增附件命令仅供本机 macOS；只读审批字段保持可选，Linux、远程、归档与子线程不开放发送。",
                "保留旧加密队列正文、会话审批门控及结果不明不重发。粘贴图片保存在私有文件中，归属索引加密；交给发送队列后保留，避免清理破坏原生历史。",
                "普通文件继续使用已有兼容编码；本轮未取得原生文件完整输入快照，不扩展未核实的审批决策，也不增加缺少复现证据的 IPC 通知。",
            ],
            "支持平台与资产": [
                "macOS ARM64：DMG／校验文件、updater 压缩包／签名及仅映射 darwin-aarch64 的 latest.json。",
                "Linux x86_64：API 服务包、SHA-256 校验文件与 .sig 分离签名；共八项发布资产。",
            ],
            "升级提示": [
                "先升级远程 API，再安装 macOS App；保留配置、凭据、会话与私有附件。",
                "在原生桌面打开同一线程并连接，使用 $ / @、拖放或粘贴发送，检查桌面进度、审批 diff 及任一侧处理后的状态同步。真实操作、视觉与日常体验待用户验收。",
                "macOS 包没有 Developer ID 签名或 Apple 公证；Minisign 用于验证归档来源，不能替代系统代码签名。",
            ],
        }
        lines = [f"# AgentHub {version}", ""]
        for heading, paragraphs in sections.items():
            lines.extend([f"## {heading}", ""])
            lines.extend(f"- {paragraph}" for paragraph in paragraphs)
            lines.append("")
        return "\n".join(lines)

    if version == "1.3.1":
        sections = {
            "版本概览": ["本版完善本机 Codex 桌面协作的引用输入、目录读取、进度同步和审批体验；对话仍由原生桌面执行。"],
            "功能调整": [
                "输入 $ 搜索技能、@ 搜索应用与插件；上下键选择、Enter/Tab 插入、Escape 关闭，所选引用显示可读占位文字与可移除标签。",
                "命令和文件审批仅在当前请求声明支持时显示“允许本次会话”；权限请求仍限本回合，不增加永久授权。",
            ],
            "问题修复": [
                "目录读取不再长时间占用缓存锁，同源并发查询合并；超时可重试，应用或插件单侧失败保留另一侧结果。",
                "校验引用的身份和 UTF-8 区间，修复插件显示名与原生名称不一致的问题，保留中文和 emoji 前后的正确占位位置。",
                "协作事件合并刷新当前线程，慢读取不被反复取消；隐藏、切换线程与断开后丢弃旧响应，并保留回合结束后的最终刷新。",
                "协议不兼容、线程未由桌面持有、状态失步和超时使用清晰的固定提示，失败时仍可阅读历史。",
            ],
            "兼容性与迁移": [
                "API 协议保持 2，引用区间与会话审批使用可选字段；协作仍仅供本机 macOS 主线程，远程和其他 Provider 的边界保持不变。",
                "保留旧加密队列正文格式与幂等身份，结果不明不自动重发；升级保留配置、凭据、通知状态及原生会话。",
                "协议依据桌面 build 20586 源码与合成测试核对。普通文件保留现有兼容编码；未核实的原生文件编码、文件 cancel 和其他桌面版本不宣称已支持。",
            ],
            "支持平台与资产": [
                "macOS ARM64：DMG／校验文件、updater 压缩包／签名及仅映射 darwin-aarch64 的 latest.json。",
                "Linux x86_64：API 服务包、SHA-256 校验文件与 .sig 分离签名；共八项发布资产。",
            ],
            "升级提示": [
                "先升级远程 API，再安装 macOS App。安装后在原生桌面打开同一主线程，连接后用 $ / @ 发送，检查桌面进度和两侧审批同步。",
                "真实引用展示、两侧操作、视觉与滚动体验待用户验收；本轮合成测试不替代实机结论。",
                "macOS 包没有 Developer ID 签名或 Apple 公证；Minisign 用于验证归档来源，不能替代系统代码签名。",
            ],
        }
        lines = [f"# AgentHub {version}", ""]
        for heading, paragraphs in sections.items():
            lines.extend([f"## {heading}", ""])
            lines.extend(f"- {paragraph}" for paragraph in paragraphs)
            lines.append("")
        return "\n".join(lines)

    if version == "1.3.0":
        sections = {
            "版本概览": ["本版接入本机 Codex 桌面协作，在 AgentHub 中继续对话，由原生桌面执行。现有三个 Provider 的阅读、通知与管理功能继续保留。"],
            "功能调整": [
                "本机普通主线程可新建、续聊、排队发送、补充当前回合或停止；模型、推理强度和模式沿用原生桌面可用选项。",
                "支持选择文件、图片及可用的技能、应用和插件引用；已支持的问题和审批可在面板处理，无法安全表达的请求回到原生桌面处理。",
                "AgentHub 独立保存待发送队列，在线程空闲且原生桌面持有该线程时逐项交付；不覆盖原生桌面的队列。",
            ],
            "问题修复": [
                "兼容当前原生 App 内置 CLI 的目录布局，处理连接取消、迟到响应及机器切换，避免旧线程状态覆盖当前页面。",
                "发送前重新检查线程状态、引用文件身份和请求内容；结果不确定的发送不自动重试，防止重复提交。",
                "断开或重启后待发送项目保留为暂停状态，恢复前需确认；停止当前回合也会暂停尚未交付的项目。",
            ],
            "兼容性与迁移": [
                "API 协议保持 2，新增协作命令仅供本机 macOS App 使用；远程、归档、子线程及 Claude Code、Grok 不提供协作输入。",
                "队列存储在 AgentHub 自有数据库并使用独立钥匙串密钥加密；升级保留既有连接、凭据、通知状态与原生会话。",
                "原生桌面协议不兼容、线程未由桌面持有或身份无法核验时停止协作，不回退为后台 CLI 执行。",
            ],
            "支持平台与资产": [
                "macOS ARM64：DMG／校验文件、updater 压缩包／签名及仅映射 darwin-aarch64 的 latest.json。",
                "Linux x86_64：API 服务包、SHA-256 校验文件与 .sig 分离签名；共八项发布资产。",
            ],
            "升级提示": [
                "先升级远程 API，再安装 macOS App。使用协作前打开兼容的原生 Codex 桌面并打开目标线程；新建空线程在首次发送前没有聊天内容。",
                "首次使用可能出现系统钥匙串授权。安装后请用专用线程验证发送、排队、补充、停止、引用及审批；视觉、滚动和日常使用待用户验收。",
                "macOS 包没有 Developer ID 签名或 Apple 公证；Minisign 用于验证归档来源，不能替代系统代码签名。",
            ],
        }
        lines = [f"# AgentHub {version}", ""]
        for heading, paragraphs in sections.items():
            lines.extend([f"## {heading}", ""])
            lines.extend(f"- {paragraph}" for paragraph in paragraphs)
            lines.append("")
        return "\n".join(lines)

    if version == "1.2.23":
        sections = {
            "版本概览": ["本版加固现有监听、更新链与工程检查，保留既有阅读、通知及管理流程。"],
            "功能调整": [
                "不新增用户功能、RPC、设置或界面入口；Codex、Claude Code、Grok、Probe 和现有管理保护保持原有边界。",
                "开发与 CI 固定 Rust 1.99.0，最低 Rust 声明为 1.88；前端依赖采用锁文件中的精确版本。",
            ],
            "问题修复": [
                "拒绝非回环 API 监听，在数据库及后台任务启动前返回固定错误。更新与 Probe 任务使用固定程序和字面参数，不执行自定义 shell 字符串。",
                "Bark 保存与投递共用地址校验，拒绝危险 IP、数字 IP 别名和 URL 凭据；保留 HTTP 回环，非法配置不会改用默认服务发送。",
                "Linux 更新先验证 SHA-256 和 Minisign 分离签名，再解包并核对版本、平台及布局；解压不继承特殊位或 world-writable 权限。",
                "CI 增加超时、--locked 和工具缓存；高危、严重及无法判级的活动漏洞或扫描失败阻断，低中危与维护类告警继续报告。定向修复相关 Rust 依赖。",
            ],
            "兼容性与迁移": [
                "API 协议保持 2，RPC、桌面命令、DTO 与 tombstone 不变；保留 Keychain、认证比较、禁止重定向和 systemd 隔离。",
                "配置只允许回环监听；更新任务接受现行及完整匹配的历史默认命令，其他自定义 shell 配置会明确拒绝。",
                "不改写原生会话。安装普通失败恢复本轮替换文件，不回滚业务数据库；显式本地 --archive/--binary 运维方式保留。",
            ],
            "支持平台与资产": [
                "macOS ARM64：DMG／校验文件、updater 压缩包／签名及仅映射 darwin-aarch64 的 latest.json。",
                "Linux x86_64：API 服务包、SHA-256 校验文件与 .sig 分离签名；共八项发布资产。",
            ],
            "升级提示": [
                "部署客户端与服务器须预装 Minisign。首次从 1.2.22 或更早版本升级，先由可信客户端验证正式包，再安装新的验签器和公钥；不降级为仅 SHA-256。",
                "先升级远程 API，再安装 macOS App；保留现有配置、连接、凭据、通知状态和会话。正常阅读、通知、更新反馈与日常使用待用户验收。",
                "macOS 包没有 Developer ID 签名或 Apple 公证；Minisign 用于验证归档来源，不能替代系统代码签名。",
            ],
        }
        lines = [f"# AgentHub {version}", ""]
        for heading, paragraphs in sections.items():
            lines.extend([f"## {heading}", ""])
            lines.extend(f"- {paragraph}" for paragraph in paragraphs)
            lines.append("")
        return "\n".join(lines)

    if version == "1.2.22":
        sections = {
            "版本概览": ["本版集中维护与加固现有能力，改善工程可维护性与升级可靠性。"],
            "功能调整": [
                "远程机器与服务使用中性名称；保留 Codex、Claude Code、Grok、Probe、设置及现有阅读和管理入口。",
                "整理 Provider 读取、CLI、Hook 和 Probe 展示模块，集中资源预算并补充兼容说明。",
            ],
            "问题修复": [
                "退役数据迁移使用版本守门及跨进程锁，重复开库不再重复扫描退役记录；中断后保留安全接续。",
                "管理操作在隔离后重新核验文件身份与指纹，补充索引、数据库提交和进程终止故障覆盖。",
                "部署校验传输内容，安装拒绝危险归档并原子替换程序；普通安装失败恢复本轮替换文件。",
                "补强真实 DTO、适配层、版本与隐私检查，固定 CI Action 引用并输出非阻塞依赖审计报告。",
            ],
            "兼容性与迁移": [
                "API 协议保持 2，现有 DTO 语义不变；不新增 RPC、数据库表、设置项或 UI 入口。",
                "保留协议 tombstone、旧服务回退、连接、Bark、通知状态和原生会话；仅迁移 AgentHub 自有数据。",
                "缓存预算不等于进程 RSS 硬上界；安装回滚不回滚业务数据库，也不新增崩溃自动恢复。",
            ],
            "支持平台与资产": [
                "macOS ARM64：DMG／校验文件、updater 压缩包／签名及仅映射 darwin-aarch64 的 latest.json。",
                "Linux x86_64：API 服务包与校验文件；共七项发布资产。",
            ],
            "升级提示": [
                "先升级远程 API，再安装本机 App；安装前核对 SHA-256 与 updater 签名。保留现有连接、凭据及会话。",
                "安装后检查本机与远程的阅读、分页、搜索、附件、Probe 与管理预览。真实功能、视觉及日常体感待用户验收。",
                "macOS 包没有 Developer ID 签名或 Apple 公证；Minisign 用于验证 updater 来源，不能替代系统代码签名。",
            ],
        }
        lines = [f"# AgentHub {version}", ""]
        for heading, paragraphs in sections.items():
            lines.extend([f"## {heading}", ""])
            lines.extend(f"- {paragraph}" for paragraph in paragraphs)
            lines.append("")
        return "\n".join(lines)

    if version == "1.2.21":
        sections = {
            "版本概览": ["本版整理自动注入上下文与附件说明，并修复时间线过宽、较早记录无法定位的问题。"],
            "功能调整": [
                "每条用户消息最多一行“上下文”，默认收起，展开时按原顺序显示；附件和真实请求正常保留，纯上下文不产生空气泡或时间线刻度。",
                "主线程、子线程和三个 Provider 使用完整用户消息索引，可直接定位尚未加载的较早记录，再向前后分页或返回最新内容。",
                "左侧刻度与悬停预览缩窄，移除重复提示；大量记录按需挂载，支持滚动和键盘访问。",
            ],
            "问题修复": [
                "同时识别新旧附件请求标题，保留正文换行并按原生身份去重；临时附件失效不影响请求显示。",
                "搜索排除环境及自动包装，保留真实正文、问题回答、附件名和 AGENTS.md 指令定位；代码、引用及不完整结构保持原文。",
                "修复 Grok 大历史只读取末尾 64MiB 导致早期记录缺失的问题，冷读使用完整流式解析并复用增量缓存。",
            ],
            "兼容性与迁移": [
                "API 协议保持 2，详情新增字段均为可选；新客户端连接旧服务时整理原始包装并回退逐页加载，不新增 RPC、数据库表或客户端路径权限。",
                "保留原生会话、管理保护、连接、附件、Plan、通知和 100M／300M 颜色规则；同步已批准的开发工具链与锁文件。",
            ],
            "支持平台与资产": [
                "macOS ARM64：DMG／校验文件、updater 压缩包／签名及仅映射 darwin-aarch64 的 latest.json。",
                "Linux x86_64：API 服务包与校验文件；共七项发布资产。",
            ],
            "升级提示": [
                "先升级远程 API，再安装本机 App。安装前核对校验和与 updater 签名。",
                "安装后检查上下文展开、AGENTS.md 搜索、最早时间线记录、双向分页与返回最新；真实视觉、滚动体验和日常操作待用户验收。",
                "macOS 包没有 Developer ID 签名或 Apple 公证；Minisign 用于验证 updater 来源，不能替代系统代码签名。",
            ],
        }
        lines = [f"# AgentHub {version}", ""]
        for heading, paragraphs in sections.items():
            lines.extend([f"## {heading}", ""])
            lines.extend(f"- {paragraph}" for paragraph in paragraphs)
            lines.append("")
        return "\n".join(lines)

    if version == "1.2.20":
        sections = {
            "版本概览": ["本版修复 Grok 闲置线程误保护与隐藏线程数量较多时的扫描卡顿。"],
            "功能调整": [
                "隐藏线程自动分批扫描，显示准备、扫描、完成或停止状态；候选与逐项结果每页最多显示 100 项。",
                "扫描可随时停止，保留已显示结果；全部扫描完成后才能确认清理，重新扫描会建立新快照。",
            ],
            "问题修复": [
                "Grok 在原生登记与进程身份确认无占用、记录完整有效时，允许历史回合未闭合的线程进入删除预览；仍打开、状态无法核验或记录损坏的线程继续受保护。",
                "隐藏扫描共用数据库元数据、索引和文件引用，流式验证历史；扫描及删除预览移入有界后台执行，减少重复读取与界面阻塞。",
                "Markdown 复用完整转换结果，减少切换线程和重复展开时的处理，保留链接安全与文件路径行为。",
            ],
            "兼容性与迁移": [
                "API 协议保持 2，不新增数据库表；旧客户端仍可单次扫描，新客户端连接旧服务时沿用旧行为。",
                "删除继续绑定预览候选与指纹，并在执行前复核身份、占用和文件变化；保留现有会话、连接、Bark 与通知状态。",
            ],
            "支持平台与资产": [
                "macOS ARM64：DMG／校验文件、updater 压缩包／签名及仅映射 darwin-aarch64 的 latest.json。",
                "Linux x86_64：API 服务包与校验文件；共七项发布资产。",
            ],
            "升级提示": [
                "先升级远程 API，再安装本机 App。安装前核对校验和与 updater 签名。",
                "安装后可在 Grok 打开原先误保护线程的删除预览，并在“更新与维护”检查扫描进度、翻页、停止和重新扫描。",
                "macOS 包没有 Developer ID 签名或 Apple 公证；Minisign 用于验证 updater 来源，不能替代系统代码签名。",
            ],
        }
        lines = [f"# AgentHub {version}", ""]
        for heading, paragraphs in sections.items():
            lines.extend([f"## {heading}", ""])
            lines.extend(f"- {paragraph}" for paragraph in paragraphs)
            lines.append("")
        return "\n".join(lines)

    if version == "1.2.19":
        sections = {
            "版本概览": ["本版补齐已回答问题的历史展示，延续长线程性能优化和 100M／300M 线程颜色规则。"],
            "功能调整": ["已回答问题同时显示原提问、选项及说明和已有回答，全部保持只读。"],
            "问题修复": ["修复搜索命中历史提问后虽然定位成功，卡片却只显示回答、看不到命中文字的问题。"],
            "兼容性与迁移": [
                "API 协议保持 2；复用已有问题记录与稳定位置，不改变问题状态、原生会话或通知去重。",
                "保留增量读取、历史分页、附件、Plan、子智能体、管理保护及已有连接配置。",
            ],
            "支持平台与资产": [
                "macOS ARM64：DMG／校验文件、updater 压缩包／签名及仅映射 darwin-aarch64 的 latest.json。",
                "Linux x86_64：API 服务包与校验文件；共七项发布资产。",
            ],
            "升级提示": [
                "先升级远程 API，再安装本机 App。安装前核对校验和与 updater 签名。",
                "macOS 包没有 Developer ID 签名或 Apple 公证；Minisign 用于验证 updater 来源，不能替代系统代码签名。",
            ],
        }
        lines = [f"# AgentHub {version}", ""]
        for heading, paragraphs in sections.items():
            lines.extend([f"## {heading}", ""])
            lines.extend(f"- {paragraph}" for paragraph in paragraphs)
            lines.append("")
        return "\n".join(lines)

    if version == "1.2.18":
        sections = {
            "版本概览": ["本版修复历史搜索定位，延续长线程性能优化和 100M／300M 线程颜色规则。"],
            "功能调整": ["搜索命中旧问答、工具结果或 AGENTS.md 指令时自动展开目标；再次搜索手动关闭的目标也可重新展开。"],
            "问题修复": [
                "修复已加载的旧问答因界面历史折叠而无法定位，以及关闭的工具组内结果没有正文锚点的问题。",
                "定位命中的具体消息或命令，保留其他折叠选择和分页位置，并清除已失效的搜索失败提示。",
            ],
            "兼容性与迁移": [
                "API 协议保持 2；保留增量读取、历史分页、通知、附件、Plan、子智能体和管理保护。",
                "无需重建连接或迁移原生会话；凭据、配置、通知游标及 Bark 去重状态继续保留。",
            ],
            "支持平台与资产": [
                "macOS ARM64：DMG／校验文件、updater 压缩包／签名及仅映射 darwin-aarch64 的 latest.json。",
                "Linux x86_64：API 服务包与校验文件；共七项发布资产。",
            ],
            "升级提示": [
                "先升级远程 API，再安装本机 App。安装前核对校验和与 updater 签名。",
                "macOS 包没有 Developer ID 签名或 Apple 公证；Minisign 用于验证 updater 来源，不能替代系统代码签名。",
            ],
        }
        lines = [f"# AgentHub {version}", ""]
        for heading, paragraphs in sections.items():
            lines.extend([f"## {heading}", ""])
            lines.extend(f"- {paragraph}" for paragraph in paragraphs)
            lines.append("")
        return "\n".join(lines)

    if version == "1.2.17":
        sections = {
            "版本概览": ["本版优化长线程与多会话列表的读取、页面切换和滚动，统一线程大小颜色分级。"],
            "功能调整": [
                "Codex、Claude Code、Grok 和子智能体详情分开刷新最新内容与加载较早历史；Grok 现在也支持持续上滚自动分页。",
                "三种 Provider 在本机与远程服务器统一按实际字节分色：小于 100M 为绿色，100M 至不足 300M 为琥珀色，300M 及以上为红色，1M = 1024² 字节。",
                "K／M／G 数值格式、准确字节提示和卡片布局保持原有规则；未知为灰色，部分结果保留 ≥ 和统计不完整提示。",
            ],
            "问题修复": [
                "减少未变化会话的重复解析、详情查询连带扫描列表，以及多会话列表反复淘汰正文缓存的开销。",
                "静态旧页不再随每次轮询重复请求；较早工具结果、待回答问题和子智能体状态仍会更新。机器或线程切换后丢弃迟到响应。",
                "线程首击立即选择，工作区按需加载，折叠正文展开时再渲染；缓存 Markdown 解析与时间线位置，保留搜索定位、复制、导出和滚动锚点。",
            ],
            "兼容性与迁移": [
                "API 协议保持 2，不新增公共命令或数据库表；分页和历史变更信息均为可选扩展，旧客户端与旧服务继续兼容。",
                "保留 Codex、Claude Code、Grok、Probe、设置、通知、附件和管理保护；文件替换、截断、原地改写及身份变化会使相应缓存失效。",
                "窗口隐藏时暂停界面轮询，后台 monitor 独立运行。整理当前文档和重复实现，原生会话、连接、凭据及 Bark 配置继续保留。",
            ],
            "支持平台与资产": [
                "macOS ARM64：DMG 安装包与 SHA-256 校验文件、updater 压缩包与签名。",
                "Linux x86_64：API 服务包与 SHA-256 校验文件；latest.json 仅映射 darwin-aarch64，共七项资产。",
            ],
            "升级提示": [
                "先升级远程 API，再安装本机 App。首次打开大文件仍需读取正文，后续刷新和历史阅读会复用缓存。",
                "安装前核对校验和及 updater 签名。macOS 包没有 Developer ID 签名或 Apple 公证，Minisign 不能替代系统代码签名。",
            ],
        }
        lines = [f"# AgentHub {version}", ""]
        for heading, paragraphs in sections.items():
            lines.extend([f"## {heading}", ""])
            lines.extend(f"- {paragraph}" for paragraph in paragraphs)
            lines.append("")
        return "\n".join(lines)

    if version == "1.2.16":
        sections = {
            "版本概览": ["本版改进长线程阅读：上滚即可继续查看历史，列表同时显示会话占用大小。"],
            "功能调整": [
                "Codex 主线程、Claude Code 和 Codex 子智能体详情在向上滚动到顶部附近时，自动逐页加载较早消息；首次打开仍定位到最新内容。",
                "三种 Provider 的线程大小放在原状态行右侧，按 1024 换算为 K／M／G，分别使用绿色、琥珀色和红色；悬停或键盘聚焦可查看准确字节数及统计范围。",
                "Codex 统计 rollout 文件；Claude Code 统计主 JSONL 及确认归属的会话专属目录；Grok 统计会话专属目录。本机与远程服务器采用相同口径。",
            ],
            "问题修复": [
                "取消日常阅读中的手动加载按钮，加载失败后才显示重试入口；历史耗尽时不保留空白区域。",
                "加载较早消息时保持可见内容及其像素位置，轮询追加、工具展开和继续滚动不会把阅读位置拉回底部。",
                "搜索与滚动共用分页协调，避免重复请求；轮询保留已加载历史及最早游标，机器或线程切换后丢弃旧响应。",
            ],
            "兼容性与迁移": [
                "API 协议保持 2，不新增公共命令或数据库表；现有会话、连接、凭据及 Bark 配置继续使用。",
                "目录在后台有界统计，缓存最长 30 秒刷新；不扫描项目工作目录或共享存储，不读取外部附件，不跟随符号链接。",
                "未知或旧服务缺少大小字段时显示 —，部分统计用 ≥ 标记；文件替换或截断会使旧缓存失效。",
            ],
            "支持平台与资产": [
                "macOS ARM64：DMG 安装包与 SHA-256 校验文件、updater 压缩包与签名。",
                "Linux x86_64：API 服务包与 SHA-256 校验文件；latest.json 仅映射 darwin-aarch64，共七项资产。",
                "不提供 Linux 桌面包、网页版或安卓安装包。",
            ],
            "升级提示": [
                "先升级远程 API，再安装本机 App；打开较长线程向上阅读，即可触发历史加载。目录大小可能在下一次列表刷新后显示。",
                "大小表示会话存储的逻辑字节数，不代表项目目录体积或删除后必然释放的磁盘空间。",
                "安装前核对校验和及 updater 签名。macOS 包没有 Developer ID 签名或 Apple 公证，Minisign 不能替代系统代码签名。",
            ],
        }
        lines = [f"# AgentHub {version}", ""]
        for heading, paragraphs in sections.items():
            lines.extend([f"## {heading}", ""])
            lines.extend(f"- {paragraph}" for paragraph in paragraphs)
            lines.append("")
        return "\n".join(lines)

    if version == "1.2.15":
        sections = {
            "版本概览": ["本版改善 Bark 在网络波动后的投递，增加持久化有限重试、分段恢复和明确的中文反馈。"],
            "功能调整": [
                "每个分段最多尝试 3 次，两次重试通常等待 15 秒、60 秒并加入 ±20% 抖动；从首次投递起最长保留 10 分钟，尊重服务端 Retry-After。",
                "重启后继续未确认的分段，已成功分段不重发；Hook 保持短等待，重试由后台 monitor 处理。",
                "界面区分等待重试、重试中、Bark 已受理、发送失败、结果未确认和已跳过，并显示次数及分段进度。",
            ],
            "问题修复": [
                "网络故障不再统一折叠为 request_error；按 DNS、连接、TLS、超时、响应读取、HTTP 或 Bark 拒绝分类，诊断不记录地址、Key 或通知正文。",
                "每次重试重新核对事件、开关与投递目标，等待期间修改配置也会停止失效通知；恢复成功更新原事件。",
                "测试统一使用“AgentHub 推送测试”和本机／远程服务器来源正文，等待真实受理结果；移除旧程序配置导入实现与旧产品文案。",
            ],
            "兼容性与迁移": [
                "API 协议保持 2，不新增公共命令或数据库表；保留现有会话、Keychain 连接、管理员 API Key、Bark 凭据及通知基线。",
                "升级不补发历史失败或终态未知结果。新队列加密保存正文，完成或停止后清除；凭据错误、证书验证失败、重定向及已知永久 APNs 错误不重试。",
                "本版优先减少漏推，结果不明时也有限重试，可能产生重复提醒。稳定通知 ID 仅辅助去重，不能保证客户端不再次响铃。",
            ],
            "支持平台与资产": [
                "macOS ARM64：DMG 安装包与 SHA-256 校验文件、updater 压缩包与签名。",
                "Linux x86_64：API 服务包与 SHA-256 校验文件；latest.json 仅映射 darwin-aarch64，共七项资产。",
                "不提供 Linux 桌面包、网页版或安卓安装包。",
            ],
            "升级提示": [
                "先升级远程 API，再安装本机 App；分别发送一次推送测试，在设备上确认收到。Bark 已受理只表示服务端确认。",
                "沿用现有通知设置；自有 monitor 日志过滤目标应为 agenthub_webd。旧配置导入入口不再执行。",
                "安装前核对校验和及 updater 签名。macOS 包没有 Developer ID 签名或 Apple 公证，Minisign 不能替代系统代码签名。",
            ],
        }
        lines = [f"# AgentHub {version}", ""]
        for heading, paragraphs in sections.items():
            lines.extend([f"## {heading}", ""])
            lines.extend(f"- {paragraph}" for paragraph in paragraphs)
            lines.append("")
        return "\n".join(lines)

    if version == "1.2.14":
        sections = {
            "版本概览": ["本版下线 Gotify 通道，通知继续使用 Bark，并保留完整 AGENTS.md 折叠修复。"],
            "功能调整": [
                "移除 Gotify 设置卡片、测试按钮、发送器和本机及远程执行入口。",
                "保留 Codex、Claude Code、Grok 的完成、失败及需要回复通知，现有事件识别规则不变。",
            ],
            "问题修复": [
                "升级清理旧通道残留，避免已下线服务继续出现在设置和投递结果中。",
                "保留 AGENTS.md 结束标签与环境标签同行时的完整折叠，内部标题不再把正文拆到用户气泡。",
            ],
            "兼容性与迁移": [
                "自动清理 AgentHub 自有的旧通道配置、加密 Token、待投递记录及专属测试任务，并清理 SQLite 遗留页。",
                "保留 Bark 凭据、成功记录、去重状态和启用基线，不补发历史通知；未知投递结果不自动重发。",
                "API 协议保持 2，原有会话、Keychain 连接、管理员 API Key 与后台 monitor 继续使用。旧测试 RPC 不再执行。",
            ],
            "支持平台与资产": [
                "macOS ARM64：DMG 安装包与 SHA-256 校验文件、updater 压缩包与签名。",
                "Linux x86_64：API 服务包与 SHA-256 校验文件；latest.json 仅映射 darwin-aarch64，共七项资产。",
                "不提供 Linux 桌面包、网页版或安卓安装包。",
            ],
            "升级提示": [
                "先升级远程 API，再安装本机 App。单独部署的 Gotify 服务需由管理员卸载；AgentHub 不自动删除外部服务数据。",
                "安装前核对校验和及 updater 签名。macOS 包没有 Developer ID 签名或 Apple 公证；Minisign 不能替代系统代码签名。",
            ],
        }
        lines = [f"# AgentHub {version}", ""]
        for heading, paragraphs in sections.items():
            lines.extend([f"## {heading}", ""])
            lines.extend(f"- {paragraph}" for paragraph in paragraphs)
            lines.append("")
        return "\n".join(lines)

    if version == "1.2.12":
        sections = {
            "版本概览": ["本版精简子智能体与清理页面的说明文字，延续 1.2.11 的功能修复。"],
            "功能调整": [
                "子智能体汇总和列表移除统计不完整的通用说明，继续展示运行中、已完成、未知及不可用状态。",
                "归档清理、隐藏线程清理移除标题下方的重复说明，保留预览、确认、执行状态和逐项结果。",
            ],
            "问题修复": ["清理面板移除说明后，状态标记仍保持右对齐，避免留下多余的说明区域。"],
            "兼容性与迁移": [
                "API 协议保持 2，无数据库或配置迁移。既有父子关联校验、清理保护、跳过规则和真实错误反馈保持有效。",
                "继续支持子智能体开始与完成活动、任务名称和直属计数；内部页面包装默认隐藏。",
            ],
            "支持平台与资产": [
                "macOS ARM64：DMG 安装包与 SHA-256 校验文件、updater 压缩包与签名。",
                "Linux x86_64：API 服务压缩包与 SHA-256 校验文件；latest.json 仅映射 darwin-aarch64，共七项资产。",
            ],
            "升级提示": [
                "升级至 1.2.12 即可应用界面调整；现有会话、连接、API Key、Keychain 和通知设置无需重建。",
                "安装前核对校验和与 updater 签名。macOS 包没有 Developer ID 签名或 Apple 公证，现有资源封装的系统签名校验限制仍在；Minisign 签名用于验证更新归档来源。",
            ],
        }
        lines = [f"# AgentHub {version}", ""]
        for heading, paragraphs in sections.items():
            lines.extend([f"## {heading}", ""])
            lines.extend(f"- {paragraph}" for paragraph in paragraphs)
            lines.append("")
        return "\n".join(lines)

    if version == "1.2.11":
        sections = {
            "版本概览": ["本版修复 Codex 子智能体开始活动、任务名称和运行数量的展示，并隐藏会话中的内部页面标记。"],
            "功能调整": [
                "线程详情增加直属子智能体汇总，显示运行中、已完成及异常状态数量；主线程列表保持原样。",
                "点击汇总打开右侧列表，运行中的子智能体优先显示。可进入详情、查看其直属下级并逐级返回，保留阅读位置和焦点。",
                "活动行、列表与详情统一使用创建任务名；缺少任务名时再回退到已验证路径、线程标题或昵称。",
            ],
            "问题修复": [
                "识别原生“开始工作”“已完成”“已中断”和交互记录，保留发生顺序；完成后不再把历史开始记录改成完成记录。",
                "子智能体当前状态依据自己的回合判断，父线程没有新消息时仍刷新。按直属原生身份去重，避免重复回合或孙级任务影响数量。",
                "默认隐藏完整的 external_codex_apps_open_page 内部包装，复制、Plan 导出和搜索同步过滤；纯包装不再留下空消息或时间线短线，代码与引用示例保留。",
            ],
            "兼容性与迁移": [
                "远程 API 协议保持 2，沿用现有详情接口；不新增 RPC、数据库表或原生会话写入。旧服务缺少汇总时保留阅读并提示升级。",
                "关联缺失、冲突或读取不完整时显示未知或不可用，不推断完成。保留现有线程管理保护、机器隔离、Keychain、Bark 和 monitor。",
            ],
            "支持平台与资产": [
                "macOS ARM64：DMG 安装包与 SHA-256 校验文件、updater 压缩包与签名。",
                "Linux x86_64：agenthub-webd API 服务包与 SHA-256 校验文件，不提供网页或 Linux 桌面包。",
                "latest.json 仅映射 darwin-aarch64；共七项资产，darwin-arm64 文件名对应同一 macOS 架构。",
            ],
            "升级提示": [
                "先将远程 API 服务升级到 1.2.11，再安装本机 App，即可查看完整的直属汇总；现有连接、凭据、会话和通知设置无需重建。",
                "安装前核对校验和与 updater 签名。macOS 包尚无 Developer ID 签名或 Apple 公证；Minisign 验证更新归档来源，不能替代系统代码签名。",
                "覆盖安装若触发钥匙串授权，请在系统对话框完成验证。部署参数继续从仓库外提供。",
            ],
        }
        lines = [f"# AgentHub {version}", ""]
        for heading, paragraphs in sections.items():
            lines.extend([f"## {heading}", ""])
            lines.extend(f"- {paragraph}" for paragraph in paragraphs)
            lines.append("")
        return "\n".join(lines)

    if version == "1.2.10":
        sections = {
            "版本概览": ["本版修复 macOS App 远程连接可能持续加载的问题，延续 1.2.9 的紧凑阅读布局与 Codex 子智能体详情。"],
            "功能调整": [
                "钥匙串读取超过 15 秒会显示当前操作的错误，方便判断并重试；不会自动切换目标机器。",
                "保存和移除连接保持完整事务，等待提交或回滚完成后才允许切换机器。",
            ],
            "问题修复": [
                "钥匙串访问改为后台串行执行，避免多个远程请求同时读取凭据并阻塞异步运行时。",
                "超时或取消后，仍在执行的原生操作继续持有串行许可；尚未开始的任务可以取消，避免积压。",
                "读取凭据和验证服务能力后重新核对连接身份，防止旧连接请求继续执行或覆盖新机器结果。",
            ],
            "兼容性与迁移": [
                "远程 API 协议保持 2，不新增 RPC、数据库字段或凭据缓存。现有 API Key、Keychain 连接、会话、Bark 和监测配置继续使用。",
                "保留三个 Provider 的阅读、搜索、工具折叠、Plan 导出、附件和线程管理，以及 Codex 子智能体只读详情。",
            ],
            "支持平台与资产": [
                "macOS ARM64：DMG 安装包与 SHA-256 校验文件、updater 压缩包与签名。",
                "Linux x86_64：agenthub-webd API 服务包与 SHA-256 校验文件，不提供网页或 Linux 桌面包。",
                "latest.json 仅映射 darwin-aarch64；共七项资产，darwin-arm64 文件名对应同一 macOS 架构。",
            ],
            "升级提示": [
                "建议使用远程管理的用户升级至 1.2.10。覆盖安装后直接使用已有连接；本机 App 和远程服务分别更新。",
                "安装前核对校验和与 updater 签名。macOS 包尚无 Developer ID 签名或 Apple 公证；Minisign 验证更新归档来源，不能替代系统代码签名。",
                "若系统请求钥匙串授权，请在 macOS 对话框完成验证。连接失败时查看当前操作反馈，不需要删除原有凭据或会话。",
            ],
        }
        lines = [f"# AgentHub {version}", ""]
        for heading, paragraphs in sections.items():
            lines.extend([f"## {heading}", ""])
            lines.extend(f"- {paragraph}" for paragraph in paragraphs)
            lines.append("")
        return "\n".join(lines)

    if version == "1.2.9":
        sections = {
            "版本概览": ["本版让会话阅读更紧凑，并为 Codex 子智能体提供可点击的活动行与右侧详情。"],
            "功能调整": [
                "Codex、Claude Code 和 Grok 统一正文、工具摘要与操作位置；桌面复制按钮在悬停或键盘聚焦时出现，触屏保持可见。",
                "Codex 子智能体显示任务名称与自身回合状态，可查看委派内容、消息、Plan、工具和附件。支持嵌套返回及独立历史分页。",
                "宽窗口并排显示详情，窄窗口使用覆盖面板；关闭后恢复父线程位置和焦点，切换机器或主线程会关闭面板。",
            ],
            "问题修复": [
                "减少重复 Provider 标签、活动组间距和复制按钮造成的空白，保留原有阅读列宽度与字号。",
                "子智能体状态按其自身原生回合判断；关联缺失或有歧义时显示原因，不按名称打开其他线程。",
                "修复 Hook 集成测试对编译期二进制路径的硬依赖；测试继续启动本次 Cargo 构建的真实 CLI，验证确认等待和通知去重。",
            ],
            "兼容性与迁移": [
                "API 协议保持 2，新增子智能体读取能力。远程旧服务缺少能力时提示升级，现有本机读取不受影响。",
                "子智能体详情和附件均验证原主线程及父子关系；不新增数据库表，不改写原生会话，不开放执行或管理控制。",
                "保留搜索、用户指令时间线、工具折叠、AGENTS.md、Plan 导出、线程管理、Bark、Keychain 与机器隔离。",
            ],
            "支持平台与资产": [
                "macOS ARM64：DMG 安装包与 SHA-256 校验文件、updater 压缩包与签名。",
                "Linux x86_64：agenthub-webd API 服务包与 SHA-256 校验文件，不提供网页或 Linux 桌面包。",
                "latest.json 仅映射 darwin-aarch64；共七项资产，darwin-arm64 文件名对应同一 macOS 架构。",
            ],
            "升级提示": [
                "使用远程子智能体详情前，请先升级 API 服务，再安装 1.2.9 App。保留现有 API Key、Keychain 连接、会话及通知设置。",
                "安装前核对校验和与 updater 签名。macOS 包尚无 Developer ID 签名或 Apple 公证；Minisign 验证更新归档来源，不能替代系统代码签名。",
                "覆盖安装若触发钥匙串授权，请在系统对话框完成验证，无需重建远程连接。部署参数继续从仓库外提供。",
            ],
        }
        lines = [f"# AgentHub {version}", ""]
        for heading, paragraphs in sections.items():
            lines.extend([f"## {heading}", ""])
            lines.extend(f"- {paragraph}" for paragraph in paragraphs)
            lines.append("")
        return "\n".join(lines)

    if version == "1.2.8":
        sections = {
            "版本概览": ["本版改进线程管理与桌面启动体验，修复隐藏清理被单个受保护线程阻断的问题，并完成 Pi 集成退役。"],
            "功能调整": [
                "右键线程立即显示操作菜单；保留省略号、触摸和键盘入口，右键未选中线程不切换当前详情。",
                "Codex 普通与归档线程均可永久删除，支持最多 100 项批量预览与确认。",
                "隐藏清理只处理本次预览候选，逐项显示删除、跳过、失败和剩余结果。",
            ],
            "问题修复": [
                "修复 Grok 短标题、长标题及运行状态下的卡片背景宽度不一致。",
                "主窗口隐藏创建后一次填满当前屏幕工作区，避免启动时反复缩放；Dock 重开和机器切换保留窗口尺寸，不切换独立全屏 Space。",
                "受保护、文件已变化或单项失败的隐藏线程不再阻断其余候选；公共数据库、索引或存储错误会回滚整批。",
            ],
            "兼容性与迁移": [
                "远程 API 协议升级为 2。旧服务不能执行新版隐藏清理，App 会提示更新。",
                "移除 AgentHub 的 Pi 导航、读取、管理、通知与专属配置；仅清理 AgentHub 自有 Pi 记录，不卸载 Pi，不修改其原生配置、会话或工作目录。",
                "Codex、Claude Code 与 Grok 的搜索、附件、时间线、Plan 导出、Bark、Keychain 连接和机器隔离继续保留。",
            ],
            "支持平台与资产": [
                "macOS ARM64：DMG 安装包与 SHA-256 校验文件、updater 压缩包与签名。",
                "Linux x86_64：agenthub-webd API 服务包与 SHA-256 校验文件，不提供网页或 Linux 桌面包。",
                "latest.json 仅映射 darwin-aarch64；以上共七项资产，darwin-arm64 文件名对应同一 macOS 架构。",
            ],
            "升级提示": [
                "使用远程管理时先升级 API 服务，再安装 1.2.8 App；本机读取不依赖远程升级。",
                "安装前核对校验和与 updater 签名。永久删除不可撤销，请核对预览目标；运行中、状态未知或关联异常的线程仍受保护。",
                "部署主机、域名和凭据继续由仓库外提供。发布前检查源码、可达历史与资产；平台保留的不可达对象不属于可达历史审计覆盖范围。",
            ],
        }
        lines = [f"# AgentHub {version}", ""]
        for heading, paragraphs in sections.items():
            lines.extend([f"## {heading}", ""])
            lines.extend(f"- {paragraph}" for paragraph in paragraphs)
            lines.append("")
        return "\n".join(lines)

    version_parts = tuple(int(part) for part in version.split("."))
    has_remote_api = version_parts >= (1, 2, 0)
    has_linux_desktop = version_parts < (1, 1, 0)
    assets = [
        "macOS ARM64：DMG 安装包与 SHA-256 校验文件、updater 压缩包与签名。",
        "Linux x86_64：agenthub-webd 服务包与 SHA-256 校验文件。",
    ]
    if has_linux_desktop:
        assets.extend([
            "Linux x86_64 桌面：AppImage、DEB、RPM，附校验文件；AppImage 另附 updater 签名。",
            "latest.json 同时包含 macOS ARM64 与 Linux x86_64 桌面映射；macOS updater 另附 SHA-256 文件，本版共十五项资产。",
        ])
    else:
        assets.append("latest.json 仅映射 darwin-aarch64；本版共七项资产，不提供 Linux 桌面包。")
    if has_remote_api:
        migration = [
            "macOS App 支持本机与远程管理；远程连接使用独立管理员 API Key，保存在本机 Keychain。",
            "Linux 服务仅提供管理 API 与健康检查，不提供网页登录。",
        ]
        upgrade = [
            "升级前核对校验文件；远程地址与 API Key 在 App 中配置，部署参数由仓库外提供。",
            "保留会话、凭据和已有监测配置；连接提示协议不兼容时，先更新远程服务。",
        ]
    else:
        migration = [
            "本版支持 macOS App 与云端 WebUI；云端使用该版本的网页登录与认证设置。",
            "升级不改写原生会话；部署地址与认证参数从仓库外提供。",
        ]
        upgrade = [
            "本页描述历史版本的实际能力；管理员 API Key 远程管理从 1.2.0 开始提供。",
            "新安装建议使用当前稳定版。升级前核对校验文件，并阅读目标版本的迁移说明。",
        ]
    sections = {
        "版本概览": [f"AgentHub {version}，本版主要调整如下。"],
        "功能调整": highlights(version),
        "兼容性与迁移": migration,
        "支持平台与资产": assets,
        "升级提示": upgrade,
    }
    lines = [f"# AgentHub {version}", ""]
    for heading, paragraphs in sections.items():
        lines.extend([f"## {heading}", ""])
        lines.extend(f"- {paragraph}" for paragraph in paragraphs)
        lines.append("")
    return "\n".join(lines)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("version", help="version without the leading v")
    parser.add_argument("--updater", action="store_true", help="render a short updater note")
    args = parser.parse_args()
    print(render(args.version, updater=args.updater), end="")
    return 0


if __name__ == "__main__":
    sys.exit(main())
