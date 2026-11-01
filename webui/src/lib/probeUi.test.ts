import { describe, expect, test } from "vitest";
import type { ProbeEvent, ProbeSettings } from "../types";
import {
  applyPlatformNotificationToggle,
  buildProbeSettingsDraft,
  buildProbeSettingsPayload,
  isPlatformNotificationEnabled,
  PROBE_NAV_LABEL,
  probeEventCard,
  probeEventDisplay,
  probeNumberInputDraftValue,
  probeSections,
  probeSettingsValidation,
} from "./probeUi";

const settings: ProbeSettings = {
  codex: {
    home: "/root/.codex",
    workspace: "/home/ubuntu/codex-workspace",
    host_label: "192.0.2.10"
  } as ProbeSettings["codex"],
  probe: {
    enabled: true,
    poll_seconds: 15,
    recent_limit: 50,
    hooks: {
      manage_stop_hook: true
    },
    error_monitor: {
      enabled: true,
    },
    notifications: {
      enabled: true,
      server_url: "https://api.day.app",
      sound: "bell",
      group: "AgentHub",
      url: "https://panel.example.com/nexushub/",
      notify_completion: true,
      notify_reply_needed: true,
      notify_recoverable: true
    },
    observability: {
        event_retention_days: 2,
      hook_event_max_lines: 120,
      hook_cooldown_max_lines: 80,
      log_max_bytes: 262144
    },
  },
  notifications: {
    enabled: true,
    device_key_configured: true,
    server_url: "https://api.day.app",
    sound: "bell",
    group: "AgentHub",
    url: "https://panel.example.com/nexushub/",
    notify_completion: true,
    notify_reply_needed: true,
    notify_recoverable: true
  },
};

