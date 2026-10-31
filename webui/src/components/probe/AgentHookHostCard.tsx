import { CheckCircle2, TerminalSquare, Trash2 } from "lucide-react";
import { type ReactNode, useEffect, useRef, useState } from "react";
import { AgentBrandIcon, type AgentBrandId } from "../common/AgentBrandIcon";
import { ConfirmDialog } from "../common/ConfirmDialog";
import { Metric } from "../common/Panel";
import { probeStateLabel } from "../../lib/domain/runtimeViewModel";
import type { ProbeSettingsDraft } from "../../lib/probeUi";
import { type HookProviderId, useClaudeHooks } from "../../lib/query/probe";
import type { ProbeStatus } from "../../types";

interface HookProviderMeta {
  agent: AgentBrandId;
  name: string;
  subtitle: string;
  eventsSummary: string;
  installConfirmTitle: string;
  installConfirmBody: string;
  removeConfirmTitle: string;
  removeConfirmBody: string;
}

const HOOK_PROVIDER_META: Record<HookProviderId, HookProviderMeta> = {
  claude: {
    agent: "claude",
    name: "Claude Code",
    subtitle: "Stop · Notification · StopFailure 通知 Hook 托管",
    eventsSummary: "Stop · Notification · StopFailure",
    installConfirmTitle: "写入 Claude Code 通知 Hook",
    installConfirmBody:
      "这会更新 Claude Code 配置文件中的 Stop、Notification 与 StopFailure hooks，并保留你已有的其他配置与第三方 hooks。",
    removeConfirmTitle: "移除 Claude Code 通知 Hook",
    removeConfirmBody:
      "这会从 Claude Code 配置文件中移除 NexusHub 托管的 Stop、Notification 与 StopFailure hooks，并保留其他第三方 hooks。",
  },
  grok: {
    agent: "grok",
    name: "Grok Build",
    subtitle: "Stop · Notification · StopFailure 通知 Hook 托管",
    eventsSummary: "Stop · Notification · StopFailure",
    installConfirmTitle: "写入 Grok 通知 Hook",
    installConfirmBody:
      "这会更新 Grok 配置文件中的 Stop、Notification 与 StopFailure hooks，并保留你已有的其他配置与第三方 hooks。",
    removeConfirmTitle: "移除 Grok 通知 Hook",
    removeConfirmBody:
      "这会从 Grok 配置文件中移除 NexusHub 托管的 Stop、Notification 与 StopFailure hooks，并保留其他第三方 hooks。",
  },
  antigravity: {
    agent: "antigravity",
    name: "Antigravity",
    subtitle: "Stop 生命周期通知 Hook 托管（自动返回 allow 放行决策）",
    eventsSummary: "Stop (decision: allow)",
    installConfirmTitle: "写入 Antigravity 通知 Hook",
    installConfirmBody:
      "这会更新 Antigravity 宿主配置文件中的 Stop 通知 Hook（执行后向宿主输出 allow 放行决策），并保留你已有的其他配置。",
    removeConfirmTitle: "移除 Antigravity 通知 Hook",
    removeConfirmBody:
      "这会从 Antigravity 宿主配置文件中移除 NexusHub 托管的 Stop 通知 Hook，并保留其他第三方配置。",
  },
};

export function AgentHookHostCardShell({
  agent,
  name,
  subtitle,
  configPath,
  children,
}: {
  agent: AgentBrandId;
  name: string;
  subtitle: string;
  configPath?: string | null;
  children: ReactNode;
}) {
  const displayPath = configPath?.trim() || "未返回路径";
  return (
    <div className="probe-card-stack probe-host-card" data-agent={agent}>
      <div className="probe-host-card-head">
        <span className="agent-brand-badge" data-agent={agent} aria-hidden="true">
          <AgentBrandIcon agent={agent} size={16} />
        </span>
        <div className="probe-host-card-copy">
          <strong>{name}</strong>
          <span>{subtitle}</span>
        </div>
        <code className="probe-host-path" title={displayPath}>
          {displayPath}
        </code>
      </div>
      {children}
    </div>
  );
}

