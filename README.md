# AgentHub

AgentHub（底层包标识 `com.lich13.agenthub`，服务端守护进程 `agenthub-webd`）是一款面向 macOS 的多平台 AI Agent 会话阅读、通知探针与安全清理工作台。它采用纯正 ChatGPT macOS Desktop App 的三层中性表面设计（`--bg-rail` / `--bg-list` / `--bg-stage`），在单一原生界面中统一纳管 **Codex**、**Claude Code**、**Grok Build**、**Grok Bot**、**Antigravity** 与 **DeepSeek Harness（DSH）** 六个 Provider 的本机会话，并支持通过系统 SSH 连接管理多台命名远程 Linux 主机。本机未归档的 Codex 主线程支持连接原生 Codex 桌面端继续协作。

v2.1.3 支持新安装及已由 v2.1.0 完成迁移的配置。更早版本先升级至 v2.1.0；早于 schema 3 的数据库须先经过 v2.0.10。协议保持 2、数据库 schema 保持 3。

## 核心能力

- **统一阅读与检索**：浏览 Codex、Claude Code、Grok Build、Grok Bot、Antigravity 与 DeepSeek Harness；`⌘F` 搜索当前线程或当前 Provider，支持历史分页与原生消息定位。达到 200 条结果后停止后续读取，当前线程不受最近列表数量限制。
- **Grok Bot 近期缓存**：本机独立入口只读当前账户的 `.blob`／`.kblob`；复制实际存在的线程缓存路径，未缓存线程仍保留列表项并禁用路径动作。不提供改名、归档、删除或发送，也不承诺完整云端历史。
- **无框流式排版与过程折叠**：保持文本与工具调用的原始时序；同一回合内连续的思考、过渡说明与命令／工具执行自动收敛为单行轻量披露块（`<details>`），最终答复独立清晰展示；`AGENTS.md`、环境与自动上下文默认合并为单行“上下文”入口，搜索命中时自动展开对应折叠块并保留用户手动展开状态。
- **最优交接路径与一键定位**：五大平台统一提供复制线程 ID、复制最优交接文件路径（Codex 明文／归档／`.jsonl.zst` rollout、Claude 主 JSONL、Grok `chat_history.jsonl` 优先、Antigravity 纯文本转录优先、DSH 明文／`.zstd` 日志）、本机 `在 Finder 中显示`（`⌘⇧R`）、复制助手纯净 Markdown 回复，以及 Codex／Grok Plan Markdown 导出。
- **访达风格多选与底部归档抽屉**：五大平台共用 `⌘` 单击切换、`Shift` 连续区间追加、`⌘A` 全选、`Escape` 清空、`⌘⌫` 批量删除及右键上下文菜单；双击或按 `F2` 行内改名。Codex、Antigravity 与 DSH 在列表底部提供按需加载的可折叠「已归档」抽屉，支持单条或批量归档、恢复与包含已核验子代理闭包的永久删除。
- **控制台优先 Probe 与五平台 Hooks／Host 矩阵**：进入 Probe 即呈现全局主控开关、Bark 通道配置与测试推送、六个 Provider 平台通知一键开关，以及结构完全一致的五平台 Hooks／Host 管理卡片（Codex `~/.codex/hooks.json`、Claude Code `~/.claude/settings.json`、Grok Build `~/.grok/hooks/agenthub.json`、Antigravity `~/.gemini/config/hooks.json` 一键安装／修复／移除，DSH 原生 `session.lock` + `.zstd` 流直连），实时事件与 15 秒增量轮询共享 SQLite 原子幂等去重。
- **GitHub CI 与 Bark**：在所选机器的「Probe → GitHub CI」读取当前 `github.com` 活跃 `gh` 账号，自动发现账号名下的公开、私有及 fork 仓库；每 60 秒刷新运行，每 10 分钟刷新仓库。首次启用、重新启用和切换账号不补发旧结果，之后按每次运行尝试推送成功、失败及超时；通知可打开对应 GitHub 工作流。两端默认关闭，选择一端开启监控并启用 Bark。
- **全平台线程清理与严格执行端防护**：「设置 → 更新与维护 → 全平台线程清理」覆盖五大平台六大清理作用域（Codex 已归档、Codex 隐藏线程、Antigravity 已归档、DSH 已归档、Claude Code 空／闲置会话、Grok 空／闲置会话），支持流式分批扫描、100 条分页候选预览与按分类勾选清理。
- **多台命名远程机器管理**：「设置 → 远程连接」支持添加、编辑、切换与移除多台自定义名称的远程主机；直接选择本机 OpenSSH 配置及 Include 中保存的 Host，复用密钥、ssh-agent 与跳板配置，并支持「安装并连接」同版本服务。切换机器隔离缓存、分页及迟到响应；已受理后台任务继续执行，不阻止切换。

