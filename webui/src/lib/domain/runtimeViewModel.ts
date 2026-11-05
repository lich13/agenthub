import type {
  HiddenScanProgress,
  HiddenThreadDeletePlan,
  JobRecord,
  ProbeEvent,
  ProbeJobAction,
  ProbeSettings,
  ProbeStatus,
  ThreadDetail,
  ThreadSummary,
  UnifiedCleanupPlan,
  UnifiedCleanupResult,
  UnifiedCleanupScopePlan,
  UnifiedCleanupTargetSelection,
  UpdateStatus
} from "../../types";
import { runtimeCapabilitiesForRuntime, type RuntimeCapabilityMatrix } from "./capabilities";
import { hostCapabilityPolicy } from "./hostCapabilityPolicy";

export type RuntimeCapabilityInput = RuntimeCapabilityMatrix | undefined;

const DEFAULT_RUNTIME_CAPABILITIES = runtimeCapabilitiesForRuntime("desktop");

export function capabilitiesForInput(input?: RuntimeCapabilityInput): RuntimeCapabilityMatrix {
  return input ?? DEFAULT_RUNTIME_CAPABILITIES;
}

export const OPS_PANEL_TITLES = {
  updates: "本机 App 更新",
  unifiedCleanup: "全平台线程清理",
  archivedCleanup: "归档线程清理",
  hiddenCleanup: "隐藏线程清理",
  dshArchiveCleanup: "DSH 归档清理",
  jobs: "Job History"
} as const;

export type UnifiedCleanupScopeMeta = {
  scope: string;
  provider: "codex" | "claude" | "grok" | "antigravity" | "dsh";
  agentTitle: string;
  scopeBadge: string;
  label: string;
  description: string;
};

export const UNIFIED_CLEANUP_SCOPES: readonly UnifiedCleanupScopeMeta[] = [
  {
    scope: "codex_archived",
    provider: "codex",
    agentTitle: "Codex",
    scopeBadge: "已归档线程",
    label: "Codex 已归档线程",
    description: "归档主线程及已验证子代理闭包"
  },
  {
    scope: "codex_hidden",
    provider: "codex",
    agentTitle: "Codex",
    scopeBadge: "隐藏线程",
    label: "Codex 隐藏线程",
    description: "内部 exec 与子代理隐藏会话文件"
  },
  {
    scope: "antigravity_archived",
    provider: "antigravity",
    agentTitle: "Antigravity",
    scopeBadge: "已归档线程",
    label: "Antigravity 已归档线程",
    description: "本机已归档会话及子代理目录闭包"
  },
  {
    scope: "dsh_archived",
    provider: "dsh",
    agentTitle: "DeepSeek Harness",
    scopeBadge: "已归档线程",
    label: "DeepSeek Harness 已归档线程",
    description: "已归档主会话及子会话存储文件"
  },
  {
    scope: "claude_empty",
    provider: "claude",
    agentTitle: "Claude Code",
    scopeBadge: "空残余会话",
    label: "Claude Code 空残余会话",
    description: "0 消息且无进程占用的空残余会话"
  },
  {
    scope: "grok_empty",
    provider: "grok",
    agentTitle: "Grok Build",
    scopeBadge: "空残余会话",
    label: "Grok Build 空残余会话",
    description: "0 消息且非活跃状态的空会话目录"
  }
];

export function opsWorkspacePanelTitles(input?: RuntimeCapabilityInput): string[] {
  const capabilities = capabilitiesForInput(input);
  return [
    updatePanelTitle(capabilities),
    ...(capabilities.threadCleanup ? [OPS_PANEL_TITLES.unifiedCleanup] : []),
    OPS_PANEL_TITLES.jobs
  ];
}

export function opsWorkspaceVisibleCopy(input?: RuntimeCapabilityInput): string[] {
  const capabilities = capabilitiesForInput(input);
  return [
    ...opsWorkspacePanelTitles(input),
    "当前版本",
    ...opsUpdateActionView(null, capabilities).map((action) => action.label),
    ...(capabilities.threadCleanup ? [
      "全平台扫描预览",
      "深度扫描隐藏线程",
      "一键清理所选平台",
      "确认清理所选",
      "可清理会话",
      "预计释放空间",
      "已选平台范围"
    ] : []),
    failureCategoryLabel("systemd_failure", capabilities),
    failureCategoryLabel("permission_denied_sudo", capabilities)
  ];
}

export function resolvedSelectedThreadId(selectedId: string | null): string | null {
  return selectedId;
}

