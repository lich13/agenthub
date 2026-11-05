import type { ProbeEvent, ProbeSettings } from "../types";

export const PROBE_NAV_LABEL = "Probe";
export const probeSections = [{ id: "settings", label: "通知配置" }] as const;
export type ProbeSectionId = (typeof probeSections)[number]["id"];
export type ProbeNumericDraftValue = number | "";

export type ProbeSettingsDraft = {
  codex: { home: string; workspace: string; host_label: string };
  probe: {
    enabled: boolean;
    poll_seconds: ProbeNumericDraftValue;
    recent_limit: ProbeNumericDraftValue;
    error_monitor: { enabled: boolean };
  };
  hooks: { manage_stop_hook: boolean };
  notifications: {
    enabled: boolean;
    device_key: string;
    device_key_configured: boolean;
    server_url: string;
    sound: string;
    group: string;
    url: string;
    notify_completion: boolean;
    notify_reply_needed: boolean;
    notify_recoverable: boolean;
    notify_codex: boolean;
    notify_grok: boolean;
    notify_grokbot: boolean;
    notify_grok_completion: boolean;
    notify_grok_failure: boolean;
    notify_grok_reply_needed: boolean;
    notify_claude: boolean;
    notify_claude_completion: boolean;
    notify_claude_failure: boolean;
    notify_claude_reply_needed: boolean;
    notify_antigravity: boolean;
    notify_antigravity_completion: boolean;
    notify_antigravity_failure: boolean;
    notify_antigravity_reply_needed: boolean;
    notify_antigravity_authorization: boolean;
    notify_dsh: boolean;
    notify_dsh_completion: boolean;
    notify_dsh_failure: boolean;
    notify_dsh_reply_needed: boolean;
    notify_dsh_authorization: boolean;
  };
  observability: {
    event_retention_days: ProbeNumericDraftValue;
    hook_event_max_lines: ProbeNumericDraftValue;
    hook_cooldown_max_lines: ProbeNumericDraftValue;
    log_max_bytes: ProbeNumericDraftValue;
  };
};

export const PROBE_MAC_DEFAULTS = {
  observability: {
    event_retention_days: 2,
    hook_event_max_lines: 500,
    hook_cooldown_max_lines: 1000,
    log_max_bytes: 5 * 1024 * 1024,
  },
} as const;

export function buildProbeSettingsDraft(settings: ProbeSettings): ProbeSettingsDraft {
  const codex = settings.codex ?? {};
  const probe = settings.probe ?? {};
  const n = settings.notifications ?? {};
  const obs = probe.observability;
  const def = PROBE_MAC_DEFAULTS.observability;
  return {
    codex: {
      home: configuredCodexHomeDraftValue(settings),
      workspace: stringOrEmpty(codex.workspace),
      host_label: stringOrEmpty(codex.host_label),
    },
    probe: {
      enabled: Boolean(probe.enabled),
      poll_seconds: toBoundedInteger(probe.poll_seconds, 15) ?? 15,
      recent_limit: toBoundedInteger(probe.recent_limit, 50) ?? 50,
      error_monitor: { enabled: probe.error_monitor?.enabled !== false },
    },
    hooks: { manage_stop_hook: probe.hooks?.manage_stop_hook !== false },
    notifications: {
      enabled: Boolean(n.enabled),
      device_key: "",
      device_key_configured: Boolean(n.device_key_configured),
      server_url: n.server_url ?? "https://api.day.app",
      sound: stringOrEmpty(n.sound),
      group: typeof n.group === "string" ? n.group : "AgentHub",
      url: stringOrEmpty(n.url),
      notify_completion: n.notify_completion !== false,
      notify_reply_needed: n.notify_reply_needed !== false,
      notify_recoverable: n.notify_recoverable !== false,
      notify_codex: n.notify_codex !== false,
      notify_grok: n.notify_grok !== false,
      notify_grokbot: n.notify_grokbot === true,
      notify_grok_completion: n.notify_grok_completion !== false,
      notify_grok_failure: n.notify_grok_failure !== false,
      notify_grok_reply_needed: n.notify_grok_reply_needed !== false,
      notify_claude: n.notify_claude !== false,
      notify_claude_completion: n.notify_claude_completion !== false,
      notify_claude_failure: n.notify_claude_failure !== false,
      notify_claude_reply_needed: n.notify_claude_reply_needed !== false,
      notify_antigravity: n.notify_antigravity === true,
      notify_antigravity_completion: n.notify_antigravity_completion === true,
      notify_antigravity_failure: n.notify_antigravity_failure === true,
      notify_antigravity_reply_needed: n.notify_antigravity_reply_needed === true,
      notify_antigravity_authorization: n.notify_antigravity_authorization === true,
      notify_dsh: n.notify_dsh === true,
      notify_dsh_completion: n.notify_dsh_completion === true,
      notify_dsh_failure: n.notify_dsh_failure === true,
      notify_dsh_reply_needed: n.notify_dsh_reply_needed === true,
      notify_dsh_authorization: n.notify_dsh_authorization === true,
    },
    observability: {
      event_retention_days: toBoundedInteger(obs?.event_retention_days, 2) ?? 2,
      hook_event_max_lines: toBoundedInteger(obs?.hook_event_max_lines, def.hook_event_max_lines) ?? def.hook_event_max_lines,
      hook_cooldown_max_lines:
        toBoundedInteger(obs?.hook_cooldown_max_lines, def.hook_cooldown_max_lines) ?? def.hook_cooldown_max_lines,
      log_max_bytes: toBoundedInteger(obs?.log_max_bytes, def.log_max_bytes) ?? def.log_max_bytes,
    },
  };
}

