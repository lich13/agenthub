import type {
  AgentProviderInfo,
  CodexConfig,
  HiddenThreadDeletePlan,
  JobRecord,
  MessageBlock,
  OptionalResult,
  PlatformOverview,
  ProbeEventsResponse,
  ProbeSettings,
  ProbeStatus,
  SystemCapabilitiesResponse,
  SystemVersion,
  ThreadBlockPage,
  ThreadDetail,
  ThreadSummary,
  UnifiedCleanupDryRunRequest,
  UnifiedCleanupExecuteRequest,
  UnifiedCleanupPlan,
  UnifiedCleanupResult,
  UpdateStatus
} from "../../types";
import {
  buildDemoPlatformOverview,
  buildDemoSystemCapabilities,
  type DemoFixtureKey
} from "../domain/demoCore";

export function demoPlatformOverview(fixture: DemoFixtureKey = "linux-api"): PlatformOverview {
  return buildDemoPlatformOverview(fixture);
}

export function demoSystemCapabilities(fixture: DemoFixtureKey = "linux-api"): SystemCapabilitiesResponse {
  return buildDemoSystemCapabilities(fixture);
}

export function demoUpdateStatus(fixture: DemoFixtureKey = "linux-api"): UpdateStatus {
  if (fixture === "macos-tauri") {
    return {
      current_version: "0.1.100",
      latest_version: "v0.1.103",
      update_available: true,
      channel: "stable",
      method: "macos_tauri_updater",
      state: "idle",
      failure_category: null,
      recommended_action: "Confirm install in the Tauri updater after signature verification.",
      capabilities: ["check", "confirm_install", "job_history", "signature_verification", "restart_after_install"]
    };
  }
  return {
    current_version: "0.1.100",
    latest_version: "v0.1.103",
    update_available: true,
    channel: "stable",
    method: "linux_systemd_job",
    state: "idle",
    failure_category: null,
    recommended_action: "/usr/local/bin/agenthub-webd-update --repo lich13/agenthub --version latest",
    capabilities: ["check", "confirm_install", "job_history", "sha256_verification", "systemd_health_check", "rollback", "prune_backups"]
  };
}

export function demoSystemVersion(): SystemVersion {
  return {
    panel_current: "0.1.5",
    panel_latest: "v0.1.5",
    panel_update_available: false,
    codex_current: "0.137.0",
    codex_latest: "0.137.0",
    codex_update_available: false,
    codex_user: "codex-cli 0.137.0",
    codex_root: "codex-cli 0.137.0",
    codex_raw: "codex-cli 0.137.0"
  };
}

export function demoCodexConfig(fixture: DemoFixtureKey = "linux-api"): CodexConfig {
  return {
    model: "gpt-5.5",
    service_tier: null,
    reasoning_effort: "xhigh",
    cwd: fixture === "macos-tauri" ? null : "/home/ubuntu/codex-workspace",
    permission_profile: "danger-full-access",
    approval_policy: "never",
    sandbox_mode: "danger-full-access",
    network_access: true,
    collaboration_mode: null
  };
}

export function demoProviders(): AgentProviderInfo[] {
  return [
    {
      id: "codex",
      label: "Codex",
      status: "ready",
      description: "完整 Codex 控制面，使用官方 state DB、session_index、rollout 与受控 job。",
      capabilities: ["threads", "chat", "plan_questions", "uploads", "updates", "doctor"],
      safety: "保留官方数据结构，不修改 Codex DB schema"
    },
    {
      id: "grok_build",
      label: "Grok Build",
      status: "ready",
      description: "只读浏览 Grok Build 会话、消息和工具活动。",
      capabilities: ["sessions", "messages", "tools", "rename", "delete_local_session"],
      safety: "改名走原生协议，删除仅限本地 session 目录"
    },
    { id: "cursor", label: "Cursor CLI", status: "planned", capabilities: [], safety: "未开放命令执行" },
    { id: "gemini", label: "Gemini CLI", status: "planned", capabilities: [], safety: "未开放命令执行" }
  ];
}

