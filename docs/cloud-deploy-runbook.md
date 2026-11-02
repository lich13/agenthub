# Linux API 部署手册

AgentHub 的 Linux 版本只提供管理 API 和健康检查。它不提供网页、登录页或静态资源，也不会安装 Claude Code 或 Linux 桌面 App。

部署时从仓库外显式提供 SSH 主机、HTTPS 域名和归档路径。域名、主机、API Key、用户会话和配置不写入 Git。

默认 Linux 配置、Codex 包装器及 systemd 写入白名单假设宿主普通用户为 `ubuntu`，并保留 root 与 ubuntu 的既有会话根。其他用户布局沿用 Provider 环境变量与 Codex home 配置；写入范围限制在 Provider 数据根，不开放整个用户 home。

## 服务与隔离

- 程序：`/usr/local/bin/agenthub-webd`
- systemd 单元：`/etc/systemd/system/agenthub-webd.service`
- 配置：`/etc/agenthub-webd/config.toml` 和 `/etc/agenthub-webd/env`
- 数据：`/var/lib/agenthub-webd/`
- 日志：`/var/log/agenthub-webd/`
- 监听：仅回环地址；反向代理只转发 `api/rpc/` 和 `healthz`，其他路径返回 404

保留 `ProtectSystem=full`、`ProtectHome=read-only`、`NoNewPrivileges=true` 和 `PrivateTmp=true`。Grok／Claude／Antigravity／DSH 数据根使用可选的 `ReadWritePaths`，覆盖主会话、旁路、共享索引及原子替换所需的父目录。其凭据和配置文件使用可选的 `ReadOnlyPaths`；只开放 sessions／projects 或单个索引文件不能支持完整删除。自定义根从 `GROK_HOME`、`CLAUDE_CONFIG_DIR`、`ANTIGRAVITY_HOME`、`DSH_HOME`、`CODEX_HOME` 和配置中的 `codex.home` 生成；新增目录或修改根配置后，重新运行安装器生成 drop-in、执行 `daemon-reload` 并重启服务。

Grok 原生 leader 使用 `/var/lib/agenthub-webd/grok-leader.sock`。凭据和 Provider 配置保持隔离，原生管理操作需要在服务命名空间中可见的工作目录。

## 安装与升级

发布页提供八项资产：macOS ARM64 DMG、校验文件、updater 压缩包及签名、`latest.json`，以及 Linux x86_64 `webd` 服务包、校验文件和分离签名。`latest.json` 的平台键为 `darwin-aarch64`，文件名中的 `darwin-arm64` 表示同一架构；Linux 服务包不能用于桌面 updater。

先通过可信系统包管理器在部署客户端与服务器安装 Minisign。Linux 归档旁必须保留同名 .sha256 与 .sig；验签公钥来自已核验的仓库或预装文件，不能从待验证归档获取信任根。1.2.22 及更早 updater 不会验签，首次升级必须使用可信客户端的 deploy-cloud.sh 验签后再安装新验签器与公钥。缺少工具、公钥或签名时停止，不降级为仅 SHA-256。

使用明确的外部参数部署：

```bash
AGENTHUB_DOMAIN=panel.example.invalid \
  bash scripts/deploy-cloud.sh SSH_HOST ./dist/agenthub-webd-linux-x86_64.tar.gz
```

升级前先核对校验文件和当前服务状态。远程 API 协议为 2，必须先升级服务，再安装新版 App。协议不兼容时会提示更新并拒绝远程操作，本机仍可使用。升级会清除 AgentHub 自有的 Pi 提供方、流、投递和 Pi 专属事件，以及三项 notify_pi 设置和 Pi 路径变量；其余业务数据库内容、Bark 加密材料、通知游标、任务记录和审计数据保留；原生 Provider 会话和 Codex 数据库不迁移、不改写。

## 管理员 API Key

在服务器生成 Key：

```bash
sudo /usr/local/bin/agenthub-webd admin key-generate --output /absolute/private/api-key
```

输出文件必须不存在，权限为 `0600`。将 Key 输入 App 的“设置 → 远程连接”后立即删除临时文件。服务器只保存摘要，不保存原文。

轮换或撤销：

```bash
sudo /usr/local/bin/agenthub-webd admin key-rotate --output /absolute/private/new-api-key
sudo /usr/local/bin/agenthub-webd admin key-revoke
```

轮换会立即使旧 Key 失效；撤销或未配置 Key 会拒绝业务 RPC。Key 不得出现在 URL、命令参数、日志、浏览器请求或仓库文件中。旧网页 Cookie、登录入口和 Turnstile 配置不再生效。

## 更新与回滚

App 的“更新与维护”页面在本机目标显示“本机 App 更新”，在远程目标显示“远程服务更新”。两者使用各自的更新接口和状态缓存；切换机器不会复用另一台机器的结果。