export type ProbeSettingsPayload = {
  codex: ProbeSettings["codex"];
  probe: Pick<
    ProbeSettings["probe"],
    "enabled" | "poll_seconds" | "recent_limit" | "hooks" | "error_monitor" | "notifications" | "observability"
  >;
  notifications?: Pick<ProbeSettings["notifications"], "device_key">;
};

export function buildProbeSettingsPayload(
  draft: ProbeSettingsDraft,
  _current?: ProbeSettings | null,
  submittedDeviceKey?: string | null,
): ProbeSettingsPayload {
  const errors = probeSettingsValidation(draft);
  if (errors.length) throw new Error(errors[0]);
  const n = draft.notifications;
  const deviceKey = (submittedDeviceKey ?? n.device_key).trim();
  const notifications: NonNullable<ProbeSettingsPayload["probe"]["notifications"]> & { device_key?: string } = {
    enabled: n.enabled || Boolean(deviceKey),
    server_url: n.server_url.trim(),
    sound: optionalString(n.sound),
    group: n.group.trim(),
    url: optionalString(n.url),
    notify_completion: n.notify_completion,
    notify_reply_needed: n.notify_reply_needed,
    notify_recoverable: n.notify_recoverable,
    notify_codex: n.notify_codex,
    notify_grok: n.notify_grok,
    notify_grokbot: n.notify_grokbot,
    notify_grok_completion: n.notify_grok_completion,
    notify_grok_failure: n.notify_grok_failure,
    notify_grok_reply_needed: n.notify_grok_reply_needed,
    notify_claude: n.notify_claude,
    notify_claude_completion: n.notify_claude_completion,
    notify_claude_failure: n.notify_claude_failure,
    notify_claude_reply_needed: n.notify_claude_reply_needed,
    notify_antigravity: n.notify_antigravity,
    notify_antigravity_completion: n.notify_antigravity_completion,
    notify_antigravity_failure: n.notify_antigravity_failure,
    notify_antigravity_reply_needed: n.notify_antigravity_reply_needed,
    notify_antigravity_authorization: n.notify_antigravity_authorization,
    notify_dsh: n.notify_dsh,
    notify_dsh_completion: n.notify_dsh_completion,
    notify_dsh_failure: n.notify_dsh_failure,
    notify_dsh_reply_needed: n.notify_dsh_reply_needed,
    notify_dsh_authorization: n.notify_dsh_authorization,
    ...(deviceKey ? { device_key: deviceKey } : {}),
  };

  return {
    codex: {
      home: codexHomePatchValue(draft.codex.home),
      workspace: draft.codex.workspace.trim() || null,
      host_label: draft.codex.host_label.trim(),
    },
    probe: {
      enabled: draft.probe.enabled,
      poll_seconds: requiredDraftNumber(draft.probe.poll_seconds),
      recent_limit: requiredDraftNumber(draft.probe.recent_limit),
      error_monitor: { enabled: draft.probe.error_monitor.enabled },
      hooks: { manage_stop_hook: draft.hooks.manage_stop_hook },
      notifications,
      observability: {
        event_retention_days: requiredDraftNumber(draft.observability.event_retention_days),
        hook_event_max_lines: requiredDraftNumber(draft.observability.hook_event_max_lines),
        hook_cooldown_max_lines: requiredDraftNumber(draft.observability.hook_cooldown_max_lines),
        log_max_bytes: requiredDraftNumber(draft.observability.log_max_bytes),
      },
    },
    ...(deviceKey ? { notifications: { device_key: deviceKey } } : {}),
  };
}

