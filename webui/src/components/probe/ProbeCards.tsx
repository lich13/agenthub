import { CheckCircle2, Cloud } from "lucide-react";
import { useRef } from "react";
import {
  codexHomeStatusValue,
  logBytesDraftToMb,
  mbDraftToLogBytes,
  probeDiscoveryWarningsText,
  probeLiveSessionsByBucket,
  probeStateLabel,
} from "../../lib/domain/runtimeViewModel";
import {
  PROBE_NOTIFICATION_PLATFORMS,
  applyPlatformNotificationToggle,
  isPlatformNotificationEnabled,
  probeEventCard,
  probeNumberInputDraftValue,
  type ProbeSettingsDraft,
} from "../../lib/probeUi";
import type { RuntimeCapabilityMatrix } from "../../lib/query/system";
import type { ProbeEvent, ProbeLiveSession, ProbeSettings, ProbeStatus } from "../../types";
import { AgentBrandIcon, type AgentBrandId } from "../common/AgentBrandIcon";
import { Metric } from "../common/Panel";
export { ProbeHookCard } from "./AgentHookHostCard";

export type ProbeSaveStatus = { tone: "success" | "error"; message: string } | null;

const PROVIDER_META: Record<string, { brand: AgentBrandId; label: string }> = {
  claude_code: { brand: "claude", label: "Claude Code" },
  claude: { brand: "claude", label: "Claude Code" },
  grok: { brand: "grok", label: "Grok Build" },
  antigravity: { brand: "antigravity", label: "Antigravity" },
  dsh: { brand: "dsh", label: "DeepSeek Harness" },
};

export function ProbeLiveOverviewCard({
  status,
  currentSettings,
  serviceText,
}: {
  status?: ProbeStatus | null;
  currentSettings?: ProbeSettings | null;
  serviceText: string;
}) {
  const buckets = probeLiveSessionsByBucket(status);
  const hostLabel = status?.host_label ?? currentSettings?.codex?.host_label ?? "本机";
  const allHighlighted: { item: ProbeLiveSession; badge: string; tone: string }[] = [
    ...buckets.replyNeeded.map(item => ({ item, badge: "待回复", tone: "warning" })),
    ...buckets.recoverable.map(item => ({ item, badge: "异常", tone: "danger" })),
    ...buckets.running.map(item => ({ item, badge: "运行中", tone: "info" })),
  ];

  return (
    <div className="probe-card-stack probe-live-overview">
      <div className="probe-live-summary-head">
        <div>
          <strong>全平台实时态势</strong>
          <span>
            {serviceText} · {hostLabel}
          </span>
        </div>
        <div className="probe-live-kpis">
          <span className="status-chip tone-info">运行中 {buckets.running.length}</span>
          <span className="status-chip tone-warning">待回复 {buckets.replyNeeded.length}</span>
          <span className="status-chip tone-danger">异常 {buckets.recoverable.length}</span>
        </div>
      </div>
      {allHighlighted.length > 0 ? (
        <div className="preview-list compact">
          {allHighlighted.slice(0, 12).map(({ item, badge, tone }) => {
            const meta = PROVIDER_META[item.provider] ?? { brand: "codex" as const, label: "Codex" };
            return (
              <article
                className="preview-item probe-live-session-row"
                key={`${item.provider}:${item.session_key}:${item.bucket}`}
              >
                <div className="probe-live-session-main">
                  <span className="agent-brand-badge" data-agent={meta.brand} aria-hidden="true">
                    <AgentBrandIcon agent={meta.brand} size={15} />
                  </span>
                  <div className="probe-live-session-copy">
                    <strong>{item.title || item.id}</strong>
                    <span>
                      {meta.label}
                      {item.cwd ? ` · ${item.cwd}` : ""}
                    </span>
                  </div>
                </div>
                <span className={`status-chip tone-${tone}`}>{badge}</span>
              </article>
            );
          })}
        </div>
      ) : (
        <div className="muted-row">当前所有已启用平台均无运行中、待回复或异常会话</div>
      )}
    </div>
  );
}