export function ProbeHookCard({
  status,
  draft,
  busy,
  onInstall,
  onRemove,
}: {
  status?: ProbeStatus | null;
  draft?: ProbeSettingsDraft | null;
  setDraft?: (draft: ProbeSettingsDraft) => void;
  busy?: boolean;
  onInstall: () => void;
  onRemove?: () => void;
}) {
  const [confirmAction, setConfirmAction] = useState<"install" | "remove" | null>(null);
  const installButtonRef = useRef<HTMLButtonElement>(null);
  const removeButtonRef = useRef<HTMLButtonElement>(null);
  const hookStatus = status?.hook_status;
  const managed = hookStatus === "managed";
  const needsRepair = hookStatus === "stale" || hookStatus === "missing";
  const configured = managed || draft?.hooks.manage_stop_hook === true;
  const actualCommandCount = status?.actual_commands?.length ?? 0;
  const staleCommandCount = status?.stale_command_count ?? 0;
  const emptyGroupCount = status?.empty_group_count ?? 0;
  const canRemove =
    Boolean(onRemove) && (managed || actualCommandCount > 0 || staleCommandCount > 0);
  const actualSummary = `${actualCommandCount} 条${staleCommandCount > 0 ? ` · 旧 ${staleCommandCount}` : ""}${emptyGroupCount > 0 ? ` · 空 ${emptyGroupCount}` : ""}`;
  const configPath = status?.codex_hooks_path ?? "";

  return (
    <>
      <AgentHookHostCardShell
        agent="codex"
        name="Codex"
        subtitle="Stop · request_user_input 双通道通知 Hook 托管"
        configPath={configPath}
      >
        <div className="settings-meta-grid">
          <Metric label="Codex Hook" value={probeStateLabel(hookStatus)} tone={managed ? "success" : "warning"} />
          <Metric label="自动维护" value={configured ? "已开启" : "已关闭"} tone={configured ? "success" : "warning"} />
          <Metric label="实际命令" value={actualSummary} tone={needsRepair ? "warning" : "success"} />
          <Metric label="监听事件" value="Stop · request_user_input" />
        </div>
        <div className="button-row">
          <button
            ref={installButtonRef}
            className="secondary-button"
            disabled={busy}
            onClick={() => setConfirmAction("install")}
          >
            <CheckCircle2 size={17} />
            {needsRepair ? "修复 Hook" : managed ? "重新写入 Hook" : "写入通知 Hook"}
          </button>
          {onRemove && (
            <button
              ref={removeButtonRef}
              className="secondary-button"
              disabled={busy || !canRemove}
              onClick={() => setConfirmAction("remove")}
            >
              <Trash2 size={17} />
              移除 Hook
            </button>
          )}
        </div>
      </AgentHookHostCardShell>
      {confirmAction && (
        <ConfirmDialog
          labelledBy="codex-hooks-confirm"
          busy={Boolean(busy)}
          returnFocus={confirmAction === "install" ? installButtonRef : removeButtonRef}
          onCancel={() => setConfirmAction(null)}
        >
          <h3 id="codex-hooks-confirm">
            {confirmAction === "install" ? "写入 Codex 通知 Hook" : "移除 Codex 通知 Hook"}
          </h3>
          <p>
            {confirmAction === "install"
              ? "写入或更新 NexusHub 托管的 Codex 通知 Hook 并启用自动维护，保留其他第三方 Hook 配置。"
              : "移除 NexusHub 托管的 Codex 通知 Hook 并关闭自动修复，保留其他第三方 Hook 配置。"}
          </p>
          {configPath && <code className="delete-path">{configPath}</code>}
          <div className="button-row">
            <button
              className="secondary-button"
              disabled={busy}
              onClick={() => setConfirmAction(null)}
            >
              取消
            </button>
            <button
              className={confirmAction === "remove" ? "danger-button" : "primary-button"}
              disabled={busy}
              onClick={() => {
                const action = confirmAction;
                setConfirmAction(null);
                if (action === "install") {
                  onInstall();
                } else {
                  onRemove?.();
                }
              }}
            >
              {confirmAction === "install" ? "确认写入" : "确认移除"}
            </button>
          </div>
        </ConfirmDialog>
      )}
    </>
  );
}