export type ProbeNotificationPlatformId = "codex" | "claude_code" | "grok" | "grokbot" | "antigravity" | "dsh";

export const PROBE_NOTIFICATION_PLATFORMS: readonly {
  id: ProbeNotificationPlatformId;
  brand: "codex" | "claude" | "grok" | "grokbot" | "antigravity" | "dsh";
  label: string;
}[] = [
  { id: "codex", brand: "codex", label: "Codex" },
  { id: "claude_code", brand: "claude", label: "Claude Code" },
  { id: "grok", brand: "grok", label: "Grok Build" },
  { id: "grokbot", brand: "grokbot", label: "Grok Bot" },
  { id: "antigravity", brand: "antigravity", label: "Antigravity" },
  { id: "dsh", brand: "dsh", label: "DeepSeek Harness" },
];

export function isPlatformNotificationEnabled(
  draft: ProbeSettingsDraft,
  platform: ProbeNotificationPlatformId,
): boolean {
  const n = draft.notifications;
  switch (platform) {
    case "codex":
      return n.notify_codex && (n.notify_completion || n.notify_reply_needed || n.notify_recoverable);
    case "claude_code":
      return n.notify_claude && (n.notify_claude_completion || n.notify_claude_failure || n.notify_claude_reply_needed);
    case "grok":
      return n.notify_grok && (n.notify_grok_completion || n.notify_grok_failure || n.notify_grok_reply_needed);
    case "grokbot":
      return n.notify_grokbot;
    case "antigravity":
      return (
        n.notify_antigravity &&
        (n.notify_antigravity_completion ||
          n.notify_antigravity_failure ||
          n.notify_antigravity_reply_needed ||
          n.notify_antigravity_authorization)
      );
    case "dsh":
      return (
        n.notify_dsh &&
        (n.notify_dsh_completion || n.notify_dsh_failure || n.notify_dsh_reply_needed || n.notify_dsh_authorization)
      );
  }
}

export function applyPlatformNotificationToggle(
  draft: ProbeSettingsDraft,
  platform: ProbeNotificationPlatformId,
  enabled: boolean,
): ProbeSettingsDraft {
  const patchMap: Record<ProbeNotificationPlatformId, Partial<ProbeSettingsDraft["notifications"]>> = {
    codex: {
      notify_codex: enabled,
      notify_completion: enabled,
      notify_reply_needed: enabled,
      notify_recoverable: enabled,
    },
    claude_code: {
      notify_claude: enabled,
      notify_claude_completion: enabled,
      notify_claude_failure: enabled,
      notify_claude_reply_needed: enabled,
    },
    grok: {
      notify_grok: enabled,
      notify_grok_completion: enabled,
      notify_grok_failure: enabled,
      notify_grok_reply_needed: enabled,
    },
    grokbot: { notify_grokbot: enabled },
    antigravity: {
      notify_antigravity: enabled,
      notify_antigravity_completion: enabled,
      notify_antigravity_failure: enabled,
      notify_antigravity_reply_needed: enabled,
      notify_antigravity_authorization: enabled,
    },
    dsh: {
      notify_dsh: enabled,
      notify_dsh_completion: enabled,
      notify_dsh_failure: enabled,
      notify_dsh_reply_needed: enabled,
      notify_dsh_authorization: enabled,
    },
  };
  return {
    ...draft,
    probe:
      platform === "codex"
        ? { ...draft.probe, error_monitor: { ...draft.probe.error_monitor, enabled } }
        : draft.probe,
    notifications: { ...draft.notifications, ...patchMap[platform] },
  };
}

