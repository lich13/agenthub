import {
  Activity,
  Cloud,
  GitFork,
  RefreshCw,
  SlidersHorizontal,
  TerminalSquare,
  TriangleAlert,
} from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { AgentHookHostCard, DshHostCard, ProbeHookCard } from "./AgentHookHostCard";
import { Panel } from "../common/Panel";
import { JobList } from "../jobs/JobList";
import {
  buildProbeSettingsDraft,
  buildProbeSettingsPayload,
  probeSettingsValidation,
  PROBE_NAV_LABEL,
  type ProbeSettingsDraft,
} from "../../lib/probeUi";
import { useProbeActions, useProbeQueries } from "../../lib/query/probe";
import { isTerminalJob, useStartedJob } from "../../lib/query/jobs";
import type { RuntimeCapabilityMatrix } from "../../lib/query/system";
import {
  isProbeSettings,
  probeJobActionLabel,
  probeSettingsAfterBarkSave,
  probeWorkspaceView,
} from "../../lib/domain/runtimeViewModel";
import {
  ProbeBarkCard,
  ProbeEventsCard,
  ProbeLiveOverviewCard,
  ProbeRuntimeSettingsCard,
  type ProbeSaveStatus,
} from "./ProbeCards";

export function ProbeWorkspace({ capabilities }: { capabilities: RuntimeCapabilityMatrix }) {
  const [historyOpen, setHistoryOpen] = useState(false);
  const { status, settings, events, jobs } = useProbeQueries({ section: "settings", historyOpen });
  const [draft, setDraft] = useState<ProbeSettingsDraft | null>(null);
  const submittedDraft = useRef<ProbeSettingsDraft | null>(null);
  const saveInFlight = useRef(false);
  const [saveStatus, setSaveStatus] = useState<ProbeSaveStatus>(null);
  const [actionStatus, setActionStatus] = useState<ProbeSaveStatus>(null);
  const data = status.data?.data;
  const available = status.data?.available ?? false;
  const currentSettings = settings.data?.data;
  const settingsErrors = draft ? probeSettingsValidation(draft) : [];
  const recentEvents = events.data?.data?.events ?? [];
  const probeView = probeWorkspaceView({
    data,
    available,
    currentSettings,
    recentEventCount: recentEvents.length,
    jobs: jobs.data,
    loading: status.isLoading,
    fetching: status.isFetching,
    error: status.isError,
    draftDeviceKeyConfigured: draft?.notifications.device_key_configured,
  });
  const probeActions = useProbeActions({
    capabilities,
    savePayload: submittedDeviceKey => {
      if (!draft) throw new Error("探针设置尚未载入");
      const errors = probeSettingsValidation(draft);
      if (errors.length) throw new Error(errors[0]);
      submittedDraft.current = draft;
      return buildProbeSettingsPayload(draft, currentSettings, submittedDeviceKey);
    },
    onJobSuccess: action => {
      setActionStatus(
        action === "bark-test" ? null : { tone: "success", message: `${probeJobActionLabel(action)} 已开始` },
      );
      if (action === "hooks-remove") {
        setDraft(current =>
          current ? { ...current, hooks: { ...current.hooks, manage_stop_hook: false } } : current,
        );
      } else if (action === "hooks-install") {
        setDraft(current =>
          current ? { ...current, hooks: { ...current.hooks, manage_stop_hook: true } } : current,
        );
      }
    },
    onJobError: (err, action) => {
      setActionStatus({ tone: "error", message: `${probeJobActionLabel(action)} 失败: ${err.message}` });
    },
    onSaveSuccess: (saved, submittedDeviceKey) => {
      const nextSettings = probeSettingsAfterBarkSave(
        saved,
        submittedDeviceKey ?? draft?.notifications.device_key,
      );
      if (!isProbeSettings(nextSettings)) {
        setSaveStatus({ tone: "error", message: "保存响应结构异常，已保留当前输入" });
        return;
      }
      setSaveStatus({ tone: "success", message: "设置已保存" });
      const savedDraft = submittedDraft.current;
      setDraft(current => (current === savedDraft ? buildProbeSettingsDraft(nextSettings) : current));
    },
    onSaveError: err => {
      setSaveStatus({ tone: "error", message: err.message });
    },
  });
  const probeJobMutation = probeActions.job;
  const startedJob = useStartedJob(probeJobMutation.data?.job_id);
  const jobRunning = Boolean(probeJobMutation.data?.job_id && !isTerminalJob(startedJob.data?.status));
  const jobBusy = probeJobMutation.isPending || jobRunning;
  const saveMutation = probeActions.save;
  const saveSettings = (deviceKey?: string) => {
    if (saveInFlight.current) return;
    saveInFlight.current = true;
    setSaveStatus(null);
    saveMutation.mutate(deviceKey, {
      onSettled: () => {
        saveInFlight.current = false;
      },
    });
  };
  const pendingProbeAction = jobBusy ? probeJobMutation.variables : null;
  const hostLabel = currentSettings?.codex?.host_label;

  useEffect(() => {
    if (!currentSettings || draft) return;
    setDraft(buildProbeSettingsDraft(currentSettings));
  }, [currentSettings, draft]);

  const hooksHostMatrix = (
    <div className="probe-hooks-grid ws-card-grid">
      <Panel title="Codex Host" icon={<GitFork size={18} />}>
        <ProbeHookCard
          status={data}
          draft={draft}
          setDraft={setDraft}
          busy={jobBusy}
          onInstall={() => probeJobMutation.mutate("hooks-install")}
          onRemove={() => probeJobMutation.mutate("hooks-remove")}
        />
      </Panel>
      <Panel title="Claude Code Host" icon={<GitFork size={18} />}>
        <AgentHookHostCard provider="claude" />
      </Panel>
      <Panel title="Grok Build Host" icon={<GitFork size={18} />}>
        <AgentHookHostCard provider="grok" />
      </Panel>
      {capabilities.antigravityNotifications && (
        <Panel title="Antigravity Host" icon={<GitFork size={18} />}>
          <AgentHookHostCard provider="antigravity" />
        </Panel>
      )}
      {capabilities.dshNotifications && (
        <Panel title="DeepSeek Harness Host" icon={<GitFork size={18} />}>
          <DshHostCard sessionsPath={data?.dsh_sessions_path} />
        </Panel>
      )}
    </div>
  );

  return (
    <div className="probe-layout" data-host-label={hostLabel ?? undefined}>
      <header className="probe-header">
        <h1>{PROBE_NAV_LABEL}</h1>
        <div className="button-row">
          <button className="icon-button" title="刷新 Probe" onClick={probeActions.refresh}>
            <RefreshCw size={17} />
          </button>
        </div>
      </header>

      <Panel title="通知配置与快捷开关" icon={<SlidersHorizontal size={18} />} className="wide-panel">
        {settings.error && <div role="alert" className="form-error">{settings.error.message}</div>}
        {actionStatus && (
          <div className={actionStatus.tone === "success" ? "form-success" : "form-error"}>
            {actionStatus.message}
          </div>
        )}
        {startedJob.error && <div role="alert" className="form-error">{startedJob.error.message}</div>}
        {startedJob.data && <JobList jobs={[startedJob.data]} capabilities={capabilities} />}
        {draft ? (
          <ProbeRuntimeSettingsCard
            draft={draft}
            setDraft={setDraft}
            errors={settingsErrors}
            saveStatus={saveStatus}
            saving={saveMutation.isPending}
            status={data}
            settings={currentSettings}
            capabilities={capabilities}
            onSave={() => saveSettings()}
          />
        ) : (
          <div className="muted-row">{settings.isLoading ? "正在读取设置" : "设置不可用"}</div>
        )}
      </Panel>

      <Panel title="Bark" icon={<Cloud size={18} />} className="wide-panel">
        {draft && (
          <ProbeBarkCard
            draft={draft}
            setDraft={setDraft}
            configuredDeviceKey={probeView.barkConfigured}
            saveStatus={saveStatus}
            saving={saveMutation.isPending}
            testing={pendingProbeAction === "bark-test"}
            testStatus={
              probeJobMutation.variables === "bark-test"
                ? jobBusy
                  ? "正在发送，遇到临时故障将自动重试…"
                  : startedJob.data?.status === "succeeded"
                    ? "Bark 已受理"
                    : startedJob.data?.status === "failed"
                      ? "发送失败，请查看执行记录"
                      : probeJobMutation.error?.message ?? null
                : null
            }
            onSave={saveSettings}
            onTest={() => probeJobMutation.mutate("bark-test")}
          />
        )}
      </Panel>

      {hooksHostMatrix}

      <Panel title="实时态势" icon={<Activity size={18} />} className="wide-panel">
        <ProbeLiveOverviewCard
          status={data}
          currentSettings={currentSettings}
          serviceText={probeView.serviceText}
        />
      </Panel>

      <Panel title="最近事件" icon={<TerminalSquare size={18} />} className="wide-panel">
        {events.error && <div role="alert" className="form-error">{events.error.message}</div>}
        <ProbeEventsCard
          events={recentEvents}
          available={events.data?.available ?? false}
          loading={events.isLoading}
        />
      </Panel>

      <details
        className="execution-history"
        open={historyOpen}
        onToggle={event => setHistoryOpen(event.currentTarget.open)}
      >
        <summary>执行记录</summary>
        <JobList jobs={probeView.probeJobs} capabilities={capabilities} />
      </details>

      {probeView.availability.tone === "danger" && (
        <Panel title="端点" icon={<TriangleAlert size={18} />} className="wide-panel">
          <div className="muted-row">探针端点不可用</div>
        </Panel>
      )}
    </div>
  );
}