export function demoProbeStatus(fixture: DemoFixtureKey = "linux-api"): ProbeStatus {
  const platform = demoPlatformOverview(fixture);
  const codexHome = fixture === "macos-tauri" ? "~/.codex" : "/root/.codex";
  return {
    label: "Probe",
    enabled: true,
    available: true,
    platform: platform.kind,
    service_kind: platform.service_kind,
    service_name: platform.service_name,
    flavor: "builtin",
    hook_status: "managed",
    bark_status: "not_configured",
    recent_event_count: 1,
    running_count: 1,
    reply_needed_count: 1,
    recoverable_count: 0,
    running_threads: [
      { id: "019e8c1f-demo", title: "活动库审阅链路", status: "Running", message_count: 18, latest_message: "正在逐项审计脚本输出。" }
    ],
    reply_needed_threads: [
      { id: "019e95a0-demo", title: "Plan Mode 修复", status: "ReplyNeeded", message_count: 7, latest_message: "等待确认" }
    ],
    recoverable_threads: [],
    lifecycle_status: "ok",
    doctor_status: "ok",
    runtime_version: "demo",
    config_path: platform.config_file,
    codex_home: codexHome,
    configured_codex_home: codexHome,
    resolved_codex_home: codexHome,
    codex_home_source: "default",
    logs_db_source: "resolved_codex_home",
    host_label: fixture,
    snapshot_age_seconds: 0,
    is_refreshing: false,
    snapshot_status: "cached"
  };
}

export function demoProbeSettings(fixture: DemoFixtureKey = "linux-api"): ProbeSettings {
  const codexHome = fixture === "macos-tauri" ? "~/.codex" : "/root/.codex";
  const runtimeProbeSettings = fixture === "macos-tauri"
    ? {
      logsPath: "~/Library/Application Support/AgentHub/logs_2.sqlite",
      workspace: "~/Documents"
    }
    : {
      logsPath: "/root/.codex/logs_2.sqlite",
      workspace: "/home/ubuntu/codex-workspace"
    };
  return {
    codex: {
      home: codexHome,
      configured_codex_home: codexHome,
      resolved_codex_home: codexHome,
      codex_home_source: "default",
      logs_db_source: "resolved_codex_home",
      discovery_warnings: [],
      workspace: runtimeProbeSettings.workspace,
      host_label: fixture
    },
    probe: {
      enabled: true,
      poll_seconds: 15,
      recent_limit: 50
    },
    notifications: {
      enabled: false,
      device_key_configured: false,
      server_url: "https://api.day.app",
      group: "AgentHub"
    },
  };
}

export function demoSavedProbeSettings(settings: Partial<ProbeSettings>, fixture: DemoFixtureKey = "linux-api"): ProbeSettings {
  return { ...demoProbeSettings(fixture), ...settings } as ProbeSettings;
}

