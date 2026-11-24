#!/usr/bin/env python3
"""Render concise Chinese release notes for GitHub Releases and updater metadata."""

from __future__ import annotations

import argparse
import sys


VERSION_NOTES: dict[str, dict[str, list[str]]] = {
    "2.1.36": {
        "summary": [
            "根治思考块与工具循环中间说明异常展开，统一五大平台过程折叠体验，并完成全仓存量包袱深度瘦身"
        ],
        "fixes": [
            "修复思考块默认折叠失效的八类问题：含思考段的过程组不再因命令非零退出自动整组展开（闭合摘要保留失败计数，纯 AGENTS.md 失败同样保持闭合）；多步或含工具过程组内的思考段包裹紧凑二层披露行，单段纯思考组保持点开直读；独立思考行运行态默认闭合。",
            "补齐折叠状态存储的单调递增序号与全别名同步写回，向上翻页加载历史改变首条锚点后不再弹开已手动收起的过程组；单元格配置签名、行高缓存与复用池绑定会话作用域键，消除跨会话同 ID 过程组展开态串台；收紧搜索／时间线位置匹配，杜绝短位置键误命中过程组。",
            "修复 Antigravity 离线 SQLite .db 未预扫同轮后续规划步导致的中间回复展开及 Claude 无 turnId 顶层未聚合问题；Codex、Claude Code、Grok Build、Antigravity 与 DeepSeek Harness 工具循环内的已确认过渡说明统一归入当轮过程组默认折叠，每轮最终答复保持顶层独立展示。",
            "完成全仓深度减法：移除零引用依赖、重复契约测试、退役 API 错误分类与早于回退窗口的旧发版死分支；拆除 FFI 假请求套娃与字符串查询绕路，合并重复工作区侧栏控件、子智能体抽屉映射、确认遮罩与菜单锚点捕获视图（修正右对齐原生菜单坐标）；移除启动期创建遗留 ~/agenthub-workspace 目录副作用，并补齐 Finder 定位、Codex 桌面连接与本机 Hook 状态查询错误提示。",
        ],
        "migration": [
            "API 协议 2、数据库 schema 3。MessageBlock 新增可选 phase 字段，新旧端双向交错兼容；沿用已有配置、凭据与会话数据，先升级 Linux 服务再升级 Mac App。自 v2.1.31 写入绑定记录身份密文后，二进制回退目标保持为 v2.1.30。",
            "早于 schema 3 的数据库先经过 v2.0.10。",
        ],
    },
    "2.1.35": {
        "summary": [
            "根治 Antigravity 系统提示词与元数据占位泄漏，仅展示用户输入与纯净委派任务，并完成全仓存量包袱深度清理"
        ],
        "fixes": [
            "参考 Orca 在 Antigravity 摄取层统一剥离 <USER_REQUEST>、<ADDITIONAL_METADATA>、<USER_SETTINGS_CHANGE>、<SYSTEM_MESSAGE> 及截断占位；用户回合仅展示用户输入正文与附件（不生成额外「上下文」占位卡片，Codex 与 Claude Code 的「上下文」折叠保持不变），并跳过纯模型／设置变更空回合。",
            "修复 Antigravity 离线 SQLite .db 递归盲扫导致的系统提示词章节占位名（f19.f12）、展开工作流（f19.f3）、思考链（f20.f3）与内部堆栈（f24.f3.f3）混入对话及 Bark 摘要问题，改为精确 Protobuf 子字段解码；在线 Connect-RPC 优先读取 userInput.userResponse 原文并移除 32 页硬截断。",
            "修复未索引转录会话标题显示字面量 <USER_REQUEST>、history.jsonl 误过滤带参斜杠指令标题，以及子智能体漏读 subagentDescriptor.role／agent_name 与 /boost <original_task> 委派包装未提纯问题。",
            "清理零生产调用旧全量读取与过渡包装 API、合并 6 处重复有界 JSONL 行读取器与重复工作区归并／归档逻辑、拍平线程分页二次切片，并将 macOS 界面残余字符伪图标与 SwiftUI.Menu 统一替换为矢量图标与原生锚定菜单。",
        ],
        "migration": [
            "API 协议 2、数据库 schema 3。沿用已有配置、凭据与会话数据，先升级 Linux 服务再升级 Mac App；自 v2.1.31 写入绑定记录身份密文后，二进制回退目标保持为 v2.1.30。",
            "早于 schema 3 的数据库先经过 v2.0.10。",
        ],
    },
    "2.1.34": {
        "summary": [
            "新增跨平台「工作区」总览与 Git Worktree 线程归并，打通 agy CLI 全功能及双端 Bark 推送，修复全平台运行态与子代理防漏并完成架构深度精简"
        ],
        "features": [
            "参考 Orca 新增左侧顶栏「工作区」跨平台总览页及 5 个代码 Agent 页内「按工作区分组／筛选」：并发聚合 Codex、Claude Code、Grok Build、Antigravity 与 DeepSeek Harness 会话，轻量解析 .git worktree 归并至主仓库，支持含子目录边界筛选、视觉顺序多选与跨平台批量归档／删除。",
            "打通 agy CLI（~/.gemini/antigravity-cli）离线 Bark 探针推送，本机 macOS（Stop Hook + 15s 轮询）与远端 Linux agenthub-webd（15s 只读轮询，新增开放 antigravity_notifications）同步覆盖完成、待回复、权限等待与终态失败通知，并内置升级历史防风暴基线。",
        ],
        "fixes": [
            "全面修复 agy CLI 运行态、子代理防漏与离线管理：根会话 presence 锁树级继承、5 类父链来源防漏与孤儿清理、完整解码 raw_summary 及 .db steps 挂起／失败态、流式解析尾随说明 JSON、补齐缺失 cwd／标题与零时间过滤、对象安全改名及 15 列纳秒 FTS5 归档同步。",
            "修复 Codex、Claude Code、Grok 与 DSH 运行态识别及主列表净化：Codex 主列表排除 exec、thread_spawn_edges 与附着子线程，保留新版 session_meta 的 cwd 并收敛无存活宿主的遗留回合；Claude 按 statusUpdatedAt 因果收敛陈旧 busy、识别用户中断并排除 agent-*／纯 sidechain 文件；修复 Grok macOS 进程识别与子会话过滤及 DSH 审批／提问等待态。",
            "深度清理跨层直透包装、退役 <=767pt 手机壳层死分支与重复序列化，补齐 Linux 远端 github_ci、codex_account_scope、hidden_cleanup_scan、antigravity_sessions、dsh_sessions 五项协议 2 能力声明。",
        ],
        "migration": [
            "API 协议 2、数据库 schema 3。沿用已有配置、凭据与会话数据，先升级 Linux 服务再升级 Mac App；自 v2.1.31 写入绑定记录身份密文后，二进制回退目标保持为 v2.1.30。",
            "存量开启 Antigravity 通知的实例首次升级至 v2.1.34 时自动记录 CLI 历史基线，仅推送升级后的新回合事件；早于 schema 3 的数据库先经过 v2.0.10。",
        ],
    },
    "2.1.33": {
        "summary": [
            "修复 Antigravity 在线与离线运行态识别，补齐多级子智能体阅读、附件与拓扑级联清理，并完成存量架构精简"
        ],
        "features": [
            "Antigravity 新增多级子智能体支持（可缺省能力 antigravity_subagents）：展示内联子代理活动卡、详情顶栏状态汇总与右侧多级抽屉（支持 L0 -> L1 -> L2 下钻、实时状态回写、双向分页与子智能体图片附件读取）。",
            "全平台线程清理的「Antigravity 已归档与孤立子会话」改用单次批量预览，并纳入整条祖先链已不存在且自身无锁、非活跃的孤立子会话残余。",
        ],
        "fixes": [
            "修复 Antigravity 线程运行状态识别：覆盖 agy CLI 无索引 .pb 持锁会话、SQLite steps.status = 8（GENERATING）／9（WAITING）／7（ERROR）、Connect-RPC WAITING／错误终态及近期异步后台任务收敛，修正旧 RUNNING 转录行状态并补齐 Linux /proc 按需端点探测。",
            "修复工作区详情轮询周期冻结与侧栏状态不同步问题，补齐「等待处理」「待回复」「异常」徽章；Antigravity 级联删除改为叶节点优先拓扑逆序，并对未水合后代的在线 404 继续清理磁盘与索引残余。",
            "清理退役 WebUI／Tauri 遗留死分支、重复详情适配层、伪图标字符与静默吞错包装，Probe 配置或运行异常在控制台显式提示。",
        ],
        "migration": [
            "API 协议 2、数据库 schema 3。沿用已有配置、凭据与会话数据，先升级 Linux 服务再升级 Mac App；自 v2.1.31 写入绑定记录身份密文后，二进制回退目标保持为 v2.1.30。",
            "从 v2.1.27 或更早直接升级且尚未迁移偏好的用户，需要重新选择主题及侧栏折叠状态；早于 schema 3 的数据库先经过 v2.0.10。",
        ],
    },
    "2.1.32": {
        "summary": [
            "统一纳管 agy CLI（~/.gemini/antigravity-cli）与 Antigravity.app 2.34.0 会话阅读、改名、归档与清理，并升级 url 修复安全公告"
        ],
        "features": [
            "Antigravity 工作区同时扫描桌面端（ANTIGRAVITY_HOME / ~/.gemini/antigravity）与 agy CLI（ANTIGRAVITY_CLI_HOME / ~/.gemini/antigravity-cli），按主存储文件归属隔离跨端幽灵摘要，统一支持阅读、检索、体积统计、改名、归档／恢复、永久删除及 Linux 远端已归档会话统一清理。",
        ],
        "fixes": [
            "兼容 Antigravity.app 2.34.0 的 daemon/hub_<httpPort>.json 端点发现，过滤过期 PID 文件并校验同用户属主与本机回环监听端口，恢复在线管理与 Bark 探针通知；agy CLI 触发共用 Stop Hook 时直接无操作快速返回。",
            "离线改名与归档同步修补 conversation_summaries.raw_summary 的 ConversationAnnotations Protobuf 字段、将 FTS5 archived 改为整型 1／0 绑定、同步 cache/conversation_metadata.json 并失效四类 proto 缓存，确保与 agy --resume 状态一致。",
            "识别 agy CLI 活跃根／子代理与 presence/<id>.lock 运行态并在离线改名、归档、恢复与删除前校验占用锁；永久删除纳入 conversation_parents 子代理表及 history.jsonl、conversation_metadata.json、last_conversations.json 原子清理，macOS 界面完整透传归档／删除禁用原因与读取诊断。",
            "升级 url 至 >= 2.5.9（修复 RUSTSEC-2026-0592），Linux systemd 单元与自定义路径生成器补齐 antigravity-cli 读写白名单及 settings.json 只读保护。",
        ],
        "migration": [
            "API 协议 2、数据库 schema 3。沿用已有配置、凭据与会话数据，先升级 Linux 服务再升级 Mac App；自 v2.1.31 写入绑定记录身份密文后，二进制回退目标保持为 v2.1.30。",
            "从 v2.1.27 或更早直接升级且尚未迁移偏好的用户，需要重新选择主题及侧栏折叠状态；早于 schema 3 的数据库先经过 v2.0.10。",
        ],
    },
    "2.1.31": {
        "summary": ["设置密钥、通知、桌面发送队列和附件索引开始写入绑定记录身份的密文。"],
        "fixes": [
            "App 和 Linux 服务启动时用现有密钥转换设置与通知。首次打开本机协作时，使用原钥匙串密钥转换发送队列和附件索引。",
            "数据库转换保留原时间戳、待发状态、租约和分段确认位置。附件索引经过验证后原子替换。",
        ],
        "migration": [
            "API 协议 2、数据库 schema 3。v2.1.28／29 与新版 App 或服务可交错升级，先升级 Linux 服务再升级 Mac App。",
            "开始写入新密文后，二进制回退目标为 v2.1.30。保留现有密钥；转换无法验证旧密文时会报告错误并保留原记录。",
            "从 v2.1.27 或更早直接升级且尚未迁移偏好的用户，需要重新选择主题及侧栏折叠状态。",
        ],
    },
    "2.1.30": {
        "summary": ["修复原生桌面关闭、附件粘贴和远程请求的生命周期问题，保留协议 2 与 schema 3。"],
        "fixes": [
            "关闭 App 时，已提交的请求会返回结果或提示结果未确认。剪贴板、焦点或接收区改变时，释放尚未交付的附件。",
            "协作选单改用原生锚定菜单，阅读器预编译 Markdown 规则。修复 inprogress 会话的活动状态判定、后台作业终态写入失败的诊断及更新失败时的备份位置提示。",
            "Bark 新事件使用单段摘要，旧队列保留已有分段确认位置。业务审计保留 90 天，Linux 安装器沿用现有 Provider 数据目录。",
        ],
        "migration": [
            "v2.1.30 可读取旧密文及按记录身份绑定的新密文，本版继续写旧格式。先升级 Linux 服务，再升级 Mac App。协议 2 支持与 v2.1.28／29 交错升级。",
            "从 v2.1.27 或更早版本直接升级且尚未迁移偏好的用户，需要重新选择主题及侧栏折叠状态。早于 schema 3 的数据库先经过 v2.0.10。",
        ],
    },
}