export function canStartHiddenThreadDelete(plan: HiddenThreadDeletePlan | null | undefined): boolean {
  return Boolean(plan) && (!plan!.scan || (plan!.scan!.phase === "complete" && !plan!.scan!.hasMore
    && plan!.candidates.length === plan!.scan!.total)) && Boolean(plan!.candidates?.length);
}

export function canStartUpdateInstall(status: UpdateStatus | null | undefined): boolean {
  return status?.update_available === true
    && /^v?\d+\.\d+\.\d+(?:[-+].*)?$/i.test(status.latest_version?.trim() ?? "")
    && displayVersion(status.latest_version) !== displayVersion(status.current_version)
    && status.state !== "failed" && status.state !== "unsupported";
}

export function displayVersion(version?: string | null): string {
  return version?.trim().replace(/^v(?=\d)/i, "") || "未知";
}

export function updatePanelTitle(input?: RuntimeCapabilityInput): string {
  return capabilitiesForInput(input).updateServiceLabels ? "远程服务更新" : "本机 App 更新";
}

export function opsUpdateActionView(
  status: UpdateStatus | null | undefined,
  input?: RuntimeCapabilityInput
): Array<{ action: "check" | "install" | "prune"; label: string; tone: "secondary" | "primary" | "danger"; disabled: boolean }> {
  const capabilities = capabilitiesForInput(input);
  return [
    {
      action: "check",
      label: "检查更新",
      tone: "secondary",
      disabled: false
    },
    ...(canStartUpdateInstall(status) ? [{
      action: "install" as const,
      label: `更新至 ${displayVersion(status?.latest_version)}`,
      tone: "primary" as const,
      disabled: false
    }] : []),
    ...(capabilities.updatePrune ? [{
      action: "prune" as const,
      label: "清理更新备份",
      tone: "danger" as const,
      disabled: false
    }] : [])
  ];
}

type Tone = "success" | "warning" | "danger";

type CleanupStageInput = {
  hasPlan: boolean;
  dryRunPending: boolean;
  armed: boolean;
  executePending: boolean;
  executableCount: number;
};

export type CleanupStageView = {
  label: string;
  tone?: Tone;
};

function cleanupStageLabel(input: CleanupStageInput): CleanupStageView {
  if (input.executePending) return { label: "执行中", tone: "warning" };
  if (input.armed) return { label: "等待确认", tone: "danger" };
  if (input.dryRunPending) return { label: "扫描中", tone: "warning" };
  if (!input.hasPlan) return { label: "待 dry-run" };
  if (input.executableCount > 0) return { label: "可清理", tone: "warning" };
  return { label: "无可清理", tone: "success" };
}

export function sourceCountsText(counts?: Record<string, number> | null): string {
  if (!counts || Object.keys(counts).length === 0) return "暂无";
  return Object.entries(counts)
    .sort(([left], [right]) => left.localeCompare(right))
    .map(([key, value]) => `${key}:${value}`)
    .join(" ");
}

export type ThreadMessageControllerView = {
  hydration: {
    threadId: string | null;
    selectedThreadSummary?: ThreadSummary | null;
    selectedDetail?: ThreadDetail | null;
  };
  realtime: {
    threadId: string;
    applyThreadTitleOverride: <T extends Partial<ThreadSummary>>(summary: T) => T;
  };
};

export function threadMessageControllerView(input: {
  threadId: string | null;
  selectedThreadSummary?: ThreadSummary | null;
  selectedDetail?: ThreadDetail | null;
  applyThreadTitleOverride?: <T extends Partial<ThreadSummary>>(summary: T) => T;
}): ThreadMessageControllerView {
  return {
    hydration: {
      threadId: input.threadId,
      selectedThreadSummary: input.selectedThreadSummary,
      selectedDetail: input.selectedDetail
    },
    realtime: {
      threadId: input.threadId ?? "",
      applyThreadTitleOverride: input.applyThreadTitleOverride ?? ((summary) => summary)
    }
  };
}

export type ProbeAvailabilityView = {
  headline: string;
  metric: string;
  tone: Tone;
};

export type ProbeThreadsByStatusView = {
  running: ThreadSummary[];
  replyNeeded: ThreadSummary[];
  recoverable: ThreadSummary[];
};

export type ProbeWorkspaceView = {
  available: boolean;
  data?: ProbeStatus;
  currentSettings?: ProbeSettings;
  barkConfigured: boolean;
  probeThreads: ProbeThreadsByStatusView;
  probeEnabled: boolean;
  serviceText: string;
  availability: ProbeAvailabilityView;
  statusTone: Tone;
  snapshotText: string;
  snapshotTone: Tone;
  probeJobs: JobRecord[];
};