export function demoProbeEvents(limit = 10): OptionalResult<ProbeEventsResponse> {
  const retryEvents = [
    { status: "waiting_retry", reason: "connection", title: "等待网络恢复" },
    { status: "retrying", reason: "timeout", title: "继续投递通知" },
    { status: "failed", reason: "apns_permanent_rejection", title: "服务拒绝通知" },
    { status: "unknown", reason: "response_read", title: "投递结果待确认" }
  ].map(({ status, reason, title }, index) => ({
    id: `probe-retry-${status}-demo`,
    kind: "completion",
    thread_id: "fixture-notification-thread",
    title,
    message: "示例任务已经完成。",
    dedupe_key: `fixture-notification-${status}`,
    source: "fixture-monitor",
    payload: {
      thread_title: title,
      body_summary: "示例任务已经完成。",
      bark: {
        sent: false, skipped: false, status, reason,
        attempts: status === "waiting_retry" ? 1 : status === "retrying" ? 2 : 3,
        request_count: 4, chunk_count: 3, confirmed_chunks: 1,
        next_retry_ms: status === "waiting_retry" ? Date.now() + 60000 : null
      }
    },
    created_at: new Date(Date.now() - (index + 3) * 300000).toISOString(),
    handled_at: null
  }));
  return {
    available: true,
    data: {
      limit,
      events: [
        {
          id: "probe-event-reply-demo",
          kind: "reply-needed",
          thread_id: "019e95a0-demo",
          title: "Raw reply event",
          message: "Probe 事件已记录",
          dedupe_key: "reply-needed:019e95a0-demo:turn-plan-demo",
          source: "agenthub-webd probe passive-scan",
          payload: {
            event_type: "reply-needed",
            thread_title: "Plan Mode 修复",
            thread_id: "019e95a0-demo",
            turn_id: "turn-plan-demo",
            beijing_time: "2026-06-16 09:30:00 北京时间",
            reason_label: "等待用户确认",
            body_summary: "Plan Mode 等待用户确认",
            body_sha256: "6b5d9f4f5a5a",
            body_length: 324,
            body_source: "proposed_plan",
            body_truncated: false,
            source: "agenthub-webd probe passive-scan",
            bark: { title: "等待回复：Plan Mode 修复", sent: false, skipped: true, reason: "dedupe", http_status: 200, dedupe_hit: true, chunk_count: 1, request_count: 0 },
            dedupe: { claimed: true, duplicate: false, status: "claimed" }
          },
          created_at: new Date().toISOString(),
          handled_at: null
        },
        {
          id: "probe-event-completion-demo",
          kind: "completion",
          thread_id: "019e5281-demo",
          title: "Completion",
          message: "Thread completed",
          dedupe_key: "completion:019e5281-demo:turn-done",
          source: "agenthub-webd probe hook-stop",
          payload: {
            event_type: "completion",
            thread_title: "检查仓库状态",
            thread_id: "019e5281-demo",
            turn_id: "turn-done",
            reason_label: "任务完成",
            body_summary: "仓库状态干净",
            body_sha256: "a13f98c0",
            body_length: 128,
            body_source: "task_complete.last_agent_message",
            body_truncated: false,
            source: "agenthub-webd probe hook-stop",
            bark: { title: "线程正常完成：检查仓库状态", sent: true, skipped: false, http_status: 200, dedupe_hit: false, chunk_count: 1, request_count: 1 },
            dedupe: { claimed: true, duplicate: false, status: "claimed" }
          },
          created_at: new Date(Date.now() - 300000).toISOString(),
          handled_at: null
        },
        {
          id: "probe-event-hook-demo",
          kind: "hook-stop",
          thread_id: "019e95a0-demo",
          title: "Codex Stop Hook",
          message: "Stop Hook event recorded by AgentHub Probe",
          dedupe_key: "hook-stop:019e95a0-demo:turn-demo",
          source: "agenthub-webd probe hook-stop",
          payload: {
            event_type: "hook-stop",
            thread_title: "Plan Mode 修复",
            thread_id: "019e95a0-demo",
            turn_id: "turn-demo",
            reason_label: "Stop Hook",
            body_summary: "Stop Hook event recorded by AgentHub Probe",
            body_sha256: "d9a8",
            body_length: 212,
            body_source: "default",
            body_truncated: false,
            source: "agenthub-webd probe hook-stop",
            bark: { title: "探针事件：Plan Mode 修复", skipped: true, reason: "notifications-disabled", dedupe_hit: false, chunk_count: 0, request_count: 0 },
            dedupe: { claimed: false, duplicate: true, status: "duplicate" }
          },
          created_at: new Date(Date.now() - 600000).toISOString(),
          handled_at: null
        },
        ...retryEvents
      ]
    }
  };
}

export function demoJobId(fallback: string): { job_id: string } {
  return { job_id: `${fallback}-demo` };
}

export function demoUpdateJobId(action: "check" | "install" | "prune"): { job_id: string } {
  return { job_id: `update-${action}-demo` };
}

