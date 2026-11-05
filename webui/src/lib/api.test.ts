import { afterEach, describe, expect, test, vi } from "vitest";

import type { MessageBlock, ProbeStatus, SystemCapabilitiesResponse, ThreadDetail, ThreadSummary } from "../types";

async function loadRealApi() {
  vi.stubEnv("VITE_USE_REAL_API", "1");
  vi.resetModules();
  return import("./api");
}

async function loadDesktopApi() {
  vi.stubEnv("VITE_USE_REAL_API", "1");
  vi.resetModules();
  globalThis.__AGENTHUB_DESKTOP_RUNTIME__ = true;
  return import("./api");
}

async function loadDesktopDemoApi() {
  vi.stubEnv("VITE_USE_REAL_API", "0");
  vi.resetModules();
  globalThis.__AGENTHUB_DESKTOP_RUNTIME__ = true;
  return import("./api");
}

function rpcCall(invokeMock: ReturnType<typeof vi.fn>, index = 0) {
  const [command, args] = invokeMock.mock.calls[index];
  return { command, path: command, body: args };
}

describe("archive delete API compatibility", () => {
  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
    vi.unstubAllEnvs();
    delete globalThis.__AGENTHUB_DESKTOP_RUNTIME__;
    delete globalThis.__AGENTHUB_TEST_INVOKE__;
    vi.resetModules();
  });

  test("thread detail request supports pagination query parameters", async () => {
    const { getThread } = await loadRealApi();
    const invokeMock = vi.fn(async (_path: string, _args?: Record<string, unknown>) => ({
      summary: { id: "thread-a", title: "example-user", status: "Recent", message_count: 1 },
      messages: [],
      blocks: [],
      raw_event_count: 1,
      total_blocks: 240,
      has_more_blocks: true,
      before_cursor: "b:120"
    }));
    vi.stubGlobal("__AGENTHUB_TEST_INVOKE__", invokeMock);

    const detail = await getThread("thread-a", { limit: 120, before: "b:240", full: true });

    expect(rpcCall(invokeMock).path).toBe("threads.detail");
    expect(rpcCall(invokeMock).body).toEqual({
      id: "thread-a",
      options: { limit: 120, before: "b:240", full: true }
    });
    expect(detail.total_blocks).toBe(240);
    expect(detail.before_cursor).toBe("b:120");
  });

  test("probe events request uses the dedicated endpoint and limit parameter", async () => {
    const { getProbeEvents } = await loadRealApi();
    const invokeMock = vi.fn(async (_path: string, _args?: Record<string, unknown>) => ({
      events: [{ id: "event-1", kind: "hook-stop", source: "test", payload: {}, created_at: "2026-06-15T00:00:00Z" }],
      limit: 10
    }));
    vi.stubGlobal("__AGENTHUB_TEST_INVOKE__", invokeMock);

    const result = await getProbeEvents(10);

    expect(rpcCall(invokeMock).path).toBe("probe.events");
    expect(rpcCall(invokeMock).body).toEqual({ limit: 10 });
    expect(result.available).toBe(true);
    expect(result.data?.events[0].kind).toBe("hook-stop");
  });

  test("demo probe events expose structured payload fields for the UI cards", async () => {
    vi.resetModules();
    const { getProbeEvents } = await import("./api");

    const result = await getProbeEvents(10);

    expect(result.available).toBe(true);
    expect(result.data?.events).toHaveLength(7);
    expect(result.data?.events[0]).toMatchObject({
      payload: expect.objectContaining({
        event_type: "reply-needed",
        thread_title: "Plan Mode 修复",
        reason_label: "等待用户确认",
        bark: expect.objectContaining({ sent: false, skipped: true, reason: "dedupe", http_status: 200, dedupe_hit: true }),
        dedupe: expect.objectContaining({ claimed: true, duplicate: false, status: "claimed" })
      })
    });
    expect(JSON.stringify(result.data)).not.toContain("secret");
  });

  test("demo probe and thread data do not expose raw proposed plan tags", async () => {
    vi.resetModules();
    const { getProbeStatus, listThreads } = await import("./api");

    const status = await getProbeStatus();
    const threads = await listThreads(false, "");
    const replyNeeded = threads.find((thread) => thread.status === "ReplyNeeded");

    expect(status.available).toBe(true);
    expect(JSON.stringify(status.data)).not.toContain("<proposed_plan>");
    expect(JSON.stringify(threads)).not.toContain("<proposed_plan>");
    expect(status.data?.reply_needed_threads?.[0]?.latest_message).toBe("等待确认");
    expect(replyNeeded?.latest_message).toBe("等待确认");
  });

  test("demo jobs include probe job history entries", async () => {
    vi.resetModules();
    const { listJobs } = await import("./api");

    const jobs = await listJobs();
    const serialized = JSON.stringify(jobs);
    const retiredCodexJobTitle = ["Codex", "update", "precheck"].join(" ");
    const retiredClaudeJobTitle = ["Claude Code", "update"].join(" ");

    expect(jobs.some((job) => job.kind.startsWith("probe_"))).toBe(true);
    expect(jobs.some((job) => job.title.includes("AgentHub 推送测试"))).toBe(true);
    expect(serialized).not.toContain(retiredCodexJobTitle);
    expect(serialized).not.toContain(retiredClaudeJobTitle);
  });

  test("thread block page request uses lightweight blocks endpoint", async () => {
    const { getThreadBlocks } = await loadRealApi();
    const invokeMock = vi.fn(async (_path: string, _args?: Record<string, unknown>) => ({
      threadId: "thread-a",
      blocks: [{ id: "b1", role: "assistant", kind: "message", text: "old", questions: [] }],
      totalBlocks: 240,
      hasMoreBlocks: true,
      beforeCursor: "b:120"
    }));
    vi.stubGlobal("__AGENTHUB_TEST_INVOKE__", invokeMock);

    const page = await getThreadBlocks("thread-a", { limit: 80, before: "b:200" });

    expect(rpcCall(invokeMock).path).toBe("threads.blocks");
    expect(rpcCall(invokeMock).body).toEqual({
      id: "thread-a",
      options: { limit: 80, before: "b:200" }
    });
    expect(page.thread_id).toBe("thread-a");
    expect(page.blocks[0].id).toBe("b1");
    expect(page.before_cursor).toBe("b:120");
    expect(page.total_blocks).toBe(240);
    expect(page.has_more_blocks).toBe(true);
  });

  test("desktop thread block page request keeps limit and cursor in the typed native command", async () => {
    const { getThreadBlocks } = await loadDesktopApi();
    const fetchMock = vi.fn();
    vi.stubGlobal("fetch", fetchMock);
    globalThis.__AGENTHUB_TEST_INVOKE__ = vi.fn(async (command, args) => {
      expect(command).toBe("threads.blocks");
      expect(args).toEqual({
        id: "thread-a",
        options: { limit: 80, before: "b:200" }
      });
      return {
        threadId: "thread-a",
        blocks: [{ id: "b1", role: "assistant", kind: "message", text: "old", questions: [] }],
        totalBlocks: 240,
        hasMoreBlocks: true,
        beforeCursor: "b:120"
      };
    });

    const page = await getThreadBlocks("thread-a", { limit: 80, before: "b:200" });

    expect(fetchMock).not.toHaveBeenCalled();
    expect(page.thread_id).toBe("thread-a");
    expect(page.before_cursor).toBe("b:120");
  });

  test("routes AgentHub updates to typed runtime command endpoints", async () => {
    const { getUpdateStatus, updates } = await loadRealApi();
    const invokeMock = vi.fn(async (_path: string, _args?: Record<string, unknown>) => ({ job_id: "panel-job" }));
    vi.stubGlobal("__AGENTHUB_TEST_INVOKE__", invokeMock);

    invokeMock.mockResolvedValueOnce(({
      current_version: "0.1.100",
      latest_version: "v0.1.103",
      update_available: true,
      channel: "stable",
      method: "linux_systemd_job",
      state: "idle",
      recommended_action: "/usr/local/bin/agenthub-webd-update",
      capabilities: ["job_history"]
    }));
    const status = await getUpdateStatus();
    const result = await updates.install();

    const statusCall = rpcCall(invokeMock, 0);
    const actionCall = rpcCall(invokeMock, 1);
    expect(status.method).toBe("linux_systemd_job");
    expect(result).toEqual({ job_id: "panel-job" });
    expect(statusCall.path).toBe("updates.status");
    expect(actionCall.path).toBe("updates.install");

    expect(actionCall.body).toEqual({});
  });

  test("Probe demo data labels the builtin AgentHub service consistently", async () => {
    vi.unstubAllEnvs();
    vi.resetModules();
    const { getProbeStatus } = await import("./api");

    await expect(getProbeStatus()).resolves.toMatchObject({
      available: true,
      data: { flavor: "builtin", service_kind: "tauri", service_name: "AgentHub.app" }
    });
  });

  test("desktop demo platform data reflects macOS Tauri instead of Linux WebUI", async () => {
    vi.unstubAllEnvs();
    vi.resetModules();
    globalThis.__AGENTHUB_DESKTOP_RUNTIME__ = true;
    const { getPlatformOverview, getProbeStatus } = await import("./api");

    await expect(getPlatformOverview()).resolves.toMatchObject({
      kind: "macos",
      service_kind: "tauri",
      service_name: "AgentHub.app"
    });
    await expect(getProbeStatus()).resolves.toMatchObject({
      available: true,
      data: {
        platform: "macos",
        service_kind: "tauri",
        service_name: "AgentHub.app"
      }
    });
  });

  test("Probe fixed jobs use typed runtime command endpoints and maintenance is retired", async () => {
    const api = await loadRealApi();
    const invokeMock = vi.fn(async () => ({ job_id: "bark-job-1" }));
    vi.stubGlobal("__AGENTHUB_TEST_INVOKE__", invokeMock);
    await expect(api.runProbeBarkTest()).resolves.toEqual({ job_id: "bark-job-1" });
    await expect(api.runProbeHooksInstall()).resolves.toEqual({ job_id: "bark-job-1" });
    await expect(api.runProbeHooksRemove()).resolves.toEqual({ job_id: "bark-job-1" });
    expect(invokeMock.mock.calls.map(([path]) => path)).toEqual(["probe.barkTest", "probe.installHooks", "probe.removeHooks"]);
    for (const name of ["getProbeLogsDbStatus", "runProbeLogsDbDryRun", "runProbeLogsDbExecute"]) expect(api).not.toHaveProperty(name);
  });

  test("Probe API accepts resolved Codex path discovery fields", async () => {
    const { getProbeStatus } = await loadRealApi();
    const probeStatus: ProbeStatus = {
      label: "Probe",
      enabled: true,
      available: true,
      platform: "linux",
      service_kind: "systemd",
      service_name: "agenthub-webd",
      flavor: "builtin",
      hook_status: "managed",
      bark_status: "configured",
      recent_event_count: 0,
      running_count: 0,
      reply_needed_count: 0,
      recoverable_count: 0,
      running_threads: [],
      reply_needed_threads: [],
      recoverable_threads: [],
      config_path: "/etc/agenthub-webd/config.toml",
      codex_home: "/root/.codex",
      configured_codex_home: null,
      resolved_codex_home: "/home/codex/.codex",
      codex_home_source: "auto",
      logs_db_source: "resolved_codex_home",
      discovery_warnings: ["configured Codex home missing"]
    };
    const responses: Record<string, unknown> = {
      "probe.status": probeStatus,
    };
    vi.stubGlobal("__AGENTHUB_TEST_INVOKE__", vi.fn(async (path: string) => (responses[String(path)])));

    await expect(getProbeStatus()).resolves.toMatchObject({
      available: true,
      data: {
        configured_codex_home: null,
        resolved_codex_home: "/home/codex/.codex",
        codex_home_source: "auto",
        logs_db_source: "resolved_codex_home",
        discovery_warnings: ["configured Codex home missing"]
      }
    });
  });

  test("Probe OptionalResult wrappers are unwrapped without confusing ProbeStatus availability", async () => {
    const { getProbeEvents, getProbeStatus } = await loadRealApi();
    const probeStatus: ProbeStatus = {
      label: "Probe",
      enabled: true,
      available: true,
      platform: "macos",
      service_kind: "tauri",
      service_name: "AgentHub.app",
      flavor: "builtin",
      hook_status: "managed",
      bark_status: "configured",
      recent_event_count: 1,
      running_count: 0,
      reply_needed_count: 0,
      recoverable_count: 0,
      running_threads: [],
      reply_needed_threads: [],
      recoverable_threads: [],
      config_path: "/Users/example/Library/Application Support/AgentHub/config.toml"
    };
    const responses: Record<string, unknown> = {
      "probe.status": probeStatus,
      "probe.events": {
        available: true,
        data: {
          limit: 10,
          events: [{
            id: "event-a",
            kind: "reply-needed",
            thread_id: "thread-a",
            title: "Reply",
            message: "Need input",
            source: "probe",
            payload: {},
            created_at: "2026-06-19T00:00:00Z",
            handled_at: null
          }]
        }
      }
    };
    vi.stubGlobal("__AGENTHUB_TEST_INVOKE__", vi.fn(async (path: string) => (responses[String(path)])));

    await expect(getProbeStatus()).resolves.toMatchObject({
      available: true,
      data: {
        available: true,
        service_kind: "tauri",
        service_name: "AgentHub.app"
      }
    });
    await expect(getProbeEvents(10)).resolves.toMatchObject({
      available: true,
      data: {
        limit: 10,
        events: [{ id: "event-a", kind: "reply-needed" }]
      }
    });
  });

  test("listJobs unwraps OptionalResult responses into a stable array contract", async () => {
    const { listJobs } = await loadRealApi();
    const invokeMock = vi.fn(async () => ({
      available: true,
      data: [{ id: "job-a", kind: "probe", status: "succeeded", title: "Job A", started_at: 1, output: "" }]
    }));
    vi.stubGlobal("__AGENTHUB_TEST_INVOKE__", invokeMock);

    await expect(listJobs()).resolves.toEqual([
      expect.objectContaining({ id: "job-a", kind: "probe" })
    ]);
    expect(rpcCall(invokeMock).path).toBe("jobs.list");
  });

  test("status path display helpers prefer resolved backend paths and source labels", async () => {
    const app = await import("../test/domain");
    const status = {
      host_label: "cloud",
      codex_home: "/root/.codex",
      configured_codex_home: null,
      resolved_codex_home: "/home/codex/.codex",
      codex_home_source: "auto",
      panel_db: "/var/lib/agenthub-webd/agenthub.sqlite"
    };

    expect(app.codexHomeStatusValue(status)).toBe("/home/codex/.codex · auto");
    expect(app.codexHomeStatusValue({ codex_home: "" })).toBe("未知");
  });

  test("runtime UI capabilities derive from core system capabilities", async () => {
    const { runtimeCapabilities, runtimeCapabilitiesFromResponse, runtimeCapabilitiesForRuntime } = await loadRealApi();
    const linuxCore: SystemCapabilitiesResponse["capabilities"] = {
      threads: true,
      jobs: true,
      probe: true,
      settings: true,
      job_history: true,
      app_updater: true,

      systemd: true,

      linux_update_job: true,
      prune_backups: true,
      thread_cleanup: true,
      thread_archive_actions: true,
    };
    const macCore: SystemCapabilitiesResponse["capabilities"] = {
      ...linuxCore,

      systemd: false,

      linux_update_job: false,
      prune_backups: false,
      thread_cleanup: true,
      thread_archive_actions: true,
    };
    const webBootstrap = runtimeCapabilitiesForRuntime("desktop");
    const desktopBootstrap = runtimeCapabilitiesForRuntime("desktop");

    expect(runtimeCapabilities()).toEqual(webBootstrap);
    expect(webBootstrap).toMatchObject({
      runtimeKind: "desktop",
      hostSurface: "desktop_embedded_tauri",

      codexStatePaths: false,
      updatePrune: false,
      threadCleanup: false,
      threadArchiveActions: false,
      updateServiceLabels: false,
    });
    expect(desktopBootstrap).toMatchObject({
      runtimeKind: "desktop",
      hostSurface: "desktop_embedded_tauri",

      codexStatePaths: false,
      updatePrune: false,
      threadCleanup: false,
      threadArchiveActions: false,
      updateServiceLabels: false,
    });

    expect(runtimeCapabilitiesFromResponse({ capabilities: linuxCore, host_surface: "linux_server_api" }, webBootstrap)).toMatchObject({
      runtimeKind: "desktop",
      hostSurface: "linux_server_api",

      updatePrune: true,
      threadCleanup: true,
      threadArchiveActions: true,
      updateServiceLabels: true,
    });
    expect(runtimeCapabilitiesFromResponse({ capabilities: macCore, host_surface: "desktop_embedded_tauri" }, desktopBootstrap)).toMatchObject({
      runtimeKind: "desktop",
      hostSurface: "desktop_embedded_tauri",

      updatePrune: false,
      threadCleanup: true,
      threadArchiveActions: true,
      updateServiceLabels: false,
    });
    expect(runtimeCapabilitiesFromResponse({ capabilities: linuxCore }, desktopBootstrap)).toMatchObject({
      runtimeKind: "desktop",

      codexStatePaths: false,
      updatePrune: false,
      threadCleanup: true,
      threadArchiveActions: true,
      updateServiceLabels: false,
    });
    expect(Object.keys(runtimeCapabilitiesFromResponse({ capabilities: linuxCore }, webBootstrap))).not.toEqual(expect.arrayContaining([
      "linuxBackupPrune",
      "linuxUpdateLabels"
    ]));
  });

  test("desktop demo/default data does not expose Linux-only operations or web auth copy", async () => {
    const { getSystemCapabilities, getUpdateStatus, getPlatformOverview } = await loadDesktopDemoApi();

    const systemCapabilities = await getSystemCapabilities();
    const updateStatus = await getUpdateStatus();
    const platformOverview = await getPlatformOverview();
    const visibleValues = [
      updateStatus.method,
      updateStatus.recommended_action,
      ...(updateStatus.capabilities ?? []),
      platformOverview.kind,
      platformOverview.data_dir,
      platformOverview.config_file,
      platformOverview.log_dir,
      platformOverview.service_name,
      platformOverview.service_kind
    ].filter((value): value is string => typeof value === "string").join("\n");

    expect(visibleValues).not.toMatch(/systemd|Nginx|管理员密码|Turnstile|Linux prune|prune_backups|linux_systemd_job|panel\.example\.com/i);
    expect(visibleValues).toMatch(/macos_tauri_updater|signature_verification|restart_after_install/);
  });

  test("saveProbeSettings sends the canonical probe payload plus Bark compatibility key", async () => {
    const { saveProbeSettings } = await loadRealApi();
    const invokeMock = vi.fn(async (_path: string, _args?: Record<string, unknown>) => ({ saved: true }));
    vi.stubGlobal("__AGENTHUB_TEST_INVOKE__", invokeMock);

    await saveProbeSettings({
      codex: { home: "/root/.codex", workspace: "/home/ubuntu/codex-workspace", host_label: "cloud" },
      probe: {
        enabled: true,
        notifications: { enabled: true, device_key: "secret", server_url: "https://api.day.app" },
        observability: { event_retention_days: 2 }
      }
    }, );

    const call = rpcCall(invokeMock);
    const body = call.body as Record<string, any>;
    expect(call.path).toBe("probe.settings.save");

    expect(Object.keys(body).sort()).toEqual(["settings"]);
    expect(Object.keys(body.settings).sort()).toEqual(["codex", "notifications", "probe"]);
    expect(body.settings.probe.notifications).toEqual({
      enabled: true,
      device_key: "secret",
      server_url: "https://api.day.app"
    });
    expect(body.settings.notifications).toEqual({ device_key: "secret" });
    expect(body.settings.probe.observability).toEqual({ event_retention_days: 2 });
  });

  test("retired provider job helper is no longer exported", async () => {
    const api = await loadRealApi() as Record<string, unknown>;
    const invokeMock = vi.fn(async (_path: string, _args?: Record<string, unknown>) => ({ job_id: "unexpected" }));
    vi.stubGlobal("__AGENTHUB_TEST_INVOKE__", invokeMock);

    expect(api[["startClaude", "CodeJob"].join("")]).toBeUndefined();
    expect(api[["claudeCode", "JobRoutes"].join("")]).toBeUndefined();
    expect(invokeMock).not.toHaveBeenCalled();
  });

  test("Job History labels read-only filesystem failures in Chinese", async () => {
    const app = await import("../test/domain") as typeof import("../test/domain") & {
      failureCategoryLabel?: (category: string) => string;
    };

    expect(app.failureCategoryLabel?.("read_only_file_system")).toBe("文件系统只读/安装目录不可写");
  });

  test("desktop update helpers use shared typed updater commands instead of Linux panel routes", async () => {
    const { getUpdateStatus, updates, runtimeCapabilitiesForRuntime } = await loadDesktopApi();
    const fetchMock = vi.fn();
    vi.stubGlobal("fetch", fetchMock);
    globalThis.__AGENTHUB_TEST_INVOKE__ = vi.fn(async (command, args) => {
      if (command === "updates.install") {
        return { job_id: "desktop-native-job", installed: false };
      }
      if (command === "updates.check") {
        return {
          job_id: "desktop-check-job",
          status: {
            current_version: "0.1.100",
            latest_version: "v0.1.103",
            update_available: true,
            channel: "stable",
            method: "macos_tauri_updater",
            state: "ready",
            recommended_action: "Confirm install in the Tauri updater after signature verification.",
            capabilities: ["signature_verification", "job_history"]
          }
        };
      }
      expect(command).toBe("updates.status");
      expect(args).toBeUndefined();
      return {
        current_version: "0.1.100",
        latest_version: "v0.1.103",
        update_available: true,
        channel: "stable",
        method: "macos_tauri_updater",
        state: "idle",
        recommended_action: "Confirm install in the Tauri updater after signature verification.",
        capabilities: ["signature_verification", "job_history"]
      };
    });

    expect((await getUpdateStatus()).method).toBe("macos_tauri_updater");
    await expect(updates.check()).resolves.toEqual({
      job_id: "desktop-check-job",
      status: expect.objectContaining({
        latest_version: "v0.1.103",
        state: "ready"
      })
    });
    await expect(updates.install()).resolves.toEqual({ job_id: "desktop-native-job" });
    const macCapabilities = runtimeCapabilitiesForRuntime("desktop");
    expect(macCapabilities.updatePrune).toBe(false);
    await expect(updates.prune(macCapabilities)).rejects.toThrow("当前运行时不支持备份清理动作");
    try {
      await updates.prune(macCapabilities);
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      expect(message).not.toMatch(/Linux|systemd|Nginx|sudo/i);
    }

    expect(fetchMock).not.toHaveBeenCalled();
    expect(globalThis.__AGENTHUB_TEST_INVOKE__).toHaveBeenCalledTimes(3);
    expect(globalThis.__AGENTHUB_TEST_INVOKE__).toHaveBeenCalledWith("updates.check", {});
    expect(globalThis.__AGENTHUB_TEST_INVOKE__).toHaveBeenCalledWith("updates.install", {});
  });

  test("update prune is gated by capability matrix instead of runtime kind", async () => {
    const { updates, runtimeCapabilitiesForRuntime } = await loadDesktopApi();
    const fetchMock = vi.fn();
    vi.stubGlobal("fetch", fetchMock);
    globalThis.__AGENTHUB_TEST_INVOKE__ = vi.fn(async (command, args) => ({ command, args, job_id: "desktop-prune-job" }));

    const desktopWithPrune = {
      ...runtimeCapabilitiesForRuntime("desktop"),
      updatePrune: true
    };

    await expect(updates.prune(desktopWithPrune)).resolves.toEqual({ job_id: "desktop-prune-job" });
    expect(globalThis.__AGENTHUB_TEST_INVOKE__).toHaveBeenCalledWith("updates.prune", {});
    expect(fetchMock).not.toHaveBeenCalled();
  });







  test("macOS demo fixtures do not include Linux-only rendered copy or paths", async () => {
    globalThis.__AGENTHUB_DESKTOP_RUNTIME__ = true;
    const {
      buildDemoFixture,
      buildDemoPlatformOverview,
      buildDemoSystemCapabilities
    } = await import("./domain/demoCore");
    const {
      demoCodexConfig,
      demoPlatformOverview,
      demoProbeSettings,
      demoSystemCapabilities,
      demoUpdateStatus
    } = await import("./api/demo");

    const macosFixtures = [
      buildDemoFixture("macos-tauri"),
      buildDemoPlatformOverview("macos-tauri"),
      buildDemoSystemCapabilities("macos-tauri"),
      demoPlatformOverview("macos-tauri"),
      demoSystemCapabilities("macos-tauri"),
      demoUpdateStatus("macos-tauri"),
      demoCodexConfig("macos-tauri"),
      demoProbeSettings("macos-tauri")
    ];
    const serialized = JSON.stringify(macosFixtures);

    expect(serialized).not.toMatch(/systemd|Nginx|Turnstile|管理员密码|公网入口|Public endpoint|Linux update|Linux prune|prune_backups|\/opt\/agenthub|192\.0\.2\.10|panel\.example\.com|\/home\/ubuntu|\/root\/\.codex/i);
  });





  test("plan, pending question and answered history render read-only without reviving actions", async () => {
    const { createElement } = await import("react");
    const { renderToStaticMarkup } = await import("react-dom/server");
    const { MessageBlockView } = await import("../components/chat/MessageStream");
    const blocks: MessageBlock[] = [
      { id: "plan", role: "assistant", kind: "plan", text: "<proposed_plan>Current plan</proposed_plan>", status: "pending", questions: [] },
      { id: "question", role: "assistant", kind: "request_user_input", turn_id: "turn-current", questions: [{ id: "q1", question: "Current question", options: [{ label: "Option A" }] }] },
      { id: "answer", role: "assistant", kind: "request_user_input_result", turn_id: "turn-old", status: "completed", questions: [], answers: [{ question_id: "q1", answers: ["Option B"] }] }
    ];
    const rendered = blocks.map(block => renderToStaticMarkup(createElement(MessageBlockView, { block }))).join("\n");
    for (const text of ["Current plan", "Current question", "Option A", "Option B", "turn-current"]) expect(rendered).toContain(text);
    expect(rendered).toContain('aria-label="复制计划"');
    expect(rendered).toContain('aria-label="下载计划 Markdown"');
    expect(rendered).not.toMatch(/<(input|textarea|form)\b/);
    expect(renderToStaticMarkup(createElement(MessageBlockView, { block: { id: "approval", role: "assistant", kind: "approval", text: "Retired approval", questions: [] } }))).toBe("");
  });

  test("retires top 5-category status tabs and splits active vs archived demo threads", async () => {
    const app = await import("../test/domain");
    const { demoThreads } = await import("./api/demo");

    expect("statusTabs" in app).toBe(false);
    const active = demoThreads(false, "");
    const archived = demoThreads(true, "");
    expect(active.length).toBeGreaterThan(0);
    expect(active.every((thread) => thread.status !== "Archived")).toBe(true);
    expect(archived.length).toBeGreaterThan(0);
    expect(archived.every((thread) => thread.status === "Archived")).toBe(true);
  });

  test("AgentHub navigation exposes the slim provider workspaces", async () => {
    const app = await import("../test/domain");

    expect(app.navigationItems.map((item: { id: string }) => item.id)).toEqual([
      "codex",
      "claude",
      "grok",
      "grokbot",
      "antigravity",
      "dsh",
      "probe",
      "ops"
    ]);
    expect(app.navigationItems.map((item: { label: string }) => item.label)).toEqual([
      "Codex",
      "Claude Code",
      "Grok Build",
      "Grok Bot",
      "Antigravity",
      "DeepSeek Harness",
      "Probe",
      "设置"
    ]);
  });

  test("thread list item text only exposes the title", async () => {
    const app = await import("../test/domain");

    expect(app.threadListItemText({
      id: "thread-a",
      title: "example-user",
      status: "Running",
      latest_message: "接手这个线程的工作 019e86d2...",
      model: "custom",
      last_event_kind: "app-server.thread/list",
      message_count: 8359
    })).toBe("example-user");
  });

  test("thread list metadata shows status and preview without contaminating title", async () => {
    const app = await import("../test/domain");
    const thread = {
      id: "thread-a",
      title: "example-user",
      status: "Running",
      latest_message: "正在执行长任务输出",
      active_turn_id: "turn-1",
      message_count: 24
    } satisfies Partial<ThreadSummary>;

    expect(app.threadListItemStatusText(thread)).toBe("运行中");
    expect(app.threadListItemPreviewText(thread)).toBe("正在执行长任务输出");
    expect(app.threadListItemText(thread)).toBe("example-user");
    expect(app.threadListItemPreviewText({
      ...thread,
      latest_message: "<proposed_plan>\n1. 检查\n2. 修复\n</proposed_plan>"
    })).toBe("1. 检查 2. 修复");
    expect(app.threadListItemPreviewText({ ...thread, latest_message: "" })).toBe("");
    expect(app.isThreadListItemRunning(thread)).toBe(true);
    expect(app.isThreadListItemRunning({ ...thread, status: "Recent", active_turn_id: undefined })).toBe(false);
  });

  test("demo thread list does not expose cwd or runtime workspace paths", async () => {
    vi.resetModules();
    const { listThreads } = await import("./api");

    const threads = await listThreads(false, "");
    const serialized = JSON.stringify(threads);

    expect(threads.every((thread) => !("cwd" in thread))).toBe(true);
    expect(serialized).not.toContain("/srv/example-workspace");
    expect(serialized).not.toContain("/root/.codex");
    expect(serialized).not.toContain("/home/ubuntu/codex-workspace");
  });

  test("thread list cache helper separates active and archived caches and applies search query", async () => {
    const app = await import("../test/domain");
    const existing = {
      id: "thread-a",
      title: "example-user",
      status: "Recent",
      latest_message: "old",
      message_count: 1
    } satisfies ThreadSummary;
    const running = {
      ...existing,
      title: "未命名线程",
      status: "Running",
      active_turn_id: "turn-live",
      latest_message: "working"
    } satisfies ThreadSummary;
    const archivedRow = {
      ...existing,
      status: "Archived",
      archived_at: "2026-10-01T00:00:00Z",
      active_turn_id: null,
      latest_message: "done"
    } satisfies ThreadSummary;

    expect(app.mergeThreadSummaryIntoListCache([], running, false, "")).toEqual([
      expect.objectContaining({ id: "thread-a", status: "Running", active_turn_id: "turn-live" })
    ]);
    expect(app.mergeThreadSummaryIntoListCache([existing], running, false, "")).toEqual([
      expect.objectContaining({ id: "thread-a", title: "example-user", latest_message: "working" })
    ]);
    expect(app.mergeThreadSummaryIntoListCache([running], archivedRow, false, "")).toEqual([]);
    expect(app.mergeThreadSummaryIntoListCache([], archivedRow, true, "")).toEqual([
      expect.objectContaining({ id: "thread-a", status: "Archived" })
    ]);
    expect(app.mergeThreadSummaryIntoListCache([existing], running, false, "example_task")).toEqual([]);
  });

  test("thread list visibility filters archived and every known subagent marker", async () => {
    const app = await import("../test/domain");
    const rows = [
      { id: "main", title: "example-user", status: "Running", message_count: 1 },
      { id: "archived", title: "old", status: "Archived", message_count: 1 },
      { id: "parent", title: "child", status: "Recent", message_count: 1, parentThreadId: "main" },
      { id: "source-kind", title: "child", status: "Recent", message_count: 1, sourceKind: "subAgentRun" },
      { id: "thread-source", title: "child", status: "Recent", message_count: 1, thread_source: "subagent" },
      { id: "source-json", title: "child", status: "Recent", message_count: 1, source: { subagent: { thread_spawn: { parentThreadId: "main" } } } },
      { id: "agent-path", title: "child", status: "Recent", message_count: 1, agentPath: "/tmp/subagent" },
      { id: "agent-nickname", title: "child", status: "Recent", message_count: 1, agentNickname: "reviewer" },
      { id: "agent-role", title: "child", status: "Recent", message_count: 1, agentRole: "explorer" },
      {
        id: "internal-exec",
        title: "只读验证任务。不要修改文件。",
        status: "Recent",
        message_count: 1,
        source: "exec",
        thread_source: "user",
        has_user_event: 0,
        first_user_message: "只读验证任务。不要修改文件。使用 tool_search 查询 spawn_agent。"
      },
      {
        id: "internal-subagent-prompt",
        title: "你是子代理 A，必须使用 gpt-5.5 和 xhigh。",
        status: "Recent",
        message_count: 1,
        source: "exec",
        thread_source: "user",
        has_user_event: 0
      }
    ] satisfies Array<Partial<ThreadSummary>>;

    expect(app.filterVisibleThreadSummaries(rows).map((thread) => thread.id)).toEqual(["main"]);
  });

  test("conversation title text ignores latest message previews", async () => {
    const app = await import("../test/domain");

    expect(app.conversationTitleText({
      title: " example-user ",
      latest_message: "接手这个线程的工作 019e86d2...",
      model: "custom",
      last_event_kind: "app-server.thread/read"
    })).toBe("example-user");
    expect(app.conversationTitleText({
      title: "   ",
      latest_message: "接手这个线程的工作 019e86d2..."
    })).toBe("未命名线程");
  });

  test("incoming realtime summary keeps an existing title when the update has only a placeholder", async () => {
    const app = await import("../test/domain");
    const current = {
      id: "thread-a",
      title: "example-user",
      status: "Recent",
      message_count: 8,
      latest_message: "旧摘要",
      last_event_kind: "task_complete"
    } satisfies Partial<ThreadSummary>;
    const incoming = {
      id: "thread-a",
      title: "未命名线程",
      status: "Running",
      message_count: 9,
      latest_message: "新摘要",
      active_turn_id: "turn-live",
      last_event_kind: "app-server.thread/list"
    } satisfies Partial<ThreadSummary>;

    expect(app.mergeIncomingThreadSummary(current, incoming)).toMatchObject({
      title: "example-user",
      status: "Running",
      latest_message: "新摘要",
      active_turn_id: "turn-live",
      last_event_kind: "task_complete"
    });
    expect(app.lastEventKindText({ last_event_kind: "app-server.thread/read" })).toBe("未知");
    expect(app.lastEventKindText({ last_event_kind: "panel.job.running" })).toBe("未知");
    expect(app.lastEventKindText({ last_event_kind: "task_complete" })).toBe("task_complete");
  });

  test("thread detail summary merges fresh list status without losing title", async () => {
    const app = await import("../test/domain");
    const detail = {
      summary: {
        id: "thread-a",
        title: "example-user",
        status: "Recent",
        message_count: 8,
        latest_message: "旧摘要",
        active_turn_id: null
      },
      messages: [],
      blocks: [],
      raw_event_count: 20
    } satisfies ThreadDetail;
    const merged = app.mergeThreadDetailSummaryFromList(detail, {
      id: "thread-a",
      title: "未命名线程",
      status: "Running",
      message_count: 9,
      latest_message: "正在执行",
      active_turn_id: "turn-live",
      last_event_kind: "app-server.thread/list"
    });

    expect(merged).not.toBe(detail);
    expect(merged.summary).toMatchObject({
      title: "example-user",
      status: "Running",
      latest_message: "正在执行",
      active_turn_id: "turn-live"
    });
  });

  test("thread detail refetch keeps polling idle selections and speeds up running threads", async () => {
    const app = await import("../test/domain");
    const idleDetail = {
      summary: {
        id: "thread-a",
        title: "example-user",
        status: "Recent",
        message_count: 8
      },
      messages: [],
      blocks: [],
      raw_event_count: 20
    } satisfies ThreadDetail;

    expect(app.threadDetailRefetchInterval(idleDetail, null)).toBe(5000);
    expect(app.threadDetailRefetchInterval(idleDetail, { status: "Running" })).toBe(5000);
    expect(app.threadDetailRefetchInterval(undefined, { status: "Running" })).toBe(2000);
    expect(app.threadDetailRefetchInterval({
      ...idleDetail,
      summary: { ...idleDetail.summary, status: "Running" }
    }, null)).toBe(2000);
  });

  test("tool block helpers expose compact title, summary, and detail text", async () => {
    const app = await import("../test/domain");
    const block = {
      id: "tool-1",
      role: "tool",
      kind: "function_call_output",
      status: "completed",
      tool_name: "exec_command",
      call_id: "call-1",
      summary: "Output: ok",
      input: "{\n  \"cmd\": \"pwd\"\n}",
      text: "Output:\n/home/ubuntu",
      truncated: true,
      questions: []
    };

    expect(app.isToolBlock(block)).toBe(true);
    expect(app.toolBlockTitle(block)).toBe("exec_command");
    expect(app.toolBlockSummary(block)).toBe("Output: ok");
    expect(app.toolBlockDetailText(block)).toContain("\"cmd\": \"pwd\"");
    expect(app.toolBlockDetailText(block)).toContain("Output:\n/home/ubuntu");
    expect(app.toolBlockDetailText(block)).toContain("[output truncated]");
    expect(app.toolBlockTitle({
      id: "chat-history-collapsed",
      role: "tool",
      kind: "chat_history_collapsed",
      status: "completed",
      tool_name: "chat_history",
      summary: "4078 条历史对话已折叠",
      questions: []
    })).toBe("4078 条历史对话已折叠");
  });

  test("upsert message block appends, replaces changed blocks, and keeps identical references", async () => {
    const app = await import("../test/domain");
    const current = [{
      id: "tool-1",
      role: "tool",
      kind: "function_call",
      status: "running",
      tool_name: "exec_command",
      questions: []
    }] satisfies MessageBlock[];
    const completed = {
      ...current[0],
      kind: "function_call_output",
      status: "completed",
      text: "Output:\n/tmp"
    } satisfies MessageBlock;

    const unchanged = app.upsertMessageBlock(current, current[0]);
    expect(unchanged).toBe(current);

    const replaced = app.upsertMessageBlock(current, completed);
    expect(replaced).not.toBe(current);
    expect(replaced).toHaveLength(1);
    expect(replaced[0]).toEqual(completed);

    const appended = app.upsertMessageBlock(replaced, {
      id: "assistant-1",
      role: "assistant",
      kind: "message",
      text: "done",
      questions: []
    });
    expect(appended).toHaveLength(2);
    expect(appended[1].id).toBe("assistant-1");
  });

  test("merge message blocks appends, prepends, updates, and preserves unchanged references", async () => {
    const app = await import("../test/domain");
    const current = [
      { id: "b2", role: "assistant", kind: "message", text: "middle", questions: [] },
      { id: "b3", role: "assistant", kind: "message", text: "old", questions: [] }
    ] satisfies MessageBlock[];

    expect(app.mergeMessageBlocks(current, [])).toBe(current);
    expect(app.mergeMessageBlocks(current, [{ ...current[0] }])).toBe(current);

    const appended = app.mergeMessageBlocks(current, [
      { id: "b3", role: "assistant", kind: "message", text: "updated", questions: [] },
      { id: "b4", role: "assistant", kind: "message", text: "new", questions: [] }
    ]);
    expect(appended).not.toBe(current);
    expect(appended.map((block: MessageBlock) => `${block.id}:${block.text}`)).toEqual([
      "b2:middle",
      "b3:updated",
      "b4:new"
    ]);

    const prepended = app.mergeMessageBlocks(appended, [
      { id: "b0", role: "user", kind: "message", text: "older", questions: [] },
      { id: "b1", role: "assistant", kind: "message", text: "old answer", questions: [] },
      { id: "b2", role: "assistant", kind: "message", text: "middle", questions: [] }
    ], "prepend");
    expect(prepended.map((block: MessageBlock) => block.id)).toEqual(["b0", "b1", "b2", "b3", "b4"]);
  });

  test("history compaction keeps every tool between the corresponding messages", async () => {
    const app = await import("../test/domain");
    const blocks: MessageBlock[] = Array.from({ length: 90 }, (_, index) => [
      { id: `text-${index}`, role: "assistant", kind: "message", text: `Step ${index}`, questions: [] },
      { id: `tool-${index}`, role: "tool", kind: "function_call_output", status: "completed", text: `done-${index}`, questions: [] }
    ]).flat();
    const compacted = app.visibleConversationBlocksForHistory(blocks, false);
    expect(compacted.filter(block => block.role === "tool" && block.kind !== "chat_history_collapsed")).toEqual(blocks.filter(block => block.role === "tool"));
    expect(compacted.some(block => block.kind === "tool_history_collapsed")).toBe(false);
    expect(compacted.slice(-4).map(block => block.id)).toEqual(["text-88", "tool-88", "text-89", "tool-89"]);
    expect(app.visibleConversationBlocksForHistory(blocks, true)).toEqual(blocks);
  });

  test("conversation message presentation uses light chat rows", async () => {
    const app = await import("../test/domain");

    expect(app.conversationMessagePresentation({ role: "user" })).toEqual({
      kind: "user",
      rowClassName: "chat-row user",
      bodyClassName: "chat-bubble"
    });
    expect(app.conversationMessagePresentation({ role: "assistant" })).toEqual({
      kind: "assistant",
      rowClassName: "chat-row assistant",
      bodyClassName: "assistant-message-body"
    });
  });

  test("plan and question blocks render in the conversation stream but not the action stack", async () => {
    const app = await import("../test/domain");
    const plan = {
      id: "plan-1",
      role: "assistant",
      kind: "plan",
      display_kind: "plan",
      turn_id: "turn-live",
      item_id: "plan-item",
      text: "<proposed_plan>先检查，再实现。</proposed_plan>",
      status: "pending",
      resolved: false,
      questions: []
    } satisfies MessageBlock;
    const question = {
      id: "question-1",
      role: "assistant",
      kind: "request_user_input",
      display_kind: "question",
      turn_id: "turn-live",
      call_id: "call-1",
      status: "pending",
      resolved: false,
      questions: [{ id: "q1", question: "选择方案", options: [{ label: "A" }, { label: "B" }] }]
    } satisfies MessageBlock;
    const approval = {
      id: "approval-1",
      role: "assistant",
      kind: "approval",
      display_kind: "approval",
      turn_id: "turn-live",
      text: "Allow command?",
      questions: []
    } satisfies MessageBlock;

    expect(app.shouldRenderConversationBlock(plan)).toBe(true);
    expect(app.shouldRenderConversationBlock(question)).toBe(true);
    expect(app.shouldRenderConversationBlock(approval)).toBe(false);
    expect(app.compactConversationBlocks([plan, question])).toEqual([plan, question]);
  });

  test("history ordering retains current plans and questions without promoting answered history", async () => {
    const app = await import("../test/domain");
    const currentPlan = {
      id: "plan-live",
      role: "assistant",
      kind: "plan",
      turn_id: "turn-live",
      item_id: "plan-item",
      status: "pending",
      resolved: false,
      text: "<proposed_plan>当前计划</proposed_plan>",
      questions: []
    } satisfies MessageBlock;
    const historyPlan = {
      ...currentPlan,
      id: "plan-old",
      turn_id: "turn-old",
      status: "completed",
      resolved: true,
      text: "<proposed_plan>旧计划</proposed_plan>"
    } satisfies MessageBlock;
    const answeredQuestion = {
      id: "question-old",
      role: "assistant",
      kind: "request_user_input",
      turn_id: "turn-old",
      call_id: "call-old",
      status: "completed",
      resolved: true,
      questions: [{ id: "q1", question: "选择方案", options: [{ label: "A" }, { label: "B" }] }],
      answers: [{ question_id: "q1", answers: ["B"], note: null }]
    } satisfies MessageBlock;
    const currentQuestion = {
      ...answeredQuestion,
      id: "question-live",
      turn_id: "turn-live",
      call_id: "call-live",
      status: "pending",
      resolved: false,
      answers: []
    } satisfies MessageBlock;

    expect(app.isActionablePlanBlock(currentPlan, currentPlan)).toBe(true);
    expect(app.isActionablePlanBlock(historyPlan, currentPlan)).toBe(false);
    expect(app.isActionableQuestionBlock(currentQuestion, currentQuestion)).toBe(true);
    expect(app.isActionableQuestionBlock(answeredQuestion, currentQuestion)).toBe(false);
    expect(app.questionAnswerLabels(answeredQuestion, "q1")).toEqual(["B"]);
    expect(app.isResolvedActionBlock(answeredQuestion)).toBe(true);

    expect(app.prioritizeCurrentActionBlocks(
      [currentPlan, answeredQuestion, currentQuestion],
      currentPlan,
      currentQuestion
    ).map((block) => block.id)).toEqual([
      "question-old",
      "plan-live",
      "question-live"
    ]);
  });

  test("conversation block compaction collapses old chat messages but keeps recent chat and running tools", async () => {
    const app = await import("../test/domain");
    const chat = Array.from({ length: 6 }, (_, index) => ({
      id: `chat-${index}`,
      role: index % 2 === 0 ? "user" : "assistant",
      kind: "message",
      text: `message-${index}`,
      questions: []
    })) satisfies MessageBlock[];
    const running = {
      id: "tool-live",
      role: "tool",
      kind: "function_call",
      status: "running",
      text: "running",
      questions: []
    } satisfies MessageBlock;

    const compacted = app.compactConversationBlocks([...chat, running], 2);

    expect(compacted.map((block) => block.id)).toEqual([
      "chat-history-collapsed",
      "chat-4",
      "chat-5",
      "tool-live"
    ]);
    expect(compacted[0].summary).toBe("4 条历史对话已折叠");
    expect(compacted[compacted.length - 1]).toBe(running);
  });

  test("conversation block compaction collapses historical plans while preserving the current plan", async () => {
    const app = await import("../test/domain");
    const historicalPlans = Array.from({ length: 8 }, (_, index) => ({
      id: `plan-${index}`,
      role: "assistant",
      kind: "plan",
      display_kind: "plan",
      turn_id: `turn-${index}`,
      item_id: `plan-item-${index}`,
      status: "pending",
      resolved: false,
      text: `<proposed_plan>历史计划 ${index}</proposed_plan>`,
      questions: []
    })) satisfies MessageBlock[];
    const currentPlan = {
      id: "plan-current",
      role: "assistant",
      kind: "plan",
      display_kind: "plan",
      turn_id: "turn-current",
      item_id: "plan-item-current",
      status: "pending",
      resolved: false,
      text: "<proposed_plan>当前计划</proposed_plan>",
      questions: []
    } satisfies MessageBlock;

    const compacted = app.visibleConversationBlocksForHistory(
      [...historicalPlans, currentPlan],
      false,
      currentPlan,
      null
    );
    const prioritized = app.prioritizeCurrentActionBlocks(compacted, currentPlan, null);

    expect(prioritized.map((block) => block.id)).toEqual([
      "action-history-collapsed",
      "plan-5",
      "plan-6",
      "plan-7",
      "plan-current"
    ]);
    expect(prioritized[0].summary).toBe("5 条历史计划/问题已折叠");
    expect(app.visibleConversationBlocksForHistory([...historicalPlans, currentPlan], true)).toHaveLength(9);
  });

  test("visible conversation history can expand from compacted to full renderable blocks", async () => {
    const app = await import("../test/domain");
    const chat = Array.from({ length: 70 }, (_, index) => ({
      id: `chat-${index}`,
      role: index % 2 === 0 ? "user" : "assistant",
      kind: "message",
      text: `message-${index}`,
      questions: []
    })) satisfies MessageBlock[];

    const compacted = app.visibleConversationBlocksForHistory(chat, false);
    const expanded = app.visibleConversationBlocksForHistory(chat, true);

    expect(compacted.map((block) => block.id)).toEqual([
      "chat-history-collapsed",
      ...Array.from({ length: 60 }, (_, index) => `chat-${index + 10}`)
    ]);
    expect(app.visibleConversationBlocksForHistory(chat, false).some((block) => block.kind === "chat_history_collapsed")).toBe(true);
    expect(expanded).toEqual(chat);
  });

  test("conversation block compaction does not duplicate server history collapse cards", async () => {
    const app = await import("../test/domain");
    const serverCollapsed = {
      id: "chat-history-collapsed",
      role: "tool",
      kind: "chat_history_collapsed",
      status: "completed",
      summary: "4078 条历史对话已折叠",
      questions: []
    } satisfies MessageBlock;
    const chat = Array.from({ length: 6 }, (_, index) => ({
      id: `chat-${index}`,
      role: "assistant",
      kind: "message",
      text: `message-${index}`,
      questions: []
    })) satisfies MessageBlock[];

    const compacted = app.compactConversationBlocks([serverCollapsed, ...chat], 2);

    expect(compacted.map((block) => block.id)).toEqual([
      "chat-history-collapsed",
      "chat-0",
      "chat-1",
      "chat-2",
      "chat-3",
      "chat-4",
      "chat-5"
    ]);
  });

  test("conversation message helper hides internal rollout roles", async () => {
    const app = await import("../test/domain");

    expect(app.shouldRenderConversationMessage({
      id: "developer-1",
      role: "developer",
      kind: "message",
      text: "internal instructions",
      questions: []
    })).toBe(false);
    expect(app.shouldRenderConversationMessage({
      id: "reasoning-1",
      role: "assistant",
      kind: "reasoning",
      text: "hidden reasoning",
      questions: []
    })).toBe(false);
    expect(app.shouldRenderConversationMessage({
      id: "assistant-1",
      role: "assistant",
      kind: "message",
      text: "visible answer",
      questions: []
    })).toBe(true);
  });

  test("conversation message helper retains complete user context and hides assistant internal context", async () => {
    const app = await import("../test/domain");

    expect(app.shouldRenderConversationMessage({
      id: "choice-1",
      role: "assistant",
      kind: "request_user_input",
      text: "选择方案",
      questions: [{ id: "q1", question: "选择方案", options: [{ label: "A" }] }]
    })).toBe(false);
    expect(app.shouldRenderConversationMessage({
      id: "subagent-1",
      role: "user",
      kind: "message",
      text: "<subagent_notification>{\"agent_id\":\"child-fixture\"}</subagent_notification>",
      questions: []
    })).toBe(true);
    expect(app.shouldRenderConversationMessage({
      id: "subagent-2",
      role: "user",
      kind: "message",
      text: "<subagent_context>\n- child-fixture: worker\n</subagent_context>",
      questions: []
    })).toBe(true);
    expect(app.shouldRenderConversationMessage({
      id: "assistant-subagent-context",
      role: "assistant",
      kind: "message",
      text: "<subagent_context>\nInternal assistant metadata fixture.\n</subagent_context>",
      questions: []
    })).toBe(false);
  });

  test("plan helper exposes proposed plan body without transcript tags", async () => {
    const app = await import("../test/domain");

    expect(app.extractPlanText("<proposed_plan>\n# Summary\n- Fix it\n</proposed_plan>")).toBe("# Summary\n- Fix it");
    expect(app.extractPlanText("")).toBe("Plan 内容等待 Codex 写入。");
  });

  test("message stream only follows when already near the bottom", async () => {
    const app = await import("../test/domain");

    expect(app.shouldAutoFollowMessageStream({
      scrollTop: 900,
      clientHeight: 600,
      scrollHeight: 1540
    })).toBe(true);
    expect(app.shouldAutoFollowMessageStream({
      scrollTop: 320,
      clientHeight: 600,
      scrollHeight: 1540
    })).toBe(false);
  });
});

  test("desktop demo fixtures are built from macOS-only data", async () => {
    vi.unstubAllEnvs();
    vi.resetModules();
    globalThis.__AGENTHUB_DESKTOP_RUNTIME__ = true;
    const { getPlatformOverview, getProbeStatus, getProbeSettings, getSystemCapabilities, getUpdateStatus, listJobs } = await import("./api");

    const systemCapabilities = await getSystemCapabilities();
    const fixtures = [
      await getPlatformOverview(),
      (await getProbeStatus()).data,
      (await getProbeSettings()).data,
      { ...systemCapabilities, capabilities: undefined },
      await getUpdateStatus(),
      await listJobs()
    ];
    const serialized = JSON.stringify(fixtures);

    expect(systemCapabilities.capabilities).toMatchObject({

      systemd: false,

      linux_update_job: false,
      prune_backups: false
    });
    expect(serialized).not.toMatch(/systemd|Nginx|Turnstile|管理员密码|Linux prune|\/opt\/agenthub|\/home\/ubuntu|192\.0\.2\.10|panel\.example\.com|linux_systemd_job|prune_backups/i);
  });

  test("hidden cleanup helpers expose readiness and disabled state", async () => {
    const app = await import("../test/domain");
    const hiddenPlan = {
      total_threads: 9,
      visible_threads: 7,
      hidden_threads: 2,
      archived_threads: 0,
      session_index_lines: 9,
      rollout_files: 9,
      hidden_ids: ["child-a", "child-b"],
      hidden_source_counts: { exec: 1, subagent: 1 },
      candidates: [{ id: "child-a", title: "Fixture A", fingerprint: "fixture", allowed: true, reason: null }, { id: "child-b", title: "Fixture B", fingerprint: null, allowed: false, reason: "protected" }],
      integrity: "ok"
    };

    expect(app.canStartHiddenThreadDelete(hiddenPlan)).toBe(true);
    expect(app.canStartHiddenThreadDelete({ ...hiddenPlan, hidden_threads: 0, candidates: [] })).toBe(false);
    expect(app.canStartHiddenThreadDelete(null)).toBe(false);
  });
