# AgentHub

AgentHub（底层包标识 `com.lich13.nexushub`，服务端守护进程 `nexushub-webd`）是一款面向 macOS 的多平台 AI Agent 会话阅读、通知探针与安全清理工作台。它采用纯正 ChatGPT macOS Desktop App 的三层中性表面设计（`--bg-rail` / `--bg-list` / `--bg-stage`），在单一原生界面中统一纳管 **Codex**、**Claude Code**、**Grok Build**、**Antigravity** 与 **DeepSeek Harness（DSH）** 五大 Agent 的本机会话，并支持通过加密 API Key 连接管理多台命名远程 Linux 主机。本机未归档的 Codex 主线程支持连接原生 Codex 桌面端继续协作。

## 核心能力

- **五平台统一阅读与全局检索**：按工作区切换浏览 Codex、Claude Code、Grok Build、Antigravity 与 DeepSeek Harness 会话；支持 `⌘K` 跨五大 Agent 全局搜索、`⌘F` 当前会话／当前工作区搜索，以及右侧独立 20px 用户指令时间线刻度定位（仅标记真实用户指令，悬停预览、点击跨越未加载分页直达目标）。
- **无框流式排版与过程折叠**：保持文本与工具调用的原始时序；同一回合内连续的思考、过渡说明与命令／工具执行自动收敛为单行轻量披露块（`<details>`），最终答复独立清晰展示；`AGENTS.md`、环境与自动上下文默认合并为单行“上下文”入口，搜索命中时自动展开对应折叠块并保留用户手动展开状态。
- **最优交接路径与一键定位**：五大平台统一提供复制线程 ID、复制最优交接文件路径（Codex 明文／归档／`.jsonl.zst` rollout、Claude 主 JSONL、Grok `chat_history.jsonl` 优先、Antigravity 纯文本转录优先、DSH 明文／`.zstd` 日志）、本机 `在 Finder 中显示`（`⌘⇧R`）、复制助手纯净 Markdown 回复，以及 Codex／Grok Plan Markdown 导出。
- **访达风格多选与底部归档抽屉**：五大平台共用 `⌘` 单击切换、`Shift` 连续区间追加、`⌘A` 全选、`Escape` 清空、`⌘⌫` 批量删除及右键上下文菜单；双击或按 `F2` 行内改名。Codex、Antigravity 与 DSH 在列表底部提供按需加载的可折叠「已归档」抽屉，支持单条或批量归档、恢复与包含已核验子代理闭包的永久删除。
- **控制台优先 Probe 与五平台 Hooks／Host 矩阵**：进入 Probe 即呈现全局主控开关、Bark 通道配置与测试推送、五大 Agent 平台通知一键开关，以及结构完全一致的五平台 Hooks／Host 管理卡片（Codex `~/.codex/hooks.json`、Claude Code `~/.claude/settings.json`、Grok Build `~/.grok/hooks/nexushub.json`、Antigravity `~/.gemini/config/hooks.json` 一键安装／修复／移除，DSH 原生 `session.lock` + `.zstd` 流直连），实时事件与 15 秒增量轮询共享 SQLite 原子幂等去重。
- **全平台线程清理与严格执行端防护**：「设置 → 更新与维护 → 全平台线程清理」覆盖五大平台六大清理作用域（Codex 已归档、Codex 隐藏线程、Antigravity 已归档、DSH 已归档、Claude Code 空／闲置会话、Grok 空／闲置会话），支持流式分批扫描、100 条分页候选预览与按分类勾选清理。
- **多台命名远程机器管理**：「设置 → 远程连接」支持添加、编辑、切换与移除多台自定义名称的远程主机；每台机器的管理员 API Key 按 `machine_id` 独立保存在 macOS Keychain 中，旧版单远程配置首次启动时自动无损迁移，切换机器时自动隔离查询缓存、分页与写操作状态。

所有模型推理与工具执行始终由各 Agent 原生程序完成；AgentHub 不捆绑模型运行环境，也不改写未知格式的原生会话文件。

## 本机 Codex 桌面协作

打开本机未归档的 Codex 主线程，在底部悬浮协作区点击「在 Codex 中打开」即可唤起原生 Codex 桌面打开该线程并自动建立连接；也可在「新建线程」中选择工作目录创建空线程并自动连接。普通浏览历史会话保持纯只读，不会后台启动协作连接。兼容性按实际握手协议与线程归属判定，不按固定 build 号拦截；若遇协议不兼容或线程 owner 变更，界面自动回退为只读阅读并提示在 Codex 桌面端处理。

协作输入区支持：

