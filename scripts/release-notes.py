#!/usr/bin/env python3
"""Render concise Chinese release notes for GitHub Releases and updater metadata."""

from __future__ import annotations

import argparse
import sys


VERSION_NOTES: dict[str, dict[str, list[str]]] = {
    "2.0.8": {
        "features": [
            "精简前端生产包（剔除运行期 45 KB 契约 JSON 与自检循环），将桌面端与服务端 Probe 状态轮询、Codex SQLite 线程扫描及配置加载解析收敛为单次执行，并移除桌面启动遗留巡检与延迟探针注入。",
        ],
        "fixes": [
            "在浏览器与桌面端首帧绘制前同步读取并应用已保存的深色／浅色外观设置，补齐启动主题挂载顺序断言与发布说明分类校验。",
        ],
    },
    "2.0.7": {
        "features": [],
        "fixes": [
            "在界面首次渲染前应用已保存的深色或浅色外观设置，消除启动时的主题闪烁，并支持在设置页用键盘方向键切换外观。",
            "当本机后台服务程序损坏或无法读取时自动重新替换，提升桌面端启动自愈能力。",
        ],
    },
    "2.0.6": {
        "features": [
            "设置页新增「外观」切换（跟随系统／浅色／深色）并记住选择，统一 Codex 会话列表顶部标题栏与搜索框布局。",
        ],
        "fixes": [
            "优化桌面端启动时后台服务程序的比对速度，仅在文件变化时更新，并修正版本发布说明中的版本概览文案。",
        ],
    },
    "2.0.5": {
        "features": [
            "统一五大 Agent 会话列表标题字号，移除 Probe 与设置工作区 5 处副标题说明，并在五平台自检中对环境变量加锁及固定 Phase 4 测试时间戳。",
        ],
        "fixes": [
            "恢复 claude_hook::context 对 ClaudePaths::default_for_user 的目录边界校验，修复 macOS helper 原地覆盖导致的 SIGKILL 137 及 Linux 子进程 ETXTBSY 竞态。",
        ],
    },
    "2.0.4": {
        "features": [],
        "fixes": [
            "修复 Config::load 因默认通知分组自迁移判定在每次加载时原子重写 config.toml 的回归，并跳过轮询主路径上的 Antigravity 诊断构建与后台任务扫描开销。",
            "补齐跨回合挂起任务收敛补发断言、Claude Hook 端到端入队自检与 notification_snapshots 模块可见性收敛，并清理本机遗留旧文件名产物。",
        ],
    },
    "2.0.3": {
        "features": [
            "统一 Antigravity 判定与诊断逻辑、消除 Stop Hook 双重 allow 竞态并改为分离后台扫描，补齐游标边界与五平台真实解析器／Hook 自检，并将全仓、本机与远端底层包名、二进制、服务与路径全面统一为 AgentHub／agenthub。",
        ],
        "fixes": [
            "修复 Probe 五张宿主卡片标题栏与路径徽标挤压导致文案逐字竖排高度失衡问题，并收紧 E2E 卡片数量、单行与高度几何断言。",
        ],
    },
    "2.0.2": {
        "features": [
            "为 Codex 补齐黑白品牌令牌并将全局默认强调色切换为 AgentHub 星轨蓝，新增 diagnose-antigravity 诊断命令、五平台通知端到端自检、通知分组自动迁移及 Hook 归属严格校验。",
        ],
        "fixes": [
            "修复 Probe 工作台边框令牌自引用、胶囊开关样式、全局 overflow-wrap 逐字竖排及 13 个缺失样式类回归，根治 Antigravity Stop Hook 同步门禁期间 settled_count 游标抢跑导致的结尾通知漏发。",
        ],
    },
    "2.0.1": {
        "features": [
            "全面统一 AgentHub 品牌文案，将 styles.css 深度重构并真实减半至 47.8 KB（单行最长 136 字符），提取 useProviderWorkspace、StandardProviderEventView 与 SessionDeleteConfirmDialog 精简全部七大工作区。",
        ],
        "fixes": [
            "修复并自愈 Antigravity 顶层 agenthub Stop Hook 与 Grok Hook 宿主配置，消除 Antigravity／DSH／Grok／Claude 跨模块重复代码，并将 Codex 清理预扫描与 Probe 活跃扫描优化为单次流式读取。",
        ],
    },
}

HIGHLIGHTS: dict[str, list[str]] = {
    **{
        ver: [*entry.get("features", []), *entry.get("fixes", [])]
        for ver, entry in VERSION_NOTES.items()
    },
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

    version_parts = tuple(int(part) for part in version.split("."))
    if version_parts >= (2, 0, 1):
        entry = VERSION_NOTES.get(version)
        if entry is not None:
            features = list(entry.get("features", []))
            fixes = list(entry.get("fixes", []))
        else:
            notes_items = highlights(version)
            features = [notes_items[0]] if len(notes_items) > 1 else []
            fixes = notes_items[1:] if len(notes_items) > 1 else notes_items
        summary_items = [item.rstrip("。") for item in [*features, *fixes] if item]
        sections: dict[str, list[str]] = {
            "版本概览": [
                f"AgentHub {version}：{'；'.join(summary_items)}；API 协议保持 2。"
            ],
        }
        if features:
            sections["功能调整"] = features
        if fixes:
            sections["问题修复"] = fixes
        sections.update(
            {
                "兼容性与迁移": [
                    "API 协议保持 2；底层 Bundle ID（com.lich13.agenthub）、配置文件、SQLite 数据库、macOS Keychain 凭据与远端 agenthub-webd 守护进程名称保持不变，无需手工迁移任何数据。",
                    "本机安装脚本在安装 /Applications/AgentHub.app 时会自动检测并平滑替换旧版 /Applications/AgentHub.app，不保留旧版副本。",
                ],
                "支持平台与资产": [
                    "macOS ARM64：AgentHub DMG／校验文件、updater 压缩包／签名及仅映射 darwin-aarch64 的 latest.json。",
                    "Linux x86_64：API 服务包、SHA-256 校验文件与分离签名；共八项正式资产。",
                ],
                "升级提示": [
                    "先升级远端 API 服务，再原地覆盖安装本机 macOS App（AgentHub.app）；随后核验各 Agent 会话列表、Probe 状态与全平台线程清理。",
                    "DSH 归档、恢复与清理前须退出目标机器正在运行的 DSH 进程；真实用户会话清理、Bark 手机实收与日常视觉交互待用户验收。",
                    "macOS 包没有 Developer ID 签名或 Apple 公证；Minisign 验证归档来源，不能替代系统代码签名。",
                ],
            }
        )
        lines = [f"# AgentHub {version}", ""]
        for heading, paragraphs in sections.items():
            lines.extend([f"## {heading}", ""])
            lines.extend(f"- {paragraph}" for paragraph in paragraphs)
            lines.append("")
        return "\n".join(lines)

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