所有模型推理与工具执行始终由各 Agent 原生程序完成；AgentHub 不捆绑模型运行环境，也不改写未知格式的原生会话文件。

在「设置 → 远程连接」选择 Host，可确认一次后「删除 SSH 配置」：移除该别名在用户配置及 Include 中的显式定义与对应连接记录，其他别名、密钥和服务器数据保留；系统配置不能删除。删除当前连接后返回本机。断线恢复沿用当前机器、页面和阅读位置，读取最多自动重试一次，写操作不重放。

## 本机 Codex 桌面协作

打开本机未归档的 Codex 主线程，在底部悬浮协作区点击「在 Codex 中打开」即可唤起原生 Codex 桌面打开该线程并自动建立连接；也可在「新建线程」中选择工作目录创建空线程并自动连接。普通浏览历史会话保持纯只读，不会后台启动协作连接。兼容性按实际握手协议与线程归属判定，不按固定 build 号拦截；若遇协议不兼容或线程 owner 变更，界面自动回退为只读阅读并提示在 Codex 桌面端处理。

协作输入区支持：

- **发送、排队、补充与停止**：按 `⌘+Enter` 发送；运行中可将新消息「排队」至下一回合、「补充」注入当前回合，或点击「停止」中断当前回合并暂停本机待发送队列。待发送队列在本机加密持久化，断线或重启后需手动确认继续；投递结果不明的记录需先在 Codex 核对，绝不自动重发。
- **附件与剪贴板接入**：支持原生文件选择器、拖拽上传、Finder 文件复制粘贴、每行一条绝对路径／`~/` 路径／`file://` URL 自动识别，以及截图（PNG／TIFF／JPEG）直接粘贴。
- **技能、应用、插件与模型切换**：输入 `$` 检索技能、`@` 检索应用与已启用插件；点击模型摘要可切换原生模型、思考强度与默认／Plan 模式，并实时同步原生 Goal 状态与命令／文件 diff 审批。