export function ProbeBarkCard(props: {
  draft: ProbeSettingsDraft;
  setDraft: (draft: ProbeSettingsDraft) => void;
  configuredDeviceKey: boolean;
  saveStatus: ProbeSaveStatus;
  saving: boolean;
  testing: boolean;
  testStatus: string | null;
  onSave: (deviceKey?: string) => void;
  onTest: () => void;
}) {
  const { draft, setDraft, configuredDeviceKey, saveStatus, saving, testing, testStatus } = props;
  const deviceKeyInputRef = useRef<HTMLInputElement>(null);
  const setNotifications = (patch: Partial<ProbeSettingsDraft["notifications"]>) =>
    setDraft({ ...draft, notifications: { ...draft.notifications, ...patch } });
  return (
    <fieldset className="probe-card-stack" disabled={saving}>
      <div className="settings-meta-grid">
        <Metric
          label="通道状态"
          value={draft.notifications.enabled ? "已启用" : "已停用"}
          tone={draft.notifications.enabled ? "success" : "warning"}
        />
        <Metric
          label="密钥状态"
          value={configuredDeviceKey ? "已配置" : "未配置"}
          tone={configuredDeviceKey ? "success" : "warning"}
        />
      </div>
      <label className="toggle-row">
        <span>启用 Bark</span>
        <input
          type="checkbox"
          checked={draft.notifications.enabled}
          onChange={event => setNotifications({ enabled: event.target.checked })}
        />
      </label>
      <div className="form-grid">
        <label className="field-label">
          Bark 服务 URL
          <input
            value={draft.notifications.server_url}
            placeholder="https://api.day.app"
            onChange={event => setNotifications({ server_url: event.target.value })}
          />
        </label>
        <label className="field-label">
          Device Key
          <input
            ref={deviceKeyInputRef}
            type="password"
            value={draft.notifications.device_key}
            placeholder={configuredDeviceKey ? "已配置，留空保持不变" : "粘贴 Bark Device Key"}
            onChange={event => setNotifications({ device_key: event.target.value })}
          />
        </label>
      </div>
      <div className="button-row">
        <button
          className="primary-button"
          disabled={saving}
          onClick={() => props.onSave(deviceKeyInputRef.current?.value ?? draft.notifications.device_key)}
        >
          <CheckCircle2 size={17} />保存
        </button>
        <button
          className="secondary-button"
          disabled={!configuredDeviceKey || testing}
          onClick={props.onTest}
        >
          <Cloud size={17} />测试推送
        </button>
      </div>
      {testStatus && <div role="status" className="muted-row">{testStatus}</div>}
      {saveStatus && (
        <div className={saveStatus.tone === "success" ? "form-success" : "form-error"}>
          {saveStatus.message}
        </div>
      )}
    </fieldset>
  );
}