export function demoThreads(archived = false, q = ""): ThreadSummary[] {
  const now = new Date().toISOString();
  const threads: ThreadSummary[] = [
    { id: "019e8c1f-demo", title: "活动库审阅链路", status: "Running", message_count: 18, latest_message: "正在逐项审计脚本输出。", rollout_path: "/isolated/codex/sessions/2026/10/29/rollout-019e8c1f-demo.jsonl", updated_at: now },
    { id: "019e95a0-demo", title: "Plan Mode 修复", status: "ReplyNeeded", message_count: 7, latest_message: "等待确认", rollout_path: "/isolated/codex/sessions/2026/10/29/rollout-019e95a0-demo.jsonl", updated_at: now },
    { id: "019e5281-demo", title: "检查仓库状态", status: "Recent", message_count: 3, latest_message: "仓库状态干净。", rollout_path: "/isolated/codex/sessions/2026/10/29/rollout-019e5281-demo.jsonl", updated_at: now },
    { id: "019e42aa-demo", title: "旧归档线程", status: "Archived", message_count: 2, latest_message: "已归档。", rollout_path: "/isolated/codex/archived_sessions/rollout-019e42aa-demo.jsonl", updated_at: new Date(Date.now() - 86400000).toISOString() }
  ];
  const wantArchived = Boolean(archived);
  return threads.filter((thread) => ((thread.status === "Archived") === wantArchived) && (!q || `${thread.title} ${thread.id}`.toLowerCase().includes(q.toLowerCase())));
}

export function demoThreadDetail(id: string): ThreadDetail {
  const all = [...demoThreads(false, ""), ...demoThreads(true, "")];
  const summary = all.find((thread) => thread.id === id) ?? all[0];
  const longChatBlocks: MessageBlock[] = Array.from({ length: 68 }, (_, index) => ({
    id: `history-${index}`,
    role: index % 2 === 0 ? "user" : "assistant",
    kind: "message",
    text: index % 2 === 0 ? `历史请求 ${index + 1}` : `历史回复 ${index + 1}`,
    questions: []
  }));
  const completedTools: MessageBlock[] = Array.from({ length: 20 }, (_, index) => ({
    id: `tool-history-${index}`,
    role: "tool",
    kind: "function_call_output",
    tool_name: "shell",
    status: "completed",
    summary: `历史工具 ${index + 1} 已完成`,
    text: `stdout line ${index + 1}`,
    questions: []
  }));
  return {
    summary: id === "019e95a0-demo" ? { ...summary, active_turn_id: "turn-plan-demo", pending_elicitation: { turn_id: "turn-plan-demo", item_id: "question-demo", questions: [{ id: "q1", question: "选择执行方式", options: [{ label: "直接实施", description: "使用当前计划继续执行" }, { label: "先修改", description: "补充约束后重新计划" }] }] } } : summary,
    raw_event_count: 96,
    total_blocks: 96,
    has_more_blocks: false,
    before_cursor: null,
    blocks: [
      ...longChatBlocks,
      { id: "u1", role: "user", kind: "userMessage", text: "检查远程主机 Codex 状态。", questions: [] },
      { id: "plan-demo", role: "assistant", kind: "plan", display_kind: "plan", turn_id: "turn-plan-demo", item_id: "plan-demo", status: "pending", resolved: false, plan_status: "pending", text: "<proposed_plan>1. 核对线程状态\n2. 修复 Plan/Questions 展示\n3. 验证并部署</proposed_plan>", questions: [] },
      { id: "question-answered", role: "assistant", kind: "request_user_input_result", display_kind: "question_result", turn_id: "turn-old-demo", status: "completed", resolved: true, answers: [{ question_id: "q0", answers: ["保留"], note: "历史选择已回答" }], questions: [{ id: "q0", question: "历史选项", options: [{ label: "保留" }, { label: "修改" }] }] },
      { id: "question-demo", role: "assistant", kind: "request_user_input", display_kind: "question", turn_id: "turn-plan-demo", call_id: "question-demo", status: "pending", resolved: false, questions: [{ id: "q1", question: "选择执行方式", options: [{ label: "直接实施", description: "使用当前计划继续执行" }, { label: "先修改", description: "补充约束后重新计划" }] }] },
      { id: "a1", role: "assistant", kind: "agentMessage", text: "状态正常，本地 Codex 状态库可读。归档删除 dry-run 可执行。", questions: [] },
      ...completedTools,
      { id: "t1", role: "tool", kind: "commandExecution", tool_name: "shell", text: "codex-cloud-doctor\nsqlite integrity_check: ok", status: "completed", questions: [] },
      { id: "t-running", role: "tool", kind: "function_call", tool_name: "shell", summary: "正在刷新本地状态", text: "sqlite3 /root/.codex/state_5.sqlite 'pragma integrity_check;'", status: "running", questions: [] }
    ],
    messages: [
      { role: "user", kind: "message", text: "检查远程主机 Codex 状态。" },
      { role: "assistant", kind: "message", text: "状态正常，本地 Codex 状态库可读。归档删除 dry-run 可执行。" },
      { role: "tool", kind: "function_call", text: "codex-cloud-doctor\nsqlite integrity_check: ok" }
    ]
  };
}