export function probeSettingsValidation(draft: ProbeSettingsDraft): string[] {
  const errors: string[] = [];
  if (!draft.codex.host_label.trim()) errors.push("主机标签不能为空");
  if (!isIntegerInRange(draft.probe.poll_seconds, 5, 3600)) errors.push("轮询间隔必须在 5 到 3600 秒之间");
  if (!isIntegerInRange(draft.probe.recent_limit, 1, 500)) errors.push("最近事件数量必须在 1 到 500 之间");
  if (!isIntegerInRange(draft.observability.hook_event_max_lines, 1, 5000)) {
    errors.push("Hook 事件行数必须在 1 到 5000 之间");
  }
  if (!isIntegerInRange(draft.observability.hook_cooldown_max_lines, 1, 5000)) {
    errors.push("Hook 静默期行数必须在 1 到 5000 之间");
  }
  if (!isIntegerInRange(draft.observability.log_max_bytes, 1024, 10485760)) {
    errors.push("日志读取上限必须在 1024 到 10485760 字节之间");
  }
  if (!isIntegerInRange(draft.observability.event_retention_days, 1, 3650)) {
    errors.push("通知事件保留天数必须在 1 到 3650 天之间");
  }
  if (!draft.notifications.server_url.trim()) errors.push("Bark 服务 URL 不能为空");
  return errors;
}

function toBoundedInteger(value: unknown, fallback: number): number | null {
  const numeric = Number(value ?? fallback);
  return Number.isFinite(numeric) ? Math.trunc(numeric) : null;
}

export function probeNumberInputDraftValue(value: string): ProbeNumericDraftValue {
  const trimmed = value.trim();
  if (!trimmed) return "";
  const numeric = Number(trimmed);
  return Number.isFinite(numeric) ? Math.trunc(numeric) : "";
}

export type ProbeEventDisplay = {
  title: string;
  summary: string;
  bark: string;
  dedupe: string;
  source: string;
  time: string;
};

export type ProbeEventCardTone = "success" | "warning" | "danger" | "muted";

export type ProbeEventCard = {
  title: string;
  headline: string;
  summary: string;
  reason: string;
  source: string;
  time: string;
  bark: { label: string; tone: ProbeEventCardTone };
  dedupe: { label: string; tone: ProbeEventCardTone };
  details: Array<{ label: string; value: string }>;
};

export function probeEventDisplay(event: ProbeEvent): ProbeEventDisplay {
  const card = probeEventCard(event);
  return {
    title: card.title,
    summary: card.summary,
    bark: card.bark.label,
    dedupe: card.dedupe.label,
    source: card.source,
    time: card.time,
  };
}

export function probeEventCard(event: ProbeEvent): ProbeEventCard {
  const payload = probeEventPayload(event);
  const eventType = cleanProbeEventText(stringFromRecord(payload, "event_type"));
  const source = cleanProbeEventText(stringFromRecord(payload, "source")) || cleanProbeEventText(event.source) || "未知来源";
  const threadId = cleanProbeEventText(stringFromRecord(payload, "thread_id")) || cleanProbeEventText(event.thread_id);
  const turnId = cleanProbeEventText(stringFromRecord(payload, "turn_id"));
  const bodyLength = numberFromRecord(payload, "body_length");
  const bodySha = cleanProbeEventText(stringFromRecord(payload, "body_sha256"));
  const bodySource = cleanProbeEventText(stringFromRecord(payload, "body_source"));
  const bodyTruncated = payload.body_truncated === true;
  const bark = recordFromRecord(payload, "bark");
  const reason = cleanProbeEventText(stringFromRecord(payload, "reason_label")) || cleanProbeEventText(stringFromRecord(payload, "reason"));
  const summary = structuredProbeEventSummary(event, payload);
  const details: Array<{ label: string; value: string }> = [];

  const provider = stringFromRecord(payload, "provider");
  if (provider) details.push({ label: "来源应用", value: provider });
  const sessionKey = stringFromRecord(payload, "session_key");
  if (sessionKey) details.push({ label: "会话定位键", value: sessionKey });
  if (threadId) details.push({ label: "线程", value: threadId });
  if (turnId) details.push({ label: "Turn", value: turnId });
  if (bodyLength !== null || bodySha) {
    details.push({
      label: "Body",
      value: [
        bodyLength !== null ? `${bodyLength} bytes` : "",
        bodySha ? `sha256 ${bodySha}` : "",
        bodyTruncated ? "已截断" : "",
        bodySource,
      ].filter(Boolean).join(" · "),
    });
  }
  const barkDetail = probeEventBarkDetail(bark);
  if (barkDetail) details.push({ label: "Bark", value: barkDetail });
  details.push({ label: "来源", value: source });

  return {
    title: probeEventKindLabel(eventType || event.kind),
    headline:
      cleanProbeEventText(stringFromRecord(payload, "thread_title")) ||
      cleanProbeEventText(event.title) ||
      eventType ||
      cleanProbeEventText(event.kind) ||
      "Probe 事件",
    summary,
    reason,
    source,
    time: cleanProbeEventText(stringFromRecord(payload, "beijing_time")) || probeEventTimeLabel(event.created_at),
    bark: probeEventBarkBadge(event),
    dedupe: probeEventDedupeBadge(event),
    details,
  };
}