export function probeWorkspaceView(input: {
  data?: ProbeStatus;
  available?: boolean;
  currentSettings?: ProbeSettings;
  recentEventCount?: number;
  jobs?: JobRecord[];
  loading?: boolean;
  fetching?: boolean;
  error?: boolean;
  draftDeviceKeyConfigured?: boolean;
}): ProbeWorkspaceView {
  const data = input.data;
  const barkConfigured = Boolean(input.currentSettings?.notifications?.device_key_configured || input.draftDeviceKeyConfigured);
  const probeEnabled = data?.enabled ?? input.currentSettings?.probe?.enabled ?? false;
  const availability = probeAvailabilityView({
    available: input.available,
    probeEnabled,
    loading: input.loading,
    fetching: input.fetching,
    hasData: Boolean(data),
    error: input.error
  });
  return {
    available: input.available ?? false,
    data,
    currentSettings: input.currentSettings,
    barkConfigured,
    probeThreads: probeThreadsByStatus(data),
    probeEnabled,
    serviceText: data ? `${data.service_kind}:${data.service_name}` : "未知",
    availability,
    statusTone: availability.tone,
    snapshotText: probeSnapshotStatusText(data, input.fetching),
    snapshotTone: data?.is_refreshing || input.fetching ? "warning" : "success",
    probeJobs: (input.jobs ?? []).filter(isProbeJob).slice(0, 6)
  };
}

export function probeStatusThreads(status?: Pick<ProbeStatus, "running_threads" | "reply_needed_threads" | "recoverable_threads"> | null): ThreadSummary[] {
  return [
    ...(status?.running_threads ?? []),
    ...(status?.reply_needed_threads ?? []),
    ...(status?.recoverable_threads ?? [])
  ];
}

export function probeThreadsByStatus(status?: Pick<ProbeStatus, "running_threads" | "reply_needed_threads" | "recoverable_threads"> | null): ProbeThreadsByStatusView {
  return {
    running: status?.running_threads ?? [],
    replyNeeded: status?.reply_needed_threads ?? [],
    recoverable: status?.recoverable_threads ?? []
  };
}

export function probeRunningCountValue(status?: Pick<ProbeStatus, "running_count" | "running_threads"> | null): string {
  const backendCount = typeof status?.running_count === "number" ? Math.max(0, status.running_count) : 0;
  const threadCount = status?.running_threads?.length ?? 0;
  return String(backendCount > 0 ? backendCount : threadCount);
}

export function probeSettingsAfterBarkSave<T extends { notifications: { device_key_configured?: boolean } }>(
  saved: T,
  submittedDeviceKey?: string | null,
): T {
  if (!submittedDeviceKey?.trim()) return saved;
  return {
    ...saved,
    notifications: {
      ...saved.notifications,
      device_key_configured: true
    }
  };
}

export function isProbeJob(job: JobRecord): boolean {
  return job.kind.startsWith("probe_")
    || job.kind.startsWith("probe-")
    || job.title.includes("探针")
    || job.title.includes("Probe");
}

export type ProbeLiveSessionsByBucket = {
  running: import("../../types").ProbeLiveSession[];
  replyNeeded: import("../../types").ProbeLiveSession[];
  recoverable: import("../../types").ProbeLiveSession[];
};

export function probeLiveSessionsByBucket(
  status?: Pick<ProbeStatus, "active_sessions" | "running_threads" | "reply_needed_threads" | "recoverable_threads"> | null
): ProbeLiveSessionsByBucket {
  if (Array.isArray(status?.active_sessions) && status.active_sessions.length > 0) {
    return {
      running: status.active_sessions.filter((item) => item.bucket === "running"),
      replyNeeded: status.active_sessions.filter((item) => item.bucket === "reply_needed"),
      recoverable: status.active_sessions.filter((item) => item.bucket === "recoverable")
    };
  }
  const mapCodex = (threads: ThreadSummary[] | undefined, bucket: "running" | "reply_needed" | "recoverable") =>
    (threads ?? []).map((thread) => ({
      provider: "codex",
      id: thread.id,
      session_key: thread.id,
      title: thread.title?.trim() ? thread.title : thread.id,
      bucket,
      updated_at: thread.updated_at,
      cwd: thread.cwd
    }));
  return {
    running: mapCodex(status?.running_threads, "running"),
    replyNeeded: mapCodex(status?.reply_needed_threads, "reply_needed"),
    recoverable: mapCodex(status?.recoverable_threads, "recoverable")
  };
}