export function demoThreadBlockPage(id: string): ThreadBlockPage {
  const detail = demoThreadDetail(id);
  return {
    thread_id: id,
    blocks: detail.blocks,
    total_blocks: detail.total_blocks ?? detail.blocks.length,
    has_more_blocks: Boolean(detail.has_more_blocks),
    before_cursor: detail.before_cursor ?? null
  };
}

export function demoOk(): { ok: boolean } {
  return { ok: true };
}

export function demoHiddenThreadDeletePlan(): HiddenThreadDeletePlan {
  return {
    total_threads: 42,
    visible_threads: 38,
    hidden_threads: 4,
    archived_threads: 0,
    session_index_lines: 42,
    rollout_files: 42,
    hidden_ids: ["019e-hidden-a", "019e-hidden-b", "019e-hidden-c", "019e-hidden-d"],
    candidates: ["019e-hidden-a", "019e-hidden-b", "019e-hidden-c", "019e-hidden-d"].map(id => ({ id, title: id, fingerprint: "fixture-fingerprint", allowed: true, reason: null })),
    hidden_source_counts: { exec: 1, subagent: 3 },
    integrity: "ok"
  };
}

export function demoUnifiedCleanupPlan(
  fixture: DemoFixtureKey = "macos-tauri",
  request?: UnifiedCleanupDryRunRequest
): UnifiedCleanupPlan {
  const scanHidden = request?.includeCodexHidden !== false || Boolean(request?.scopes?.includes("codex_hidden"));
  const macos = fixture === "macos-tauri";
  const allScopes = [
    {
      scope: "codex_archived",
      provider: "codex",
      label: "Codex 已归档线程",
      supported: true,
      scanned: true,
      unavailableReason: null,
      candidateCount: 2,
      deletableCount: 2,
      estimatedBytes: 24576,
      candidates: [
        { id: "019e-demo-a", title: "旧归档线程 A", estimatedBytes: 12288, fingerprint: "fp-codex-arch-a", blockedReason: null },
        { id: "019e-demo-b", title: "旧归档线程 B", estimatedBytes: 12288, fingerprint: "fp-codex-arch-b", blockedReason: null }
      ],
      issues: []
    },
    {
      scope: "codex_hidden",
      provider: "codex",
      label: "Codex 隐藏线程",
      supported: true,
      scanned: scanHidden,
      unavailableReason: null,
      candidateCount: scanHidden ? 4 : 0,
      deletableCount: scanHidden ? 4 : 0,
      estimatedBytes: scanHidden ? 16384 : 0,
      candidates: scanHidden
        ? ["019e-hidden-a", "019e-hidden-b", "019e-hidden-c", "019e-hidden-d"].map((id) => ({
            id,
            title: id,
            estimatedBytes: 4096,
            fingerprint: `fp-${id}`,
            blockedReason: null
          }))
        : [],
      issues: []
    },
    {
      scope: "antigravity_archived",
      provider: "antigravity",
      label: "Antigravity 已归档线程",
      supported: macos,
      scanned: macos,
      unavailableReason: macos ? null : "仅本机 macOS 支持 Antigravity 会话清理",
      candidateCount: macos ? 1 : 0,
      deletableCount: macos ? 1 : 0,
      estimatedBytes: macos ? 8192 : 0,
      candidates: macos
        ? [{ id: "ag:ag-archived-demo", title: "Antigravity 历史归档会话", estimatedBytes: 8192, fingerprint: "fp-ag-demo", blockedReason: null }]
        : [],
      issues: []
    },
    {
      scope: "dsh_archived",
      provider: "dsh",
      label: "DeepSeek Harness 已归档线程",
      supported: true,
      scanned: true,
      unavailableReason: null,
      candidateCount: 1,
      deletableCount: 1,
      estimatedBytes: 6144,
      candidates: [{ id: "dsh-archived-demo", title: "DSH 历史归档会话", estimatedBytes: 6144, fingerprint: "fp-dsh-demo", blockedReason: null }],
      issues: []
    },
    {
      scope: "claude_empty",
      provider: "claude",
      label: "Claude Code 空残余会话",
      supported: true,
      scanned: true,
      unavailableReason: null,
      candidateCount: 1,
      deletableCount: 1,
      estimatedBytes: 1024,
      candidates: [{ id: "claude:empty-demo", title: "空会话残余", estimatedBytes: 1024, fingerprint: "fp-claude-demo", blockedReason: null }],
      issues: []
    },
    {
      scope: "grok_empty",
      provider: "grok",
      label: "Grok Build 空残余会话",
      supported: true,
      scanned: true,
      unavailableReason: null,
      candidateCount: 0,
      deletableCount: 0,
      estimatedBytes: 0,
      candidates: [],
      issues: []
    }
  ];
  const scopes = request?.scopes?.length
    ? allScopes.filter((item) => request.scopes!.includes(item.scope))
    : allScopes;
  return {
    totalCandidates: scopes.reduce((sum, item) => sum + item.candidateCount, 0),
    totalDeletable: scopes.reduce((sum, item) => sum + item.deletableCount, 0),
    totalEstimatedBytes: scopes.reduce((sum, item) => sum + item.estimatedBytes, 0),
    scopes
  };
}