详细协议边界见[架构说明](docs/ARCHITECTURE.md#本机-codex-桌面协作)。

## 阅读、分页与体积统计

- **双向无感分页與锚点保持**：各 Provider 主会话与 Codex 子智能体详情均支持上滚至距顶部 `200px` 时自动加载较早历史分页，轮询、追加消息与展开工具组时严格保持当前可见消息像素偏移，不跳回底部。
- **有界内存与读取复用**：服务端按文件身份及修改信息复用未变化的列表和历史。Grok Bot、Antigravity 与 DSH 共享最多 **32 MiB／64 项**历史缓存，DSH 摘要按 **8 MiB／2000 项**淘汰旧项；超过预算仍正常读取。Antigravity 运行中会话继续读取原生状态，文件替换、截断或修改会使缓存失效。详情缓存空闲 TTL 为 **300 秒（5 分钟）**；窗口隐藏时前端暂停常规轮询，恢复可见时立即刷新。
- **线程体积分级（K／M／G）**：五大平台会话卡片右侧以等宽数字展示实际字节大小，按统一阈值着色：**`< 100 MiB` 绿色**、**`100 MiB – 300 MiB` 琥珀色**、**`>= 300 MiB` 红色**；未知或后台统计中显示灰色 `—`，部分下限结果标 `≥`。统计仅涵盖已确认归属的 rollout、会话 JSONL、专属子目录与压缩日志，不计入项目代码工作目录，也不跟随符号链接。

## 会话删除与全平台清理安全边界

会话列表的直接删除与「更新与维护 → 全平台线程清理」保留各自已有校验，执行范围以确认请求为准：

1. **固定确认范围**：Codex、Antigravity 与 DSH 只删除提交的根 ID 和后代 ID，执行时复核后代归属，不纳入预览后新增的子会话。统一清理另按预览指纹与最新允许集求交集。
2. **保留平台写入边界**：Antigravity 检查所选会话的运行状态与离线占用锁；DSH 归档、恢复与删除须退出对应宿主，并检查所选会话锁。Codex、Claude 与 Grok 的直接删除不因正文读取或活动状态额外设限。
3. **逐项报告结果**：直接批量操作保留每项成功、失败及残留；Antigravity／DSH 单项子树失败时停止该子树后续删除，统一清理沿用首错即停规则。已成功项目不因其他项目失败回滚。
4. **专属范围与符号链接隔离**：仅清理确认归属的会话文件、专属旁路并更新共享索引；符号链接只删除链接本身，不沿链接删除外部文件。

## Probe 通知探针与五平台 Hooks

Probe 仅保留控件、状态、错误、事件数据和确认对象；「全平台实时态势」继续显示现有聚合结果，前端最多显示 12 条。

Bark 通知统一采用 `Provider｜状态｜线程标题` 格式（状态涵盖：`完成`、`需要回复`、`需要授权`、`失败`），正文提炼最多 4 个非空行、300 个 Unicode 字符的纯净摘要，剥离 Markdown、代码块、工具输出与内部元数据。

- **平台通知开关**：Probe 首屏独立控制各 Provider；Grok Bot 默认关闭，仅在本机按原生待回复／待授权请求提醒，不推断完成或失败。缓存淘汰、替换、重新启用与重启不重放旧提醒，首发和重试均复核请求仍待处理。Antigravity 与 DSH 通知也仅在本机生效。
- **四平台实时 Hooks + DSH 原生流**：
  - **Codex**：管理 `~/.codex/hooks.json` 的 Stop Hook，并保留逐问题识别与去重；
  - **Claude Code**：管理 `~/.claude/settings.json` 的 `Stop`、`Notification`、`StopFailure` Hooks；
  - **Grok Build**：管理 `~/.grok/hooks/agenthub.json`，将完成、失败与待回复延迟从 15 秒轮询降至 `<1 秒` 实时回调，同时保留 15 秒轮询兜底；
  - **Antigravity**：管理 `~/.gemini/config/hooks.json` 中 `"agenthub"` 命名空间的 `Stop` Hook（立即返回 `{"decision":"allow"}` 不阻塞 IDE），实现回合结束瞬间实时推送，并与 15 秒原生 Language Server 轮询共享去重；
  - **DeepSeek Harness**：直接通过原生 `session.lock` 与 `.zstd`／v4 JSONL 增量流及 `turn/end` 精确结算，无需外部 Hook 文件即可零侵入实时监测。
- **安全校验与有限重试**：Bark 仅允许 HTTPS 服务或 HTTP 本机回环地址；临时网络故障按既有退避策略有限重试，升级和重启保留已有队列与未确认分段。首次启用或重新启用建立基线，不补发此前历史通知。
- **正文与去重保留**：待发正文加密保存，终态清除正文副本；保留期清理仍保护当前游标需要的去重身份，避免旧完成事件重新发送。桌面端首次运行生成独立密钥，已有配置或密钥异常会明确报错并保留数据。

## 运行与部署

### 本机 macOS App

下载安装 `AgentHub-<版本>-darwin-arm64.dmg`（应用包名 `AgentHub.app`，Bundle ID 保持 `com.lich13.agenthub` 以无缝沿用既有配置、数据库与系统钥匙串）。本机安装脚本会自动退出运行中的旧实例、原地替换 `/Applications/AgentHub.app`（若存在旧版 `/Applications/AgentHub.app` 会一并无损迁移清理）并校验签名与版本号。

### 远程 Linux SSH 服务

在「设置 → 远程连接」选择本机已保存的 SSH Host，按服务器权限选择「使用 sudo」，然后「保存并连接」或「安装并连接」。需要密码、首次主机确认或密钥交互时，先在终端完成认证。App 不复制私钥或保存 SSH 密码，不修改 sudoers。

桌面通过常驻 SSH 通道运行固定的 `agenthub-webd rpc-stdio`，转发至服务端私有 Unix 套接字；通知和后台任务由常驻服务继续执行。断线后由下一次刷新或请求自动重连，保持当前机器和阅读位置；读取最多自动重试一次，写操作不会重放。Linux 不再监听管理 HTTP 端口，也不需要管理员 API Key 或反向代理。

安装入口下载与 App 同版本的正式签名服务包。Ubuntu/Debian x86_64 自动补齐必要依赖，其他 Linux x86_64 须已有 systemd 和依赖。命令行部署仍可使用仓库外的连接参数：

```bash
bash scripts/deploy-cloud.sh SSH_HOST ./dist/agenthub-webd-linux-x86_64.tar.gz
```

升级须先部署服务，再安装 App。旧 HTTP/Nginx、目录、配置及桌面钥匙串迁移须先由 v2.1.0 完成；本版保留正常安装回退、健康检查、现用密钥和独立 Codex 发送队列。完整步骤见[云端部署手册](docs/cloud-deploy-runbook.md)。

## 发布资产

当前源码版本为 **2.1.3**；[查看 GitHub Releases](https://github.com/lich13/agenthub/releases)。每次正式发布包含八项资产：

- `AgentHub-<version>-darwin-arm64.dmg` 与 `.dmg.sha256`；
- `agenthub-darwin-arm64.tar.gz` 与 `.tar.gz.sig`（Tauri updater 归档与 Minisign 签名）；
- 仅包含 `darwin-aarch64` 平台映射的 `latest.json`；
- `agenthub-webd-linux-x86_64.tar.gz`、`.tar.gz.sha256` 与 `.tar.gz.sig`。

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
- [docs/cloud-deploy-runbook.md](docs/cloud-deploy-runbook.md)：Linux SSH 服务部署、升级与回滚手册。
- [docs/progress/MASTER.md](docs/progress/MASTER.md)：维护者版本演进与已验证证据。