def highlights(version: str) -> list[str]:
    entry = VERSION_NOTES.get(version)
    if entry is None:
        raise SystemExit(f"version {version} is not registered in VERSION_NOTES")
    items = [*entry.get("features", []), *entry.get("fixes", [])]
    if not items:
        raise SystemExit(f"version {version} has no features or fixes in VERSION_NOTES")
    return items


def render(version: str, *, updater: bool = False) -> str:
    entry = VERSION_NOTES.get(version)
    if entry is None:
        raise SystemExit(f"version {version} is not registered in VERSION_NOTES")
    features = list(entry.get("features", []))
    fixes = list(entry.get("fixes", []))
    if not features and not fixes:
        raise SystemExit(f"version {version} has no features or fixes in VERSION_NOTES")
    if updater:
        return f"AgentHub {version}：" + "；".join(item.rstrip("。") for item in highlights(version)[:2])
    summary_items = [item.rstrip("。") for item in entry.get("summary", [*features, *fixes]) if item]
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
                "API 协议保持 2；底层 Bundle ID（com.lich13.agenthub）和远端 agenthub-webd 服务名称保持不变，保留原生会话与业务数据，具体升级要求见下文。",
                "本机安装脚本在安装 /Applications/AgentHub.app 时会自动检测并平滑替换旧版 /Applications/AgentHub.app，不保留旧版副本。",
                *entry.get("migration", []),
            ],
            "支持平台与资产": [
                "macOS ARM64：AgentHub DMG／校验文件、updater 压缩包／签名及仅映射 darwin-aarch64 的 latest.json。",
                "Linux x86_64：服务包、SHA-256 校验文件与分离签名；共八项正式资产。",
            ],
            "升级提示": [
                "先升级远端服务，再原地覆盖安装本机 macOS App（AgentHub.app）；保留既有数据与用户配置。",
                "DSH 归档、恢复与清理前须退出目标机器正在运行的 DSH 进程。",
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


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("version", help="version without the leading v")
    parser.add_argument("--updater", action="store_true", help="render a short updater note")
    args = parser.parse_args()
    print(render(args.version, updater=args.updater), end="")
    return 0


if __name__ == "__main__":
    sys.exit(main())