export function probeJobActionLabel(action: ProbeJobAction | undefined): string {
  switch (action) {
    case "bark-test":
      return "AgentHub 推送测试";
    case "hooks-install":
      return "Hook 安装";
    case "hooks-remove":
      return "Hook 移除";
    default:
      return "Probe job";
  }
}

export function probeEventSummary(event: ProbeEvent): string {
  const thread = event.thread_id ? `线程 ${event.thread_id}` : "无线程";
  const fields = [
    event.payload?.session_id ? "session" : "",
    event.payload?.transcript_path ? "transcript" : "",
    event.payload?.last_assistant_message ? "assistant" : ""
  ].filter(Boolean);
  return [thread, fields.length ? fields.join(" · ") : "payload 已脱敏"].join(" · ");
}

export function shouldAutoScrollProbeFeed(
  current: { scrollTop: number; clientHeight: number; scrollHeight: number },
  _previous?: { scrollTop: number; clientHeight: number; scrollHeight: number } | null
): boolean {
  return current.scrollHeight - current.scrollTop - current.clientHeight <= 32;
}

type CodexHomePathFields = {
  home?: string | null;
  codex_home?: string | null;
  configured_codex_home?: string | null;
  resolved_codex_home?: string | null;
  codex_home_source?: string | null;
};

export function codexHomeStatusValue(status?: CodexHomePathFields | null): string {
  return pathWithSource(
    firstStringValue(status, ["resolved_codex_home", "codex_home", "home", "configured_codex_home"]),
    firstStringValue(status, ["codex_home_source"])
  );
}

export function probeDiscoveryWarningsText(warnings?: string[] | null): string {
  return warnings?.length ? warnings.join(", ") : "无";
}

export function pathText(value?: string | null): string {
  return value && value.trim() ? value : "未知";
}

function pathWithSource(value?: string | null, source?: string | null): string {
  const path = pathText(value);
  const cleanedSource = source?.trim();
  return path !== "未知" && cleanedSource ? `${path} · ${cleanedSource}` : path;
}

function firstStringValue(source: unknown, keys: string[]): string | null {
  if (!source || typeof source !== "object") return null;
  const record = source as Record<string, unknown>;
  for (const key of keys) {
    const value = record[key];
    if (typeof value === "string" && value.trim()) return value.trim();
  }
  return null;
}

export function probeStateLabel(value?: string | null): string {
  if (!value) return "未知";
  const labels: Record<string, string> = {
    managed: "已管理",
    stale: "需修复",
    missing: "未安装",
    disabled: "已停用",
    configured: "已配置",
    not_configured: "未配置",
    maintenance_ready: "可维护",
    ready: "就绪",
    ok: "正常",
    builtin: "内置"
  };
  return labels[value] ?? value;
}

export function isProbeSettings(value: unknown): value is ProbeSettings {
  return Boolean(value && typeof value === "object" && "codex" in value && "probe" in value && "notifications" in value);
}

export function logBytesDraftToMb(value: number | ""): number | "" {
  if (value === "") return "";
  return Math.max(1, Math.round(value / (1024 * 1024)));
}

export function mbDraftToLogBytes(value: string): number | "" {
  const parsed = numberInputDraftValue(value);
  return parsed === "" ? "" : parsed * 1024 * 1024;
}

export function probeSnapshotStatusText(status?: Pick<ProbeStatus, "snapshot_age_seconds" | "is_refreshing" | "snapshot_status"> | null, fetching = false): string {
  const age = typeof status?.snapshot_age_seconds === "number" ? Math.max(0, Math.round(status.snapshot_age_seconds)) : null;
  const prefix = status?.is_refreshing || fetching ? "后台刷新" : "已同步";
  if (age === null) return prefix;
  if (age < 60) return `${prefix} ${age}s`;
  const minutes = Math.floor(age / 60);
  return `${prefix} ${minutes}m`;
}