function probeEventBarkDetail(bark: Record<string, unknown> | null): string {
  if (!bark) return "";
  const title = cleanProbeEventText(stringFromRecord(bark, "title"));
  const chunkCount = numberFromRecord(bark, "chunk_count");
  const requestCount = numberFromRecord(bark, "request_count");
  const confirmed = numberFromRecord(bark, "confirmed_chunks");
  const attempt = numberFromRecord(bark, "attempts");
  const next = numberFromRecord(bark, "next_retry_ms");
  return [
    title,
    chunkCount !== null && chunkCount > 1
      ? confirmed !== null
        ? `${confirmed}/${chunkCount} 段已受理`
        : `${chunkCount} 段`
      : "",
    attempt !== null && attempt > 1 ? `第 ${attempt} 次尝试` : "",
    next !== null ? `下次重试 ${new Date(next).toLocaleTimeString()}` : "",
    requestCount !== null && requestCount > 1 ? `${requestCount} 请求` : "",
  ].filter(Boolean).join(" · ");
}

export function probeEventReadableSummary(event: ProbeEvent): string {
  return structuredProbeEventSummary(event, probeEventPayload(event));
}

export function probeEventBarkStatus(event: ProbeEvent): string {
  return probeEventBarkBadge(event).label;
}

export function probeEventBarkBadge(event: ProbeEvent): { label: string; tone: ProbeEventCardTone } {
  const bark = recordFromRecord(probeEventPayload(event), "bark");
  if (!bark) return { label: "Bark 未记录", tone: "muted" };
  const status = stringFromRecord(bark, "status");
  if (status === "waiting_retry") return { label: "Bark 等待重试", tone: "warning" };
  if (status === "retrying") return { label: "Bark 重试中", tone: "warning" };
  if (bark.sent === true || status === "sent") return { label: "Bark 已受理", tone: "success" };
  if (bark.skipped === true || status === "skipped") return { label: "Bark 已跳过", tone: "muted" };
  const reason = stringFromRecord(bark, "reason") ?? "";
  const unknown =
    status === "unknown" ||
    (!status &&
      [
        "request_error",
        "network_unknown",
        "timeout",
        "response_decode",
        "response_read",
        "delivery_interrupted_outcome_unknown",
      ].includes(reason));
  const categories: Record<string, string> = {
    dns: "域名解析失败",
    connection: "连接失败",
    tls: "TLS 连接失败",
    certificate_validation: "证书验证失败",
    timeout: "请求超时",
    response_decode: "响应无法确认",
    response_read: "响应读取失败",
    redirect_rejected: "重定向已拒绝",
    http_status: "HTTP 拒绝",
    bark_response_code: "服务拒绝",
    apns_permanent_rejection: "推送服务永久拒绝",
    invalid_server_url: "地址无效",
    network_unknown: "网络错误",
    request_error: "网络错误",
  };
  const detail = categories[reason];
  return { label: `Bark ${unknown ? "结果未确认" : "发送失败"}${detail ? ` · ${detail}` : ""}`, tone: "warning" };
}

export function probeEventDedupeStatus(event: ProbeEvent): string {
  return probeEventDedupeBadge(event).label;
}

export function probeEventDedupeBadge(event: ProbeEvent): { label: string; tone: ProbeEventCardTone } {
  const payload = probeEventPayload(event);
  const dedupe = recordFromRecord(payload, "dedupe");
  if (dedupe?.duplicate === true) return { label: "重复事件", tone: "warning" };
  if (dedupe?.claimed === true) return { label: "已认领", tone: "success" };
  const dedupeStatus = cleanProbeEventText(stringFromRecord(dedupe ?? {}, "status"));
  if (dedupeStatus) {
    return { label: dedupeStatus, tone: dedupeStatus.toLowerCase().includes("duplicate") ? "warning" : "muted" };
  }
  if (payload.duplicate === true) return { label: "重复事件", tone: "warning" };
  const outcome = recordFromRecord(payload, "probe_event") ?? recordFromRecord(payload, "outcome");
  if (outcome?.duplicate === true) return { label: "重复事件", tone: "warning" };
  if (outcome?.recorded === true) return { label: "已记录", tone: "success" };
  return event.dedupe_key?.trim() ? { label: "去重键已记录", tone: "muted" } : { label: "无去重键", tone: "muted" };
}

