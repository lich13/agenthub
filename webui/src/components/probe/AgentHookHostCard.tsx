import { CheckCircle2, Trash2 } from "lucide-react";
import { type ReactNode, type RefObject, useEffect, useRef, useState } from "react";
import { probeStateLabel } from "../../lib/domain/runtimeViewModel";
import type { ProbeSettingsDraft } from "../../lib/probeUi";
import { type HookProviderId, useClaudeHooks } from "../../lib/query/probe";
import type { ProbeStatus } from "../../types";
import { AgentBrandIcon, type AgentBrandId } from "../common/AgentBrandIcon";
import { ConfirmDialog } from "../common/ConfirmDialog";
import { Metric } from "../common/Panel";

const HOOK_PROVIDER_META: Record<
  HookProviderId,
  { agent: AgentBrandId; name: string; label: string }
> = {
  claude: {
    agent: "claude",
    name: "Claude Code",
    label: "Claude Code",
  },
  grok: {
    agent: "grok",
    name: "Grok Build",
    label: "Grok",
  },
  antigravity: {
    agent: "antigravity",
    name: "Antigravity",
    label: "Antigravity",
  },
};

export function AgentHookHostCardShell(props: {
  agent: AgentBrandId;
  name: string;
  configPath?: string | null;
  children: ReactNode;
}) {
  const displayPath = props.configPath?.trim() || "未返回路径";
  return (
    <div className="probe-card-stack probe-host-card" data-agent={props.agent}>
      <div className="probe-host-card-head">
        <span className="agent-brand-badge" data-agent={props.agent} aria-hidden="true">
          <AgentBrandIcon agent={props.agent} size={16} />
        </span>
        <div className="probe-host-card-copy">
          <strong>{props.name}</strong>
        </div>
        <code className="probe-host-path" title={displayPath}>
          {displayPath}
        </code>
      </div>
      {props.children}
    </div>
  );
}