describe("Probe UI helpers", () => {
  test("uses Chinese probe labels and a single unified settings section without redundant sub-tabs", () => {
    expect(PROBE_NAV_LABEL).toBe("Probe");
    expect(probeSections.map((section) => section.id)).toEqual(["settings"]);
    expect(probeSections.map((section) => section.label)).toEqual(["通知配置"]);
  });

  test("builds readable event display details without exposing secret payload values", () => {
    const event: ProbeEvent = {
      id: "event-a",
      kind: "reply-needed",
      thread_id: "thread-a",
      title: "Codex Reply Needed",
      message: "Need operator input",
      dedupe_key: "reply-needed:thread-a:turn-a",
      source: "nexushub-webd probe passive-scan",
      payload: {
        bark: {
          sent: false,
          skipped: true,
          reason: "dedupe",
          device_key_configured: true,
          dedupe_key: "reply-needed:thread-a:turn-a"
        },
        duplicate: true,
        session_id: "session-a",
        device_key: "secret-device-key"
      },
      created_at: "2026-06-15T00:00:00Z"
    };

    const display = probeEventDisplay(event);

    expect(display.title).toBe("需回复");
    expect(display.summary).toContain("Need operator input");
    expect(display.bark).toBe("Bark 已跳过");
    expect(display.dedupe).toBe("重复事件");
    expect(display.source).toBe("nexushub-webd probe passive-scan");
    expect(display.time).toContain("2026-06-15");
    expect(JSON.stringify(display)).not.toContain("secret-device-key");
  });

  test("event summaries do not repeat raw kind or duplicate title when payload has context", () => {
    const event: ProbeEvent = {
      id: "event-summary",
      kind: "reply-needed",
      thread_id: "thread-a",
      title: "reply-needed",
      message: "",
      dedupe_key: "reply-needed:thread-a:turn-a",
      source: "nexushub-webd probe passive-scan",
      payload: {
        summary: "Plan Mode 等待用户确认",
        status: "reply-needed"
      },
      created_at: "2026-06-15T00:00:00Z"
    };

    expect(probeEventDisplay(event).summary).toBe("Plan Mode 等待用户确认");
  });

  test("builds rich structured event cards from payload fields before raw message or title", () => {
    const event: ProbeEvent = {
      id: "event-structured",
      kind: "legacy-kind",
      thread_id: "fallback-thread",
      title: "Raw event title",
      message: "Raw event message",
      dedupe_key: "reply-needed:thread-a:turn-a",
      source: "raw event source",
      payload: {
        event_type: "reply-needed",
        thread_title: "Plan Mode 修复",
        thread_id: "thread-a",
        turn_id: "turn-a",
        beijing_time: "2026-06-16 09:30:00 北京时间",
        reason_label: "等待用户确认",
        body_summary: "Plan Mode 等待用户确认",
        body_sha256: "abc123",
        body_length: 324,
        source: "nexushub-webd probe passive-scan",
        bark: {
          title: "等待回复：Plan Mode 修复",
          sent: true,
          skipped: false,
          http_status: 200,
          dedupe_hit: false,
          chunk_count: 4,
          request_count: 4
        },
        dedupe: {
          claimed: true,
          duplicate: false,
          status: "claimed"
        },
        device_key: "secret-device-key",
        bark_device_key: "secret-bark-key",
        token: "secret-token"
      },
      created_at: "2026-06-16T01:30:00Z"
    };

    const card = probeEventCard(event);

    expect(card).toMatchObject({
      title: "需回复",
      headline: "Plan Mode 修复",
      summary: "Plan Mode 等待用户确认",
      reason: "等待用户确认",
      source: "nexushub-webd probe passive-scan",
      time: "2026-06-16 09:30:00 北京时间",
      bark: { label: "Bark 已受理", tone: "success" },
      dedupe: { label: "已认领", tone: "success" }
    });
    expect(card.details).toEqual(expect.arrayContaining([
      { label: "线程", value: "thread-a" },
      { label: "Turn", value: "turn-a" },
      { label: "Body", value: "324 bytes · sha256 abc123" },
      { label: "Bark", value: "等待回复：Plan Mode 修复 · 4 段 · 4 请求" }
    ]));
    expect(card.summary).not.toBe("Raw event message");
    expect(JSON.stringify(card)).not.toContain("secret-device-key");
    expect(JSON.stringify(card)).not.toContain("secret-bark-key");
    expect(JSON.stringify(card)).not.toContain("secret-token");
  });

  test("event cards show safe Bark chunk metadata without rendering stored body fields", () => {
    const event: ProbeEvent = {
      id: "event-bark-body",
      kind: "completion",
      thread_id: "thread-safe",
      source: "nexushub-webd probe notify-completion",
      payload: {
        event_type: "completion",
        thread_title: "完整反馈测试",
        body_summary: "安全摘要",
        body_sha256: "hash",
        body_length: 12000,
        body_source: "task_complete.last_agent_message",
        body_truncated: true,
        bark: {
          title: "线程正常完成：完整反馈测试",
          body: "完整正文不应出现在卡片",
          sent: true,
          http_status: 200,
          chunk_count: 6,
          request_count: 6
        }
      },
      created_at: "2026-06-16T00:00:00Z"
    };

    const card = probeEventCard(event);

    expect(card.summary).toBe("安全摘要");
    expect(card.details).toEqual(expect.arrayContaining([
      { label: "Body", value: "12000 bytes · sha256 hash · 已截断 · task_complete.last_agent_message" },
      { label: "Bark", value: "线程正常完成：完整反馈测试 · 6 段 · 6 请求" }
    ]));
    expect(JSON.stringify(card)).not.toContain("完整正文不应出现在卡片");
  });

  test("event card summary is compact even when backend keeps a long safe summary", () => {
    const event: ProbeEvent = {
      id: "event-long-summary",
      kind: "completion",
      thread_id: "thread-a",
      source: "nexushub-webd probe notify-completion",
      payload: {
        event_type: "completion",
        body_summary: "长正文片段 ".repeat(80),
        body_length: 4096,
        body_sha256: "abc123"
      },
      created_at: "2026-06-16T00:00:00Z"
    };

    const card = probeEventCard(event);

    expect(card.summary.length).toBeLessThanOrEqual(243);
    expect(card.summary).toContain("...");
  });

  test("marks skipped Bark separately from duplicate dedupe outcomes", () => {
    const event: ProbeEvent = {
      id: "event-duplicate",
      kind: "hook-stop",
      source: "raw source",
      payload: {
        event_type: "completion",
        body_summary: "任务已完成",
        bark: { skipped: true, reason: "dedupe", dedupe_hit: true },
        dedupe: { claimed: false, duplicate: true, status: "duplicate" }
      },
      created_at: "2026-06-16T00:00:00Z"
    };

    const card = probeEventCard(event);

    expect(card.title).toBe("完成");
    expect(card.bark).toEqual({ label: "Bark 已跳过", tone: "muted" });
    expect(card.dedupe).toEqual({ label: "重复事件", tone: "warning" });
  });

  test.each([
    ["waiting_retry", "Bark 等待重试", "warning"],
    ["retrying", "Bark 重试中", "warning"],
    ["sent", "Bark 已受理", "success"],
    ["failed", "Bark 发送失败", "warning"],
    ["unknown", "Bark 结果未确认", "warning"],
    ["skipped", "Bark 已跳过", "muted"]
  ])("renders the %s delivery state without inventing device receipt", (status, label, tone) => {
    const event: ProbeEvent = {
      id: `event-${status}`,
      kind: "completion",
      source: "fixture-source",
      payload: { body_summary: "安全摘要", bark: { status } },
      created_at: "2026-06-16T00:00:00Z"
    };

    expect(probeEventCard(event).bark).toEqual({ label, tone });
    expect(probeEventDisplay(event).bark).toBe(label);
  });

  test.each([
    ["dns", "域名解析失败"],
    ["connection", "连接失败"],
    ["tls", "TLS 连接失败"],
    ["certificate_validation", "证书验证失败"],
    ["timeout", "请求超时"],
    ["response_decode", "响应无法确认"],
    ["response_read", "响应读取失败"],
    ["redirect_rejected", "重定向已拒绝"],
    ["http_status", "HTTP 拒绝"],
    ["bark_response_code", "服务拒绝"],
    ["apns_permanent_rejection", "推送服务永久拒绝"]
  ])("maps the %s failure reason to a fixed safe label", (reason, description) => {
    const event: ProbeEvent = {
      id: "event-safe-reason",
      kind: "completion",
      source: "fixture-source",
      payload: { body_summary: "安全摘要", bark: { status: "failed", reason } },
      created_at: "2026-06-16T00:00:00Z"
    };

    expect(probeEventCard(event).bark.label).toBe(`Bark 发送失败 · ${description}`);
  });

  test("shows confirmed chunks and the next attempt without exposing pending payloads", () => {
    const event: ProbeEvent = {
      id: "event-retry-details",
      kind: "completion",
      source: "fixture-source",
      payload: {
        body_summary: "安全摘要",
        bark: {
          status: "waiting_retry",
          title: "完成通知",
          chunk_count: 4,
          confirmed_chunks: 2,
          attempts: 2,
          request_count: 5,
          next_retry_ms: Date.parse("2026-06-16T00:01:00Z"),
          body: "fixture private pending body",
          device_key: "fixture private device key",
          response_body: "fixture raw response body",
          request_url: "https://push.example.invalid/private-key"
        }
      },
      created_at: "2026-06-16T00:00:00Z"
    };

    const card = probeEventCard(event);
    const detail = card.details.find((item) => item.label === "Bark")?.value;
    expect(card.bark.label).toBe("Bark 等待重试");
    expect(detail).toContain("2/4 段已受理");
    expect(detail).toContain("第 2 次尝试");
    expect(detail).toContain("下次重试");
    expect(detail).toContain("5 请求");
    for (const privateValue of [
      "fixture private pending body", "fixture private device key", "fixture raw response body",
      "https://push.example.invalid/private-key"
    ]) {
      expect(JSON.stringify(card)).not.toContain(privateValue);
    }
  });

  test("unknown raw reasons never become visible error text", () => {
    const rawReason = "fixture transport failure https://push.example.invalid/private-key fixture-device-key";
    for (const status of ["waiting_retry", "retrying", "failed", "unknown", "skipped"]) {
      const event: ProbeEvent = {
        id: `event-private-${status}`,
        kind: "completion",
        source: "fixture-source",
        payload: { body_summary: "安全摘要", bark: { status, reason: rawReason } },
        created_at: "2026-06-16T00:00:00Z"
      };

      const card = probeEventCard(event);
      expect(JSON.stringify(card)).not.toContain(rawReason);
      expect(JSON.stringify(card)).not.toContain("private-key");
      expect(JSON.stringify(card)).not.toContain("fixture-device-key");
    }
  });

  test("builds a form draft and leaves configured Bark device_key unchanged when blank", () => {
    const draft = buildProbeSettingsDraft(settings);
    expect(draft.notifications.device_key).toBe("");

    const payload = buildProbeSettingsPayload(draft, settings);
    expect(payload).not.toHaveProperty("notifications");
    expect(payload).not.toHaveProperty("logs_db");
    expect(payload.probe?.notifications).not.toHaveProperty("device_key");
    expect(payload).toEqual({
      codex: {
        home: "/root/.codex",
        workspace: "/home/ubuntu/codex-workspace",
        host_label: "192.0.2.10"
      },
      probe: {
        enabled: true,
        poll_seconds: 15,
        recent_limit: 50,
        error_monitor: {
          enabled: true,
        },
        hooks: {
          manage_stop_hook: true
        },
        notifications: {
          enabled: true,
          server_url: "https://api.day.app",
          sound: "bell",
          group: "AgentHub",
          url: "https://panel.example.com/nexushub/",
          notify_completion: true,
          notify_reply_needed: true,
          notify_codex: true,
          notify_grok: true,
          notify_grok_completion: true,
          notify_grok_failure: true,
          notify_grok_reply_needed: true,
          notify_claude: true,
          notify_claude_completion: true,
          notify_claude_failure: true,
          notify_claude_reply_needed: true,
          notify_antigravity: false,
          notify_antigravity_completion: false,
          notify_antigravity_failure: false,
          notify_antigravity_reply_needed: false,
          notify_antigravity_authorization: false,
          notify_dsh: false,
          notify_dsh_completion: false,
          notify_dsh_failure: false,
          notify_dsh_reply_needed: false,
          notify_dsh_authorization: false,

          notify_recoverable: true
        },
        observability: {
        event_retention_days: 2,
          hook_event_max_lines: 120,
          hook_cooldown_max_lines: 80,
          log_max_bytes: 262144
        },
      }
    });
  });

  test("uses Mac app-compatible defaults when Probe settings omit optional numbers", () => {
    const minimal: ProbeSettings = {
      codex: {
        home: "/root/.codex",
        host_label: "cloud"
      } as ProbeSettings["codex"],
      probe: {},
      notifications: {},
    };

    expect(buildProbeSettingsDraft(minimal)).toMatchObject({
      notifications: { server_url: "https://api.day.app" },
      observability: {
        event_retention_days: 2,
        hook_event_max_lines: 500,
        hook_cooldown_max_lines: 1000,
        log_max_bytes: 5 * 1024 * 1024
      },
    });
  });

  test("uses configured Codex Home for the draft instead of the resolved default", () => {
    const autoResolvedSettings: ProbeSettings = {
      ...settings,
      codex: {
        ...settings.codex,
        home: "/root/.codex",
        configured_codex_home: null,
        resolved_codex_home: "/root/.codex",
        codex_home_source: "auto"
      }
    };
    expect(buildProbeSettingsDraft(autoResolvedSettings).codex.home).toBe("");

    const configuredSettings: ProbeSettings = {
      ...settings,
      codex: {
        ...settings.codex,
        home: "/root/.codex",
        configured_codex_home: "/srv/codex-home",
        resolved_codex_home: "/root/.codex",
        codex_home_source: "config"
      }
    };
    expect(buildProbeSettingsDraft(configuredSettings).codex.home).toBe("/srv/codex-home");
  });

  test("draft codex fields only expose local state paths and host metadata", () => {
    const draft = buildProbeSettingsDraft(settings);

    expect(draft.codex).toEqual({
      home: "/root/.codex",
      workspace: "/home/ubuntu/codex-workspace",
      host_label: "192.0.2.10"
    });
    expect(draft.codex).not.toHaveProperty("app_server_socket");
    expect(draft.codex).not.toHaveProperty("app_server_service");
    expect(draft.codex).not.toHaveProperty("bridge_enabled");
  });

  test("allows blank or auto Codex Home and serializes it as automatic discovery", () => {
    const blankDraft = buildProbeSettingsDraft(settings);
    blankDraft.codex.home = "   ";
    expect(probeSettingsValidation(blankDraft)).not.toContain("Codex Home 不能为空");
    expect(buildProbeSettingsPayload(blankDraft, settings).codex.home).toBeNull();

    const autoDraft = buildProbeSettingsDraft(settings);
    autoDraft.codex.home = "auto";
    expect(probeSettingsValidation(autoDraft)).not.toContain("Codex Home 不能为空");
    expect(buildProbeSettingsPayload(autoDraft, settings).codex.home).toBeNull();
  });

  test("builds a safe draft from partial desktop Probe settings", () => {
    const partialSettings = {
      codex: {
        host_label: "example-host"
      }
    } as ProbeSettings;

    expect(() => buildProbeSettingsDraft(partialSettings)).not.toThrow();
    expect(buildProbeSettingsDraft(partialSettings)).toMatchObject({
      codex: {
        home: "",
        workspace: "",
        host_label: "example-host"
      },
      probe: {
        enabled: false,
        poll_seconds: 15,
        recent_limit: 50,
        error_monitor: {
          enabled: true,
        }
      },
      notifications: {
        enabled: false,
        device_key_configured: false,
        server_url: "https://api.day.app",
        group: "AgentHub"
      },
    });
  });

  test("omits a blank Bark device_key whether or not a key is already configured", () => {
    const configuredDraft = buildProbeSettingsDraft(settings);
    configuredDraft.notifications.device_key = "   ";
    expect(buildProbeSettingsPayload(configuredDraft, settings).probe?.notifications).not.toHaveProperty("device_key");

    const unconfiguredSettings: ProbeSettings = {
      ...settings,
      notifications: { ...settings.notifications, device_key_configured: false }
    };
    const unconfiguredDraft = buildProbeSettingsDraft(unconfiguredSettings);
    unconfiguredDraft.notifications.device_key = "";
    expect(buildProbeSettingsPayload(unconfiguredDraft, unconfiguredSettings).probe?.notifications).not.toHaveProperty("device_key");
  });

  test("enables Bark when saving a new Device Key from the slim card", () => {
    const unconfiguredSettings = {
      ...settings,
      notifications: { ...settings.notifications, enabled: false, device_key_configured: false }
    };
    const draft = buildProbeSettingsDraft(unconfiguredSettings);
    draft.notifications.device_key = "new-device-key";

    expect(buildProbeSettingsPayload(draft, unconfiguredSettings).probe.notifications).toMatchObject({
      enabled: true,
      device_key: "new-device-key"
    });
    expect(buildProbeSettingsPayload(draft, unconfiguredSettings).notifications).toMatchObject({
      device_key: "new-device-key"
    });
  });

  test("a configured Device Key does not override an explicit Bark disable", () => {
    const draft = buildProbeSettingsDraft(settings);
    draft.notifications.enabled = false;
    draft.notifications.device_key = "";
    const payload = buildProbeSettingsPayload(draft, settings);
    expect(payload.probe.notifications?.enabled).toBe(false);
    const reloaded = buildProbeSettingsDraft({
      ...settings,
      notifications: { ...settings.notifications, enabled: false, device_key_configured: true }
    });
    expect(reloaded.notifications.enabled).toBe(false);
    expect(reloaded.notifications.device_key_configured).toBe(true);
  });

  test("uses the submitted Bark Device Key when the React draft is stale", () => {
    const unconfiguredSettings = {
      ...settings,
      notifications: { ...settings.notifications, enabled: false, device_key_configured: false }
    };
    const staleDraft = buildProbeSettingsDraft(unconfiguredSettings);
    staleDraft.notifications.device_key = "";

    const payload = buildProbeSettingsPayload(staleDraft, unconfiguredSettings, " submitted-device-key ");

    expect(payload.probe.notifications).toMatchObject({
      enabled: true,
      device_key: "submitted-device-key"
    });
    expect(payload.notifications).toMatchObject({
      device_key: "submitted-device-key"
    });
  });

  test("keeps blank numeric input as a blank draft value until validation blocks save", () => {
    const draft = buildProbeSettingsDraft(settings);
    draft.probe.poll_seconds = probeNumberInputDraftValue("");
    draft.observability.log_max_bytes = probeNumberInputDraftValue("abc");
    draft.observability.event_retention_days = probeNumberInputDraftValue("3");

    expect(draft.probe.poll_seconds).toBe("");
    expect(draft.observability.log_max_bytes).toBe("");
    expect(draft.observability.event_retention_days).toBe(3);
    expect(probeSettingsValidation(draft)).toEqual([
      "轮询间隔必须在 5 到 3600 秒之间",
      "日志读取上限必须在 1024 到 10485760 字节之间"
    ]);
    expect(() => buildProbeSettingsPayload(draft, settings)).toThrow("轮询间隔必须在 5 到 3600 秒之间");
  });

  test("validates numeric settings before save", () => {
    const draft = buildProbeSettingsDraft(settings);
    draft.probe.poll_seconds = 0;
    draft.probe.recent_limit = 501;
    draft.observability.event_retention_days = 0;

    expect(probeSettingsValidation(draft)).toEqual([
      "轮询间隔必须在 5 到 3600 秒之间",
      "最近事件数量必须在 1 到 500 之间",
      "通知事件保留天数必须在 1 到 3650 天之间"
    ]);
  });

});