export function ProbeRuntimeSettingsCard(props: {
  draft: ProbeSettingsDraft;
  setDraft: (draft: ProbeSettingsDraft) => void;
  errors: string[];
  saveStatus: ProbeSaveStatus;
  saving: boolean;
  status?: ProbeStatus;
  settings?: ProbeSettings;
  capabilities: RuntimeCapabilityMatrix;
  onSave: () => void;
}) {
  const { draft, setDraft, errors, saveStatus, saving, status, settings, capabilities, onSave } = props;
  const setCodex = (patch: Partial<ProbeSettingsDraft["codex"]>) =>
    setDraft({ ...draft, codex: { ...draft.codex, ...patch } });
  const setProbe = (patch: Partial<ProbeSettingsDraft["probe"]>) =>
    setDraft({ ...draft, probe: { ...draft.probe, ...patch } });
  const setObservability = (patch: Partial<ProbeSettingsDraft["observability"]>) =>
    setDraft({ ...draft, observability: { ...draft.observability, ...patch } });

  const visiblePlatforms = PROBE_NOTIFICATION_PLATFORMS.filter(platform => {
    if (platform.id === "antigravity") return Boolean(capabilities.antigravityNotifications);
    if (platform.id === "dsh") return Boolean(capabilities.dshNotifications);
    return true;
  });
  const errorMonitorState =
    status?.error_monitor_status ?? (draft.probe.error_monitor.enabled ? "enabled" : "disabled");
  const numericFields: {
    label: string;
    min: number;
    max: number;
    value: string | number;
    onChange: (raw: string) => void;
  }[] = [
    {
      label: "轮询秒数",
      min: 5,
      max: 3600,
      value: draft.probe.poll_seconds,
      onChange: raw => setProbe({ poll_seconds: probeNumberInputDraftValue(raw) }),
    },
    {
      label: "最近事件数",
      min: 1,
      max: 500,
      value: draft.probe.recent_limit,
      onChange: raw => setProbe({ recent_limit: probeNumberInputDraftValue(raw) }),
    },
    {
      label: "通知事件保留天数",
      min: 1,
      max: 3650,
      value: draft.observability.event_retention_days,
      onChange: raw => setObservability({ event_retention_days: probeNumberInputDraftValue(raw) }),
    },
    {
      label: "Hook 事件行数",
      min: 1,
      max: 5000,
      value: draft.observability.hook_event_max_lines,
      onChange: raw => setObservability({ hook_event_max_lines: probeNumberInputDraftValue(raw) }),
    },
    {
      label: "冷却行数",
      min: 1,
      max: 5000,
      value: draft.observability.hook_cooldown_max_lines,
      onChange: raw => setObservability({ hook_cooldown_max_lines: probeNumberInputDraftValue(raw) }),
    },
    {
      label: "日志上限 MB",
      min: 1,
      max: 10,
      value: logBytesDraftToMb(draft.observability.log_max_bytes),
      onChange: raw => setObservability({ log_max_bytes: mbDraftToLogBytes(raw) }),
    },
  ];

  return (
    <fieldset className="probe-card-stack" disabled={saving}>
      <div className="settings-meta-grid">
        <Metric
          label="探针"
          value={draft.probe.enabled ? "已启用" : "已停用"}
          tone={draft.probe.enabled ? "success" : "warning"}
        />
        <Metric
          label="通知"
          value={draft.notifications.enabled ? "已启用" : "已停用"}
          tone={draft.notifications.enabled ? "success" : "warning"}
        />
        <Metric label="错误监控" value={probeStateLabel(errorMonitorState)} tone={draft.probe.error_monitor.enabled ? "success" : "warning"} />
        <Metric label="错误事件" value={String(status?.error_monitor_incident_count ?? 0)} />
        {capabilities.codexStatePaths && (
          <Metric label="Codex Home" value={codexHomeStatusValue(status ?? settings?.codex)} wide />
        )}
        <Metric
          label="Discovery"
          value={probeDiscoveryWarningsText(
            status?.discovery_warnings ??
              settings?.codex?.discovery_warnings ??
              settings?.discovery_warnings,
          )}
          wide
        />
      </div>

      <div className="probe-toggle-grid">
        <label className="toggle-row">
          <span>启用 Probe</span>
          <input
            type="checkbox"
            checked={draft.probe.enabled}
            onChange={event => setProbe({ enabled: event.target.checked })}
          />
        </label>
        <label className="toggle-row">
          <span>终止错误监控</span>
          <input
            type="checkbox"
            checked={draft.probe.error_monitor.enabled}
            onChange={event =>
              setProbe({ error_monitor: { ...draft.probe.error_monitor, enabled: event.target.checked } })
            }
          />
        </label>
      </div>

      <div className="probe-platform-switch-section">
        <div className="probe-platform-switch-header">
          <strong>平台通知一键开关</strong>
        </div>
        <div className="probe-platform-switch-grid">
          {visiblePlatforms.map(platform => {
            const enabled = isPlatformNotificationEnabled(draft, platform.id);
            const runtimeStats = status?.provider_notifications?.find(
              entry => entry.provider === platform.id,
            );
            return (
              <label
                key={platform.id}
                className={`probe-platform-switch-card ${enabled ? "is-enabled" : ""}`}
                data-agent={platform.brand}
              >
                <span className="probe-platform-switch-brand">
                  <span className="agent-brand-badge" data-agent={platform.brand} aria-hidden="true">
                    <AgentBrandIcon agent={platform.brand} size={17} />
                  </span>
                  <span className="probe-platform-switch-copy">
                    <strong>{platform.label}</strong>
                    {runtimeStats && <small>{runtimeStats.streams} 个会话</small>}
                  </span>
                </span>
                <input
                  type="checkbox"
                  aria-label={platform.id === "grok" ? "Grok 通知" : `${platform.label} 通知`}
                  checked={enabled}
                  onChange={event =>
                    setDraft(applyPlatformNotificationToggle(draft, platform.id, event.target.checked))
                  }
                />
              </label>
            );
          })}
        </div>
      </div>

      <details className="probe-advanced-settings" open>
        <summary>高级轮询与保留参数</summary>
        <div className="form-grid compact-three">
          {capabilities.codexStatePaths && (
            <label className="field-label">
              Codex Home
              <input
                value={draft.codex.home}
                placeholder="auto"
                onChange={event => setCodex({ home: event.target.value })}
              />
            </label>
          )}
          <label className="field-label">
            主机标签
            <input
              value={draft.codex.host_label}
              onChange={event => setCodex({ host_label: event.target.value })}
            />
          </label>
          {numericFields.map(field => (
            <label key={field.label} className="field-label">
              {field.label}
              <input
                type="number"
                min={field.min}
                max={field.max}
                value={field.value}
                onChange={event => field.onChange(event.target.value)}
              />
            </label>
          ))}
        </div>
      </details>

      {errors.length > 0 && <div className="form-error">{errors[0]}</div>}
      {saveStatus && (
        <div className={saveStatus.tone === "success" ? "form-success" : "form-error"}>
          {saveStatus.message}
        </div>
      )}
      <button className="primary-button" disabled={saving || errors.length > 0} onClick={onSave}>
        <CheckCircle2 size={17} />保存设置
      </button>
    </fieldset>
  );
}