服务器更新在受保护的 API 服务外启动短时 root unit，写入完成并记录成功后再安排服务重启。在线更新验证 SHA-256 与分离签名，核对签名版本、平台及包布局；部署校验本地与远端传输内容，安全解包并原子替换程序；普通安装失败恢复本轮替换文件，不回滚业务数据库或原生会话。异步重启调度成功不等于新进程已启动，仍需检查服务与健康接口。安全机制见[构建、CI 与发布](ARCHITECTURE.md#构建ci-与发布)。

现有配置只允许回环 listen。更新任务仅接受固定包装器及完整匹配的历史默认命令，其他自定义 shell 字符串将被拒绝。Bark HTTPS 私网或无效目标不能继续投递，也不会替换成默认服务；HTTP 回环仍可用于隔离测试。具体地址边界见[架构说明](ARCHITECTURE.md#远程连接与认证)。

## 部署后检查

```bash
sudo /usr/local/bin/agenthub-webd --version
sudo systemctl is-active agenthub-webd
sudo systemctl show agenthub-webd -p MainPID -p ProtectHome -p ProtectSystem -p ReadWritePaths -p ReadOnlyPaths
curl -fsS http://127.0.0.1:15742/healthz
```

预期结果：服务为 `active`，健康接口返回成功；未认证业务请求返回 `401`，旧页面和静态资源返回 `404`。

在正式 App 中验证远程连接、会话读取、搜索、附件按需读取、路径复制、Plan 本地保存和用户指令时间线短线。没有原生 Claude 会话的机器显示空状态；空会话仍可读取并进入删除定位流程。

## 原生数据边界

Claude Code 从 CLAUDE_CONFIG_DIR/projects 或用户配置目录读取已有 JSONL。改名保留身份、版本和占用判断，删除按项目相对路径及文件名定位，覆盖专属目录、file-history 等旁路和共享索引对应记录。读取失败、活动登记或未知版本不阻止删除。Grok 和 Codex 同样只操作当前机器的原生数据根。

Codex 删除直接处理确认的目标和原生后代，不依赖本机原生 helper。覆盖普通／归档 rollout、压缩副本、旁路、状态和分页历史数据库及名称索引；外部引用不阻止执行，也不扩大目标。文件系统或数据库失败逐项报告，其他目标继续。API 协议保持 2，先升级 API 再安装 App。

Claude hooks 仅由本机 macOS 设置页明确安装，发布和安装 App 不自动启用。远端继续轮询，不新增 hooks 配置写权限。Claude 有效旧完成可跨回合补发；Bark 新事件使用单条摘要，旧队列保留内容和分段确认。默认开关、启用基线、成功去重、凭据及 Codex 逐问题提醒保持。

Claude 过程折叠仅在本机详情显示：同一原生回合的思考、明确的过程说明和工具活动按原顺序收起，最终答复单独显示。旧服务缺少阶段字段时采用保守展示；过程折叠状态按机器、会话和回合身份保存在内存中，不写入会话或远端服务。

关闭桌面 App 后两台机器继续独立监测；停止远程服务会同时停止该服务内的远程监测。清理只针对本次部署产生的暂存文件；不要删除用户会话、凭据、Keychain 项或与 AgentHub 无关的服务文件。

## 上下文与时间线兼容

1.2.21 的可选 contextBlocks、timeline 和较新页游标仍使用 API 协议 2；旧客户端忽略新增字段，新客户端连接旧服务时本地整理原始包装并保留逐页加载。先升级 API 才能在远程使用完整时间线索引和有界跳转。升级不改写原生记录，也不新增客户端文件路径权限。

`codex_desktop` 能力与九个协作命令只在 macOS Tauri 提供，不注册到 Linux RPC，诊断入口也不远程开放。服务器可建立 AgentHub 自有 schema 2 发送记录表，但远程 API 不调度桌面对话；升级不重复 schema 1 的退役清理。本机加密发送队列使用独立 Keychain 项，不迁移到服务器。正式发布先升级 API、再安装 App。

本机桌面契约包括可选引用区间、会话审批与只读审批 review 字段，附件接收命令只接受会话、接收区和 handle，仍使用协议 2；Linux 不开放这些写入入口。旧加密队列正文保持兼容，不重发结果不明项。普通文件仍保留兼容编码，文件 cancel 与未来不兼容协议未核实；粘贴图片和加密归属索引只存于本机私有目录，覆盖安装和清理必须保留；不迁移到远程服务。详见[桌面协作证据与限制](ARCHITECTURE.md#证据与未核实项)。

1.3.4 已在本机修复候选完成[附件与发送验收](https://github.com/lich13/agenthub/blob/2b443fb412670e436bdb919affeef405f4efe200/docs/progress/MASTER.md#候选与用户验收)。正式发布要求对应提交三项 CI 通过，安装复用发布包，先升级远程 API，再原地替换原 bundle 身份的 macOS App，不保留旧版 App 副本。安装后检查版本、健康与启动，保留 Keychain、私有附件和原生会话。可选粘贴结果与加密文件身份摘要仅服务本机附件流程，Linux allowlist 和协议 2 不变。owner 变化只恢复阅读，必须在现有连接入口确认写入。

## 升级验收

部署或安装时只核对对应提交、资产校验、服务版本、健康状态和 App 启动。以下功能、交互与真实入口项目由用户验收；助手仅在明确要求或定位已证实的硬性故障时执行必要部分。未执行项标记“待用户验收”，不把 fixture 或服务启动视为功能通过。

删除与空会话的验收入口：打开仅元数据的 Claude、仅系统上下文的 Grok 会话，应显示“暂无对话”且删除入口可用。确认删除后核对列表、主文件、专属旁路及对应登记消失。在“更新与维护”确认隐藏扫描分页、停止和重新扫描；加载完整目标后再确认删除。真实删除由用户执行。

- 在包含环境、AGENTS.md 和附件包装的主线程及子线程中，默认应只看到附件、真实请求及一行“上下文”；展开可读原文。搜索真实请求或 AGENTS.md 应定位并展开，自动环境字段不应产生消息命中。
- 打开长线程并悬停、点击最早时间线刻度，检查窄预览、较早/较新分页和返回最新；大量刻度可用滚动及 Home/End 到达。检查机器切换后迟到结果不会出现在新线程。
- 在正式 App 的本机与远程目标验证列表、设置版本、详情、搜索、按需附件、Plan 本地保存、路径与用户指令时间线。网络或钥匙串失败必须就地显示，不能静默切回本机；HTTPS 成功不能替代正式 App 验收。
- 用长 Codex、Claude、Grok 和子智能体样本检查初始最新定位、连续上滚、加载中继续滚动、工具展开和机器切换；轮询不能丢失历史或重置父线程位置。父线程不追加时仍检查直属子智能体的当前状态。
- 三种 Provider 按 100MiB、300MiB 字节阈值分色；在仓库外独立只读核对实际字节与统计范围。大小统计无需开放工作目录或附件根。旧服务缺字段显示 —，部分结果保留 ≥，不能冒充完整大小。
- `system.capabilities` 保持协议 2，子智能体能力使用 `thread_subagents`。详情及附件只接受原生身份、主线程上下文和有界游标，不能接受任意路径。缺少真实样本时只记录空状态、接口或 fixture 覆盖。
- 用隔离数据检查改名、归档、删除和混合清理；改名与归档保留指纹规则，删除和清理只处理确认的目标集合。保留 API Key、Keychain、Bark、通知状态和 systemd 隔离；不使用用户会话进行破坏性验收。
- 本机 Codex 新建专用测试线程或打开已有主线程，在原生桌面打开后连接。输入 $ / @，用上下键和 Enter/Tab 选择引用，拖入文件或粘贴图片后发送，检查桌面进度和附件、协作状态行与可展开的审批 diff；修改占位文字或移除标签应撤销引用。目录超时可重试，模型与 Plan、文件、排队、补充和停止保持可用。命令/文件请求仅在支持时显示“允许本次会话”；任一侧处理审批或问答后，另一侧待处理项应消失。断线重连后暂停项需明确继续，结果不明不自动重发；隐藏或切换线程不得恢复旧请求。切换远程、归档或子线程时不应出现协作输入区。
- 从本机与远程分别发起新的“AgentHub 推送测试”，等待真实作业终态，并核对设备实收。HTTP 与 Bark JSON 同时成功仅表示服务端受理。关闭 App 后 monitor 应继续运行且无额外监听端口。

钥匙串读取后台串行，15 秒等待超时仍保护已开始的原生操作；保存与移除连接须完成事务或回滚。若系统要求授权，只在系统对话框处理。

## 迁移与通知保护

迁移只清理 AgentHub 自有的旧 web、Goal、Pi 与 Gotify 配置、Token、队列和专属记录；保留不可执行的兼容 tombstone。SQLite 遗留页与 WAL 清理可在中断后继续。不得删除原生 Provider 程序、会话或其他应用数据库。

Bark 保留成功记录、游标和启用基线，不补发历史失败或未知事件；结果未知仍可能重复提醒。重试参数见[通知与监测](ARCHITECTURE.md#通知与监测)。故障与重启验证使用隔离 HTTP 服务，不能破坏生产网络。

monitor 日志目标使用 `agenthub_webd`，只输出安全分类、状态码、耗时、次数和匿名标识。外部退休服务的卸载属于独立运维操作，需要明确归属；不修改共用 Nginx、TLS 或其他服务。

验收结束关闭本轮预览和转发，仅清理确认归属的下载、测试和部署暂存；保留正式服务、用户数据、连接、凭据与共享缓存。性能收益与实际回收空间分别计量。