export function AgentHookHostCard({
  provider = "claude",
}: {
  provider?: HookProviderId;
}) {
  const meta = HOOK_PROVIDER_META[provider];
  const [confirmAction, setConfirmAction] = useState<
    "install" | "remove" | null
  >(null);
  const installRef = useRef<HTMLButtonElement>(null);
  const removeRef = useRef<HTMLButtonElement>(null);
  const { local, revision, status: statusQuery, mutation: actionMutation } =
    useClaudeHooks(() => setConfirmAction(null), provider);
  useEffect(() => setConfirmAction(null), [revision]);

  if (!local) {
    return (
      <AgentHookHostCardShell
        agent={meta.agent}
        name={meta.name}
        subtitle={meta.subtitle}
        configPath="远端只读"
      >
        <div className="settings-meta-grid">
          <Metric label="Hook 状态" value="仅限本机" />
          <Metric label="宿主模式" value="远端只读轮询" />
        </div>
      </AgentHookHostCardShell>
    );
  }

  const status = statusQuery.data;
  const installed = status?.installed ?? false;
  const managedCount = status?.managedCount ?? 0;
  const configPath = status?.configPath ?? "";
  const busy = actionMutation.isPending;
  const statusText = statusQuery.isLoading
    ? "读取中"
    : statusQuery.error
      ? "不可用"
      : installed
        ? "已安装"
        : "未安装";
  const statusTone = statusQuery.error
    ? "danger"
    : installed
      ? "success"
      : "warning";

  const runConfirmed = (action: "install" | "remove") => {
    actionMutation.mutate(action);
  };

  return (
    <>
      <AgentHookHostCardShell
        agent={meta.agent}
        name={meta.name}
        subtitle={meta.subtitle}
        configPath={configPath}
      >
        {statusQuery.error && (
          <div role="alert" className="form-error">
            {statusQuery.error.message}
          </div>
        )}
        {actionMutation.error && (
          <div role="alert" className="form-error">
            {actionMutation.error.message}
          </div>
        )}
        <div className="settings-meta-grid">
          <Metric label="Hook 状态" value={statusText} tone={statusTone} />
          <Metric label="托管条目" value={`${managedCount} 条`} />
          <Metric label="监听事件" value={meta.eventsSummary} wide />
        </div>
        <div className="button-row">
          <button
            ref={installRef}
            className="secondary-button"
            disabled={busy || statusQuery.isLoading}
            onClick={() => setConfirmAction("install")}
          >
            <CheckCircle2 size={17} />
            {installed ? "重新写入 Hook" : "写入通知 Hook"}
          </button>
          <button
            ref={removeRef}
            className="secondary-button"
            disabled={busy || statusQuery.isLoading || !installed}
            onClick={() => setConfirmAction("remove")}
          >
            <Trash2 size={17} />
            移除 Hook
          </button>
        </div>
      </AgentHookHostCardShell>
      {confirmAction && (
        <ConfirmDialog
          labelledBy={`${provider}-hooks-confirm`}
          busy={busy}
          returnFocus={confirmAction === "install" ? installRef : removeRef}
          onCancel={() => setConfirmAction(null)}
        >
          <h3 id={`${provider}-hooks-confirm`}>
            {confirmAction === "install"
              ? meta.installConfirmTitle
              : meta.removeConfirmTitle}
          </h3>
          <p>
            {confirmAction === "install"
              ? meta.installConfirmBody
              : meta.removeConfirmBody}
          </p>
          {configPath && <code className="delete-path">{configPath}</code>}
          <div className="button-row">
            <button
              className="secondary-button"
              disabled={busy}
              onClick={() => setConfirmAction(null)}
            >
              取消
            </button>
            <button
              className={
                confirmAction === "remove" ? "danger-button" : "primary-button"
              }
              disabled={busy}
              onClick={() => runConfirmed(confirmAction)}
            >
              {confirmAction === "install" ? "确认写入" : "确认移除"}
            </button>
          </div>
        </ConfirmDialog>
      )}
    </>
  );
}

export function DshHostCard({
  sessionsPath,
}: {
  sessionsPath?: string | null;
}) {
  return (
    <AgentHookHostCardShell
      agent="dsh"
      name="DeepSeek Harness"
      subtitle="原生会话状态流直连监控（无需写入外部 Hook 配置）"
      configPath={sessionsPath}
    >
      <div className="settings-meta-grid">
        <Metric label="接入模式" value="原生直连" tone="success" />
        <Metric label="Hook 依赖" value="免安装" tone="success" />
        <Metric
          label="监听事件"
          value="完成 · 失败 · 待回复 · 授权确认"
          wide
        />
      </div>
      <div className="button-row">
        <span className="status-chip tone-success">
          <TerminalSquare size={15} /> 原生事件流直连 · 免安装 Hook
        </span>
      </div>
    </AgentHookHostCardShell>
  );
}