export function ProbeEventsCard(props: {
  events: ProbeEvent[];
  available: boolean;
  loading: boolean;
}) {
  if (!props.available) {
    return <div className="muted-row">{props.loading ? "正在读取事件" : "事件接口不可用"}</div>;
  }
  if (props.events.length === 0) {
    return <div className="muted-row">暂无最近通知与 Hook 事件</div>;
  }
  return (
    <div className="preview-list compact">
      {props.events.map(event => (
        <ProbeEventRow event={event} key={event.id} />
      ))}
    </div>
  );
}

function ProbeEventRow({ event }: { event: ProbeEvent }) {
  const card = probeEventCard(event);
  return (
    <article className="preview-item probe-event-card">
      <div>
        <strong>
          {card.title} · {card.headline}
        </strong>
        <span>{card.summary}</span>
      </div>
      {card.reason && <small>{card.reason}</small>}
      <div className="probe-event-detail-row">
        <span className={`status-chip tone-${card.bark.tone}`}>{card.bark.label}</span>
        <span className={`status-chip tone-${card.dedupe.tone}`}>{card.dedupe.label}</span>
        {card.details.map(detail => (
          <span key={`${detail.label}:${detail.value}`}>
            {detail.label}: {detail.value}
          </span>
        ))}
        <span>{card.time}</span>
      </div>
    </article>
  );
}