export function demoUnifiedCleanupResult(request?: UnifiedCleanupExecuteRequest): UnifiedCleanupResult {
  const plan = demoUnifiedCleanupPlan("macos-tauri", { includeCodexHidden: true });
  const selected = request?.scopes?.length
    ? plan.scopes.filter((item) => request.scopes!.includes(item.scope))
    : plan.scopes.filter((item) => item.deletableCount > 0);
  const scopes = selected.map((item) => ({
    scope: item.scope,
    provider: item.provider,
    label: item.label,
    deletedCount: item.deletableCount,
    skippedCount: 0,
    failedCount: 0,
    deletedBytes: item.estimatedBytes,
    residuals: [],
    issues: []
  }));
  return {
    deletedCount: scopes.reduce((sum, item) => sum + item.deletedCount, 0),
    skippedCount: 0,
    failedCount: 0,
    deletedBytes: scopes.reduce((sum, item) => sum + item.deletedBytes, 0),
    stoppedEarly: false,
    scopes
  };
}

export function demoJobs(): JobRecord[] {
  return [
    { id: "probe-bark-demo", kind: "probe_bark_test", status: "succeeded", title: "AgentHub 推送测试", started_at: 1780731706, finished_at: 1780731710, exit_code: 0, output: "AgentHub 推送测试\n来自本机的测试通知。\nBark 已受理" },
    { id: "job-demo", kind: "agenthub_update_check", status: "succeeded", title: "AgentHub update precheck", started_at: 1780731606, output: "version check\nintegrity_check: ok" },
    { id: "job-failed-demo", kind: "panel_update", status: "failed", title: "Panel update", started_at: 1780731206, finished_at: 1780731252, exit_code: 1, output: "download release asset\nverify checksum", error: "release asset checksum mismatch", analysis: "Downloaded asset digest did not match release metadata.", explanation: "Retry after confirming the release asset has finished publishing." }
  ];
}

export function demoJob(id: string): JobRecord {
  return demoJobs().find((job) => job.id === id) ?? {
    id,
    kind: "unknown",
    status: "failed",
    title: id,
    started_at: Date.now() / 1000,
    output: "",
    error: "demo job not found"
  };
}