- **发送、排队、补充与停止**：按 `⌘+Enter` 发送；运行中可将新消息「排队」至下一回合、「补充」注入当前回合，或点击「停止」中断当前回合并暂停本机待发送队列。待发送队列在本机加密持久化，断线或重启后需手动确认继续；投递结果不明的记录需先在 Codex 核对，绝不自动重发。
- **附件与剪贴板接入**：支持原生文件选择器、拖拽上传、Finder 文件复制粘贴、每行一条绝对路径／`~/` 路径／`file://` URL 自动识别，以及截图（PNG／TIFF／JPEG）直接粘贴。
- **技能、应用、插件与模型切换**：输入 `$` 检索技能、`@` 检索应用与已启用插件；点击模型摘要可切换原生模型、思考强度与默认／Plan 模式，并实时同步原生 Goal 状态与命令／文件 diff 审批。

详细协议边界见[架构说明](docs/ARCHITECTURE.md#本机-codex-桌面协作)。

## 阅读、分页与体积统计

- **双向无感分页與锚点保持**：五大 Agent 主会话与 Codex 子智能体详情均支持上滚至距顶部 `200px` 时自动加载较早历史分页，轮询、追加消息与展开工具组时严格保持当前可见消息像素偏移，不跳回底部。
- **有界内存与增量复用**：服务端对五大平台的列表扫描采用基于 `(mtime, size)` 的有界内存／SQLite 投影缓存（包括 DSH `.zstd` 折叠元数据缓存、Antigravity 本地端点缓存、Claude 活动快照短缓存与 Probe 实时态势缓存）；会话详情缓存空闲 TTL 为 **300 秒（5 分钟）**，超期自动释放并收缩容量。窗口隐藏时前端自动暂停常规轮询，恢复可见时立即刷新。
- **线程体积分级（K／M／G）**：五大平台会话卡片右侧以等宽数字展示实际字节大小，按统一阈值着色：**`< 100 MiB` 绿色**、**`100 MiB – 300 MiB` 琥珀色**、**`>= 300 MiB` 红色**；未知或后台统计中显示灰色 `—`，部分下限结果标 `≥`。统计仅涵盖已确认归属的 rollout、会话 JSONL、专属子目录与压缩日志，不计入项目代码工作目录，也不跟随符号链接。

## 会话删除与全平台清理安全边界

无论是会话列表的单条／批量永久删除，还是「更新与维护 → 全平台线程清理」中的六大分类清理，均严格执行以下安全红线：

1. **预览指纹绑定与允许集重算**：执行端绝不信任客户端传入的路径或 ID 列表，每次执行前均在服务端重新枚举并计算当前允许清理集合，并与用户预览确认的 `(id, fingerprint)` 严格取交集——预览后新增或发生内容变化的会话一律跳过，不会误删。
2. **运行态与锁占用强制拦截**：执行前对每个目标（含 Codex／Antigravity／DSH 的全部已核验子代理后代闭包）逐一核查活跃进程、文件锁（如 `session.lock`、`LOCK_SH`）与宿主运行状态（DSH 归档／恢复／删除前须退出正在运行的 DSH 进程）。任一目标处于运行或加锁状态即拒绝删除。
3. **首错即停与残留如实报告**：批量或统一清理按序逐项执行，遇到非预期的权限、数据库、RPC 或文件系统错误时立即停止后续删除（`stopped_early = true`），并分开报告**已删除数量**、**实际回收字节数**、**跳过项**与**残留项（`residuals`）**。
4. **专属范围与符号链接隔离**：仅清理确认归属的会话文件、专属旁路（writer lock、shell snapshot、visualization、brain 旁路等）并原子更新共享索引（无命中时不重写共享文件）；目录内遇到符号链接仅删除链接本身，绝不沿符号链接越界删除外部文件。

## Probe 通知探针与五平台 Hooks

Bark 通知统一采用 `Provider｜状态｜线程标题` 格式（状态涵盖：`完成`、`需要回复`、`需要授权`、`失败`），正文提炼最多 4 个非空行、300 个 Unicode 字符的纯净摘要，剥离 Markdown、代码块、工具输出与内部元数据。

- **五平台通知一键开关**：在 Probe 首屏即可独立启停 Codex、Claude Code、Grok Build、Antigravity 与 DeepSeek Harness 通知（Antigravity 与 DSH 仅在本机运行时生效）。
- **四平台实时 Hooks + DSH 原生流**：
  - **Codex**：管理 `~/.codex/hooks.json` 的 Stop Hook，并保留逐问题识别与去重；
  - **Claude Code**：管理 `~/.claude/settings.json` 的 `Stop`、`Notification`、`StopFailure` Hooks；
  - **Grok Build**：管理 `~/.grok/hooks/nexushub.json`，将完成、失败与待回复延迟从 15 秒轮询降至 `<1 秒` 实时回调，同时保留 15 秒轮询兜底；
  - **Antigravity**：管理 `~/.gemini/config/hooks.json` 中 `"nexushub"` 命名空间的 `Stop` Hook（立即返回 `{"decision":"allow"}` 不阻塞 IDE），实现回合结束瞬间实时推送，并与 15 秒原生 Language Server 轮询共享去重；
  - **DeepSeek Harness**：直接通过原生 `session.lock` 与 `.zstd`／v4 JSONL 增量流及 `turn/end` 精确结算，无需外部 Hook 文件即可零侵入实时监测。
- **安全校验与有限重试**：Bark 仅允许 HTTPS 服务或 HTTP 本机回环地址；临时网络故障按既有退避策略有限重试，重启后继续未确认分段，升级或首次启用只建立基线、不补发历史通知。

## 运行与部署

### 本机 macOS App

下载安装 `AgentHub-<版本>-darwin-arm64.dmg`（应用包名 `AgentHub.app`，Bundle ID 保持 `com.lich13.nexushub` 以无缝沿用既有配置、数据库与系统钥匙串）。本机安装脚本会自动退出运行中的旧实例、原地替换 `/Applications/AgentHub.app`（若存在旧版 `/Applications/NexusHub.app` 会一并无损迁移清理）并校验签名与版本号。

### 远程 Linux API 服务

Linux `nexushub-webd` 仅提供受管理员 API Key（`x-api-key`）保护的 `/api/rpc/*` 管理接口与 `/healthz` 健康检查，仅绑定回环地址，由前置 HTTPS 反向代理对外提供服务。部署参数须从仓库外提供：

```bash
NEXUSHUB_DOMAIN=panel.example.invalid \
  bash scripts/deploy-cloud.sh SSH_HOST ./dist/nexushub-webd-linux-x86_64.tar.gz
```

在远端服务器生成独立管理员 API Key 并填入桌面端「设置 → 远程连接」：

```bash
sudo /usr/local/bin/nexushub-webd admin key-generate --output /absolute/private/api-key
```

保存成功后立即删除临时 Key 文件。完整步骤见[云端部署手册](docs/cloud-deploy-runbook.md)。

## 发布资产

当前源码版本为 **2.0.1**；[查看 GitHub Releases](https://github.com/lich13/nexushub/releases)。每次正式发布包含八项资产：

- `AgentHub-<version>-darwin-arm64.dmg` 与 `.dmg.sha256`；
- `nexushub-darwin-arm64.tar.gz` 与 `.tar.gz.sig`（Tauri updater 归档与 Minisign 签名）；
- 仅包含 `darwin-aarch64` 平台映射的 `latest.json`；
- `nexushub-webd-linux-x86_64.tar.gz`、`.tar.gz.sha256` 与 `.tar.gz.sig`。

## 开发与验证

```bash
corepack pnpm@12.11.2 --dir webui install --frozen-lockfile
node scripts/check-wire-types.mjs
node --test scripts/test-wire-types.mjs
node --test scripts/test-playwright-browser.mjs
cargo fmt --all -- --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo test --manifest-path src-tauri/Cargo.toml --locked
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --locked -- -D warnings
corepack pnpm@12.11.2 --dir webui typecheck
corepack pnpm@12.11.2 --dir webui test
corepack pnpm@12.11.2 --dir webui build:tauri
bash scripts/test-install-script.sh
python3 scripts/check-versions.py
python3 scripts/check-workspaces.py
python3 scripts/test-install-safety.py
python3 scripts/test-install-signatures.py
python3 scripts/test-privacy-check.py
python3 scripts/test-check-versions.py
python3 scripts/test-check-workspaces.py
python3 scripts/test-tauri-build-config.py
node --test scripts/test-security-audit.mjs
python3 scripts/privacy-check.py --git-objects
git diff --check
```

所有单元与集成测试均在独立的临时沙箱目录（`TempDir`）中运行，绝不读写或删除宿主机真实用户会话。Playwright 跨引擎（Chromium／WebKit）端到端回归与 macOS 有头交互帧预算检查默认由 GitHub Actions CI 执行。

## 当前文档

- [AGENTS.md](AGENTS.md)：协作、安全与交付规范。
- [DESIGN.md](DESIGN.md)：三层表面、组件与交互设计规范。
- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)：模块边界、缓存预算与线协议契约。
- [docs/cloud-deploy-runbook.md](docs/cloud-deploy-runbook.md)：Linux API 服务部署、升级与回滚手册。
- [docs/progress/MASTER.md](docs/progress/MASTER.md)：维护者版本演进与已验证证据。