test("local providers default off and all four categories survive a settings round trip", () => {
  const draft = buildProbeSettingsDraft(settings);
  expect(draft.notifications.notify_antigravity).toBe(false);
  expect(draft.notifications.notify_dsh).toBe(false);
  for (const provider of ["antigravity", "dsh"] as const) {
    for (const suffix of ["", "_completion", "_failure", "_reply_needed", "_authorization"] as const) {
      draft.notifications[`notify_${provider}${suffix}`] = true;
    }
  }
  const payload = buildProbeSettingsPayload(draft);
  for (const provider of ["antigravity", "dsh"] as const) {
    for (const suffix of ["", "_completion", "_failure", "_reply_needed", "_authorization"] as const) {
      expect(payload.probe.notifications?.[`notify_${provider}${suffix}`]).toBe(true);
    }
  }
});

test("one-click platform notification switches toggle all sub-events and grok reply_needed", () => {
  let draft = buildProbeSettingsDraft(settings);
  expect(draft.notifications.notify_grok_reply_needed).toBe(true);
  expect(isPlatformNotificationEnabled(draft, "grok")).toBe(true);

  draft = applyPlatformNotificationToggle(draft, "grok", false);
  expect(isPlatformNotificationEnabled(draft, "grok")).toBe(false);
  expect(draft.notifications.notify_grok).toBe(false);
  expect(draft.notifications.notify_grok_completion).toBe(false);
  expect(draft.notifications.notify_grok_failure).toBe(false);
  expect(draft.notifications.notify_grok_reply_needed).toBe(false);

  draft = applyPlatformNotificationToggle(draft, "antigravity", true);
  expect(isPlatformNotificationEnabled(draft, "antigravity")).toBe(true);
  expect(draft.notifications.notify_antigravity_reply_needed).toBe(true);
  expect(draft.notifications.notify_antigravity_authorization).toBe(true);

  draft = applyPlatformNotificationToggle(draft, "codex", false);
  expect(isPlatformNotificationEnabled(draft, "codex")).toBe(false);
  expect(draft.probe.error_monitor.enabled).toBe(false);
});