export function probeEventKindLabel(kind?: string | null): string {
  const normalized = kind?.trim();
  const labels: Record<string, string> = {
    "hook-stop": "Stop Hook",
    hook_stop: "Stop Hook",
    completion: "完成",
    failure: "失败",
    "reply-needed": "需回复",
    reply_needed: "需回复",
    recoverable: "异常/可恢复",
    running: "运行中",
    "bark-test": "Bark 测试",
  };
  return normalized ? labels[normalized] ?? normalized : "Probe 事件";
}

function isIntegerInRange(value: ProbeNumericDraftValue, min: number, max: number): value is number {
  return typeof value === "number" && Number.isInteger(value) && value >= min && value <= max;
}

function requiredDraftNumber(value: ProbeNumericDraftValue): number {
  if (typeof value !== "number") throw new Error("探针数字设置无效");
  return value;
}

function stringOrEmpty(value: unknown): string {
  return typeof value === "string" ? value : "";
}

function configuredCodexHomeDraftValue(settings: ProbeSettings): string {
  const codex = (settings.codex ?? {}) as ProbeSettings["codex"] & Record<string, unknown>;
  if (Object.prototype.hasOwnProperty.call(codex, "configured_codex_home")) {
    return stringOrEmpty(codex.configured_codex_home);
  }
  return stringOrEmpty(codex.home);
}

function codexHomePatchValue(value: string): string | null {
  const trimmed = value.trim();
  return !trimmed || trimmed.toLowerCase() === "auto" ? null : trimmed;
}

function optionalString(value: string): string | null {
  const trimmed = value.trim();
  return trimmed ? trimmed : null;
}

function probeEventPayload(event: ProbeEvent): Record<string, unknown> {
  return event.payload && typeof event.payload === "object" ? event.payload : {};
}

function recordFromRecord(record: Record<string, unknown>, key: string): Record<string, unknown> | null {
  const value = record[key];
  return value && typeof value === "object" && !Array.isArray(value) ? (value as Record<string, unknown>) : null;
}

function stringFromRecord(record: Record<string, unknown>, key: string): string | null {
  const value = record[key];
  return typeof value === "string" ? value : null;
}

function numberFromRecord(record: Record<string, unknown>, key: string): number | null {
  const value = record[key];
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

function cleanProbeEventText(value?: string | null): string {
  return value?.replace(/\s+/g, " ").trim() ?? "";
}

function compactProbeEventSummary(value: string): string {
  return value.length > 240 ? `${value.slice(0, 240).trimEnd()}...` : value;
}

function structuredProbeEventSummary(event: ProbeEvent, payload: Record<string, unknown>): string {
  const candidates = [
    stringFromRecord(payload, "body_summary"),
    stringFromRecord(payload, "reason_label"),
    stringFromRecord(payload, "summary"),
    stringFromRecord(payload, "status"),
    stringFromRecord(payload, "reason"),
    stringFromRecord(payload, "kind"),
    event.message,
    event.title,
  ];
  const kind = cleanProbeEventText(stringFromRecord(payload, "event_type") || event.kind).toLowerCase();
  const kindLabel = probeEventKindLabel(kind).toLowerCase();
  const summary = candidates
    .map(cleanProbeEventText)
    .find(candidate => {
      if (!candidate) return false;
      const normalized = candidate.toLowerCase();
      return normalized !== kind && normalized !== kindLabel;
    });
  if (summary) return compactProbeEventSummary(summary);
  return event.thread_id ? `线程 ${event.thread_id}` : "Probe 事件已记录";
}

function probeEventTimeLabel(value: string | number): string {
  if (typeof value === "number") {
    const millis = value > 10_000_000_000 ? value : value * 1000;
    const date = new Date(millis);
    return Number.isNaN(date.getTime()) ? String(value) : date.toISOString();
  }
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? value : date.toISOString();
}