function HookConfirmFooter(props: {
  action: "install" | "remove";
  busy: boolean;
  configPath: string;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  return (
    <>
      {props.configPath && <code className="delete-path">{props.configPath}</code>}
      <div className="button-row">
        <button className="secondary-button" disabled={props.busy} onClick={props.onCancel}>
          取消
        </button>
        <button
          className={props.action === "remove" ? "danger-button" : "primary-button"}
          disabled={props.busy}
          onClick={props.onConfirm}
        >
          {props.action === "install" ? "确认写入" : "确认移除"}
        </button>
      </div>
    </>
  );
}

function HookActionButtons(props: {
  installRef: RefObject<HTMLButtonElement>;
  removeRef: RefObject<HTMLButtonElement>;
  installLabel: string;
  installDisabled: boolean;
  removeDisabled: boolean;
  showRemove?: boolean;
  onSelect: (action: "install" | "remove") => void;
}) {
  return (
    <div className="button-row">
      <button
        ref={props.installRef}
        className="secondary-button"
        disabled={props.installDisabled}
        onClick={() => props.onSelect("install")}
      >
        <CheckCircle2 size={17} />
        {props.installLabel}
      </button>
      {(props.showRemove ?? true) && (
        <button
          ref={props.removeRef}
          className="secondary-button"
          disabled={props.removeDisabled}
          onClick={() => props.onSelect("remove")}
        >
          <Trash2 size={17} />
          移除 Hook
        </button>
      )}
    </div>
  );
}

export function ProbeHookCard(props: {
  status?: ProbeStatus | null;
  draft?: ProbeSettingsDraft | null;
  setDraft?: (draft: ProbeSettingsDraft) => void;
  busy?: boolean;
  onInstall: () => void;
  onRemove?: () => void;
}) {
  const [confirmAction, setConfirmAction] = useState<"install" | "remove" | null>(null);
  const installRef = useRef<HTMLButtonElement>(null);
  const removeRef = useRef<HTMLButtonElement>(null);
  const hookStatus = props.status?.hook_status;
  const managed = hookStatus === "managed";
  const needsRepair = hookStatus === "stale" || hookStatus === "missing";
  const configured = managed || props.draft?.hooks.manage_stop_hook === true;
  const actualCount = props.status?.actual_commands?.length ?? 0;
  const staleCount = props.status?.stale_command_count ?? 0;
  const emptyCount = props.status?.empty_group_count ?? 0;
  const canRemove = Boolean(props.onRemove) && (managed || actualCount > 0 || staleCount > 0);
  const actualSummary = `${actualCount} 条${staleCount > 0 ? ` · 旧 ${staleCount}` : ""}${emptyCount > 0 ? ` · 空 ${emptyCount}` : ""}`;
  const configPath = props.status?.codex_hooks_path ?? "";

  return (
    <>
      <AgentHookHostCardShell
        agent="codex"
        name="Codex"
        configPath={configPath}
      >
        <div className="settings-meta-grid">
          <Metric label="Codex Hook" value={probeStateLabel(hookStatus)} tone={managed ? "success" : "warning"} />
          <Metric label="自动维护" value={configured ? "已开启" : "已关闭"} tone={configured ? "success" : "warning"} />
          <Metric label="实际命令" value={actualSummary} tone={needsRepair ? "warning" : "success"} />
        </div>
        <HookActionButtons
          installRef={installRef}
          removeRef={removeRef}
          installLabel={needsRepair ? "修复 Hook" : managed ? "重新写入 Hook" : "写入通知 Hook"}
          installDisabled={Boolean(props.busy)}
          removeDisabled={Boolean(props.busy) || !canRemove}
          showRemove={Boolean(props.onRemove)}
          onSelect={setConfirmAction}
        />
      </AgentHookHostCardShell>
      {confirmAction && (
        <ConfirmDialog
          labelledBy="codex-hooks-confirm"
          busy={Boolean(props.busy)}
          returnFocus={confirmAction === "install" ? installRef : removeRef}
          onCancel={() => setConfirmAction(null)}
        >
          <h3 id="codex-hooks-confirm">
            {confirmAction === "install" ? "写入 Codex 通知 Hook" : "移除 Codex 通知 Hook"}
          </h3>
          <HookConfirmFooter
            action={confirmAction}
            busy={Boolean(props.busy)}
            configPath={configPath}
            onCancel={() => setConfirmAction(null)}
            onConfirm={() => {
              const action = confirmAction;
              setConfirmAction(null);
              if (action === "install") props.onInstall();
              else props.onRemove?.();
            }}
          />
        </ConfirmDialog>
      )}
    </>
  );
}

export function AgentHookHostCard({ provider = "claude" }: { provider?: HookProviderId }) {
  const meta = HOOK_PROVIDER_META[provider];
  const [confirmAction, setConfirmAction] = useState<"install" | "remove" | null>(null);
  const installRef = useRef<HTMLButtonElement>(null);
  const removeRef = useRef<HTMLButtonElement>(null);
  const { local, revision, status: statusQuery, mutation } = useClaudeHooks(
    () => setConfirmAction(null),
    provider,
  );
  useEffect(() => setConfirmAction(null), [revision]);

  if (!local) {
    return (
      <AgentHookHostCardShell
        agent={meta.agent}
        name={meta.name}
        configPath="远端只读"
      >
        <div className="settings-meta-grid">
          <Metric label="Hook 状态" value="仅限本机" />
          <Metric label="宿主模式" value="远端只读轮询" />
        </div>
      </AgentHookHostCardShell>
    );
  }

  const installed = statusQuery.data?.installed ?? false;
  const managedCount = statusQuery.data?.managedCount ?? 0;
  const configPath = statusQuery.data?.configPath ?? "";
  const busy = mutation.isPending;
  const statusText = statusQuery.isLoading
    ? "读取中"
    : statusQuery.error
      ? "不可用"
      : installed
        ? "已安装"
        : "未安装";
  const statusTone = statusQuery.error ? "danger" : installed ? "success" : "warning";
  const confirmId = `${provider}-hooks-confirm`;

  return (
    <>
      <AgentHookHostCardShell
        agent={meta.agent}
        name={meta.name}
        configPath={configPath}
      >
        {statusQuery.error && <div role="alert" className="form-error">{statusQuery.error.message}</div>}
        {mutation.error && <div role="alert" className="form-error">{mutation.error.message}</div>}
        <div className="settings-meta-grid">
          <Metric label="Hook 状态" value={statusText} tone={statusTone} />
          <Metric label="托管条目" value={`${managedCount} 条`} />
        </div>
        <HookActionButtons
          installRef={installRef}
          removeRef={removeRef}
          installLabel={installed ? "重新写入 Hook" : "写入通知 Hook"}
          installDisabled={busy || statusQuery.isLoading}
          removeDisabled={busy || statusQuery.isLoading || (!installed && managedCount === 0)}
          onSelect={setConfirmAction}
        />
      </AgentHookHostCardShell>
      {confirmAction && (
        <ConfirmDialog
          labelledBy={confirmId}
          busy={busy}
          returnFocus={confirmAction === "install" ? installRef : removeRef}
          onCancel={() => setConfirmAction(null)}
        >
          <h3 id={confirmId}>
            {confirmAction === "install"
              ? `写入 ${meta.label} 通知 Hook`
              : `移除 ${meta.label} 通知 Hook`}
          </h3>
          <HookConfirmFooter
            action={confirmAction}
            busy={busy}
            configPath={configPath}
            onCancel={() => setConfirmAction(null)}
            onConfirm={() => mutation.mutate(confirmAction)}
          />
        </ConfirmDialog>
      )}
    </>
  );
}

export function DshHostCard({ sessionsPath }: { sessionsPath?: string | null }) {
  return (
    <AgentHookHostCardShell
      agent="dsh"
      name="DeepSeek Harness"
      configPath={sessionsPath}
    >
      <div className="settings-meta-grid">
        <Metric label="接入模式" value="原生直连" tone="success" />
        <Metric label="Hook 依赖" value="免安装" tone="success" />
      </div>
    </AgentHookHostCardShell>
  );
}