export function probeAvailabilityView(input: {
  available?: boolean;
  probeEnabled?: boolean;
  loading?: boolean;
  fetching?: boolean;
  hasData?: boolean;
  error?: boolean;
}): ProbeAvailabilityView {
  if (!input.hasData && input.error) {
    return {
      headline: "Probe 快照读取失败",
      metric: "读取失败",
      tone: "danger"
    };
  }
  if (!input.hasData && (input.loading || input.fetching)) {
    return {
      headline: "正在读取 Probe 快照",
      metric: "读取中",
      tone: "warning"
    };
  }
  if (input.available) {
    return input.probeEnabled
      ? { headline: "Probe 正在接管远程主机观测", metric: "运行中", tone: "success" }
      : { headline: "Probe 已停用", metric: "停用", tone: "warning" };
  }
  return {
    headline: "Probe 端点不可用",
    metric: "不可用",
    tone: "danger"
  };
}

function numberInputDraftValue(value: string): number | "" {
  const trimmed = value.trim();
  if (!trimmed) return "";
  const numeric = Number(trimmed);
  return Number.isFinite(numeric) ? Math.trunc(numeric) : "";
}

export function formatCleanupBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  return formatFileSize(bytes);
}

const SCAN_PHASE_LABELS: Record<HiddenScanProgress["phase"], string> = {
  preparing: "正在准备",
  scanning: "正在扫描",
  complete: "扫描完成",
  stopping: "正在停止",
  stopped: "已停止",
  failed: "扫描失败"
};

export function unifiedCleanupStageView(input: {
  hasPlan: boolean;
  dryRunPending: boolean;
  armed: boolean;
  executePending: boolean;
  executableCount: number;
  hiddenScan?: HiddenScanProgress | null;
}): CleanupStageView {
  if (input.executePending) return { label: "执行中", tone: "warning" };
  if (input.armed) return { label: "等待确认", tone: "danger" };
  if (input.hiddenScan && input.hiddenScan.phase !== "complete") {
    const label = `${SCAN_PHASE_LABELS[input.hiddenScan.phase]} · ${input.hiddenScan.processed} / ${input.hiddenScan.total}`;
    const tone = input.hiddenScan.phase === "failed" ? "danger" : "warning";
    return { label, tone };
  }
  return cleanupStageLabel({
    hasPlan: input.hasPlan,
    dryRunPending: input.dryRunPending,
    armed: input.armed,
    executePending: input.executePending,
    executableCount: input.executableCount
  });
}

export function mergeHiddenScanIntoUnifiedPlan(
  current: UnifiedCleanupPlan | null,
  hiddenPlan: HiddenThreadDeletePlan
): UnifiedCleanupPlan {
  const scanComplete = !hiddenPlan.scan || (
    hiddenPlan.scan.phase === "complete"
    && !hiddenPlan.scan.hasMore
    && hiddenPlan.candidates.length === hiddenPlan.scan.total
  );
  const candidates = hiddenPlan.candidates.map((item) => ({
    id: item.id,
    title: item.title || item.id,
    estimatedBytes: 0,
    fingerprint: item.fingerprint ?? null,
    blockedReason: item.allowed ? null : (item.reason ?? "当前线程暂不可清理")
  }));
  const deletableCount = candidates.filter((item) => !item.blockedReason).length;
  const hiddenScope: UnifiedCleanupScopePlan = {
    scope: "codex_hidden",
    provider: "codex",
    label: "Codex 隐藏线程",
    supported: true,
    scanned: scanComplete,
    unavailableReason: null,
    candidateCount: candidates.length,
    deletableCount,
    estimatedBytes: 0,
    candidates,
    issues: []
  };
  const existing = new Map((current?.scopes ?? []).map((item) => [item.scope, item]));
  existing.set("codex_hidden", hiddenScope);
  const scopes = UNIFIED_CLEANUP_SCOPES.map((meta) => (
    existing.get(meta.scope) ?? {
      scope: meta.scope,
      provider: meta.provider,
      label: meta.label,
      supported: true,
      scanned: false,
      unavailableReason: null,
      candidateCount: 0,
      deletableCount: 0,
      estimatedBytes: 0,
      candidates: [],
      issues: []
    }
  ));
  return {
    totalCandidates: scopes.reduce((sum, item) => sum + item.candidateCount, 0),
    totalDeletable: scopes.reduce((sum, item) => sum + item.deletableCount, 0),
    totalEstimatedBytes: scopes.reduce((sum, item) => sum + item.estimatedBytes, 0),
    scopes
  };
}

export function collectUnifiedCleanupTargets(
  plan: UnifiedCleanupPlan | null,
  selectedScopes: readonly string[]
): UnifiedCleanupTargetSelection[] {
  if (!plan || selectedScopes.length === 0) return [];
  const selectedSet = new Set(selectedScopes);
  const targets: UnifiedCleanupTargetSelection[] = [];
  for (const scope of plan.scopes) {
    if (!selectedSet.has(scope.scope) || !scope.supported || !scope.scanned) continue;
    for (const candidate of scope.candidates) {
      if (candidate.blockedReason) continue;
      targets.push({
        scope: scope.scope,
        id: candidate.id,
        fingerprint: candidate.fingerprint ?? null
      });
    }
  }
  return targets;
}

export function unifiedPlanAfterExecute(
  current: UnifiedCleanupPlan | null,
  result: UnifiedCleanupResult
): UnifiedCleanupPlan | null {
  if (!current) return current;
  const byScope = new Map(result.scopes.map((item) => [item.scope, item]));
  const scopes = current.scopes.map((scope) => {
    const executed = byScope.get(scope.scope);
    if (!executed || executed.deletedCount === 0) return scope;
    if (executed.failedCount === 0 && executed.skippedCount === 0 && executed.residuals.length === 0) {
      return {
        ...scope,
        candidateCount: 0,
        deletableCount: 0,
        estimatedBytes: 0,
        candidates: []
      };
    }
    const remainingDeletable = Math.max(0, scope.deletableCount - executed.deletedCount);
    const remainingCount = Math.max(remainingDeletable, scope.candidateCount - executed.deletedCount);
    return {
      ...scope,
      candidateCount: remainingCount,
      deletableCount: remainingDeletable,
      estimatedBytes: Math.max(0, scope.estimatedBytes - executed.deletedBytes)
    };
  });
  return {
    totalCandidates: scopes.reduce((sum, item) => sum + item.candidateCount, 0),
    totalDeletable: scopes.reduce((sum, item) => sum + item.deletableCount, 0),
    totalEstimatedBytes: scopes.reduce((sum, item) => sum + item.estimatedBytes, 0),
    scopes
  };
}

export type UnifiedCleanupUnsuccessfulEntry = {
  key: string;
  text: string;
  residuals: string[];
};

export function unifiedCleanupUnsuccessfulItems(
  result: UnifiedCleanupResult | null
): UnifiedCleanupUnsuccessfulEntry[] {
  if (!result) return [];
  const entries: UnifiedCleanupUnsuccessfulEntry[] = [];
  for (const scope of result.scopes) {
    scope.issues.forEach((issue, index) => {
      entries.push({
        key: `${scope.scope}:issue:${index}`,
        text: `${scope.label} · ${issue}`,
        residuals: index === 0 ? scope.residuals : []
      });
    });
    if (scope.issues.length === 0 && scope.residuals.length > 0) {
      entries.push({
        key: `${scope.scope}:residuals`,
        text: `${scope.label} · 存在未清理残留路径`,
        residuals: scope.residuals
      });
    }
  }
  return entries;
}

function formatFileSize(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) return "unknown";
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KiB", "MiB", "GiB"];
  let value = bytes / 1024;
  for (const unit of units) {
    if (value < 1024 || unit === units[units.length - 1]) {
      return `${value.toFixed(value >= 10 ? 0 : 1)} ${unit}`;
    }
    value /= 1024;
  }
  return `${bytes} B`;
}

export function failureCategoryLabel(category: string, input?: RuntimeCapabilityInput): string {
  const capabilities = capabilitiesForInput(input);
  const policy = hostCapabilityPolicy(capabilities);
  if (policy.failureLabels[category]) return policy.failureLabels[category];
  const labels: Record<string, string> = {
    release_missing: "Release 缺失",
    download_sha256_mismatch: "下载或校验失败",
    read_only_file_system: "文件系统只读/安装目录不可写",
    codex_auth_failure: "Codex 认证失败",
    sqlite_integrity_failure: "SQLite 完整性失败",
    network_tls_eof: "网络或 TLS 中断",
    codex_local_state_unavailable: "Codex 本地状态不可用",
    app_server_unavailable: "Codex 本地状态不可用",
    unknown: "未知失败"
  };
  return labels[category] ?? category;
}

export function jobFailureAnalysisView(
  analysis: NonNullable<JobRecord["failure_analysis"]>,
  input?: RuntimeCapabilityInput
): { label: string; explanation: string; suggestions: string[] } {
  const capabilities = capabilitiesForInput(input);
  const label = failureCategoryLabel(analysis.category, capabilities);
  return { label, explanation: analysis.explanation, suggestions: analysis.suggestions };
}

export function jobOutputView(value: string, _input?: RuntimeCapabilityInput): string {
  return value.trim() || "no output";
}
