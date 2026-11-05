import { expect, type Page } from "@playwright/test";
import * as demo from "../src/lib/api/demo";
import type { RemoteConnectionView } from "../src/lib/runtime";

type MockArgs = Record<string, any>;
type MockHandler = (args: MockArgs, next: () => Promise<unknown>) => unknown;
const mockStates = new WeakMap<Page, { overrides: Map<string, MockHandler>; observers: Array<(command: string, args: MockArgs) => void> }>();

export async function mockCommand(page: Page, command: string, handler: MockHandler) {
  const state = mockStates.get(page);
  if (!state) throw new Error("Install mockApi before adding command overrides");
  state.overrides.set(command, handler);
}

export function observeCommands(page: Page, observer: (command: string, args: MockArgs) => void) {
  mockStates.get(page)!.observers.push(observer);
}

export async function mockApi(page: Page, options: { connection?: Partial<RemoteConnectionView> } = {}) {
  const state = { overrides: new Map<string, MockHandler>(), observers: [] as Array<(command: string, args: MockArgs) => void> };
  mockStates.set(page, state);
  let connection: RemoteConnectionView = { target: "local", revision: 0, sshHost: null, configured: false, ...options.connection };
  if(connection.configured && !connection.machines?.length){
    connection.machines=[{id:"remote",name:"Fixture remote",sshHost:connection.sshHost??"fixture-host",useSudo:true}];
    connection.activeMachineId="remote";connection.activeMachineName="Fixture remote";
  }
  const calls: string[] = [];
  const titles = new Map<string, string>();
  const archived = new Set<string>(demo.demoThreads(true, "").map((thread) => thread.id));
  let probeSettings = demo.demoProbeSettings(connection.target === "remote" ? "linux-api" : "macos-tauri");
  probeSettings.notifications = { ...probeSettings.notifications, enabled: true, device_key_configured: true };
  let jobReads = 0;
  let grok = [{ id: "grok-fixture", title: "Grok fixture", cwd: "/isolated/workspace", path: "/isolated/sessions/grok-fixture", handoffPath: "/isolated/sessions/grok-fixture/chat_history.jsonl", messageCount: 2, status: "recent" }];
  let antigravity = [{
    id: "ag-fixture",
    sessionKey: "ag:ag-fixture",
    title: "Antigravity fixture",
    cwd: "/isolated/antigravity-workspace",
    path: "/isolated/antigravity/brain/ag-fixture",
    archived: false,
    updatedAt: "2026-10-01T00:00:00Z",
    messageCount: 2,
    lastMessage: "Antigravity fixture result",
    status: "recent",
    formatVersion: "transcript-v1",
    storageSize: { status: "ready", bytes: 4096 },
    canRename: true,
    canArchive: true,
    canDelete: true
  }];
  let dsh = [{
    id: "dsh-fixture",
    sessionKey: "dsh:dsh-fixture",
    title: "DSH fixture",
    cwd: "/isolated/dsh-workspace",
    path: "/isolated/dsh/sessions/dsh-fixture",
    archived: false,
    updatedAt: "2026-10-01T00:00:00Z",
    messageCount: 2,
    lastMessage: "DSH fixture result",
    status: "recent",
    formatVersion: "dsh-zstd-v2",
    storageSize: { status: "ready", bytes: 2048 },
    canRename: true,
    canArchive: true,
    canDelete: true
  }];
  const dispatch = async (name: string, args: MockArgs = {}, target: "local" | "remote" = "local"): Promise<unknown> => {
    const fixture = () => target === "remote" ? "linux-api" as const : "macos-tauri" as const;
    calls.push(name);
    state.observers.forEach(observer => observer(name, args));
    const respond = async (): Promise<unknown> => {
      if (name === "remote.get") return { ...connection };
      if (name === "remote.sshHosts") return [{alias:"fixture-host"}, {alias:"fixture-second"}];
      if (name === "remote.verify") return demo.demoSystemCapabilities("linux-api");
      if (name === "remote.save" || name === "remote.install") {
        const existing = [...(connection.machines ?? [])];
        const reqId = typeof args.request.id === "string" && args.request.id ? args.request.id : null;
        const matched = (reqId ? existing.find(m => m.id === reqId) : existing.find(m => m.sshHost === args.request.sshHost)) ?? null;
        const id = matched?.id ?? `m-${existing.length + 1}`;
        const defaultHost = args.request.sshHost;
        const machineName = typeof args.request.name === "string" && args.request.name.trim() ? args.request.name.trim() : (matched?.name ?? defaultHost);
        const nextMachines = matched
          ? existing.map(m => m.id === id ? { id, name: machineName, sshHost: args.request.sshHost, useSudo: args.request.useSudo } : m)
          : [...existing, { id, name: machineName, sshHost: args.request.sshHost, useSudo: args.request.useSudo }];
        connection = {
          target: "remote",
          revision: connection.revision + 1,
          sshHost: args.request.sshHost,
          configured: true,
          activeMachineId: id,
          activeMachineName: machineName,
          machines: nextMachines,
        };
        return { ...connection };
      }
      if (name === "remote.select") {
        const list = connection.machines ?? [];
        const picked = args.request.machineId ? list.find(m => m.id === args.request.machineId) : (list.find(m => m.id === connection.activeMachineId) ?? list[0]);
        connection = {
          ...connection,
          target: args.request.target,
          revision: connection.revision + 1,
          ...(picked ? { sshHost: picked.sshHost, activeMachineId: picked.id, activeMachineName: picked.name } : {}),
        };
        return { ...connection };
      }
      if (name === "remote.remove") {
        const list = connection.machines ?? [];
        const targetId = args.request.machineId ?? connection.activeMachineId ?? list[0]?.id;
        const remaining = targetId ? list.filter(m => m.id !== targetId) : [];
        const nextActive = remaining[0] ?? null;
        connection = {
          target: "local",
          revision: connection.revision + 1,
          sshHost: nextActive?.sshHost ?? null,
          configured: remaining.length > 0,
          activeMachineId: nextActive?.id ?? null,
          activeMachineName: nextActive?.name ?? null,
          machines: remaining,
        };
        return { ...connection };
      }
      if (name === "remote.invoke") return dispatch(args.request.command, args.request.args, "remote");
      if (name === "grok.detail" && !grok.length) throw new Error("Grok session not found");
      const responses: Record<string, () => unknown> = {
        "system.capabilities": () => {
          const view = demo.demoSystemCapabilities(fixture());
          return {
            ...view,
            capabilities: {
              ...view.capabilities,
              antigravity_notifications: view.capabilities.antigravity_notifications,
              dsh_notifications: view.capabilities.dsh_notifications,
            },
          };
        },
        "claude.hooks": () => ({
          installed: true,
          managedCount: 3,
          changed: false,
          configPath: `/isolated/${args.provider ?? "claude"}/hooks.json`,
        }),
        "system.version": demo.demoSystemVersion,
        "system.platform": () => demo.demoPlatformOverview(fixture()),
        "threads.list": () => {
          const wantArchived = Boolean(args.archived);
          const q = String(args.q ?? "").toLowerCase();
          return [...demo.demoThreads(false, ""), ...demo.demoThreads(true, "")]
            .map((thread) => {
              const isArch = archived.has(thread.id);
              return {
                ...thread,
                title: titles.get(thread.id) ?? thread.title,
                status: isArch ? ("Archived" as const) : thread.status === "Archived" ? ("Recent" as const) : thread.status,
                archived_at: isArch ? (thread.archived_at ?? "2026-10-01T00:00:00Z") : null,
              };
            })
            .filter((thread) => (thread.status === "Archived") === wantArchived && (!q || thread.title.toLowerCase().includes(q) || thread.id.toLowerCase().includes(q)));
        },
        "threads.detail": () => {
          const detail = demo.demoThreadDetail(args.id);
          const isArch = archived.has(args.id);
          return {
            ...detail,
            summary: {
              ...detail.summary,
              title: titles.get(args.id) ?? detail.summary.title,
              status: isArch ? ("Archived" as const) : detail.summary.status === "Archived" ? ("Recent" as const) : detail.summary.status,
              archived_at: isArch ? (detail.summary.archived_at ?? "2026-10-01T00:00:00Z") : null,
            },
          };
        },
        "threads.rename": () => { titles.set(args.threadId, args.name); return { ok: true }; },
        "threads.archive": () => { archived.add(args.threadId); return { ok: true }; },
        "threads.restore": () => { archived.delete(args.threadId); return { ok: true }; },
        "threads.blocks": () => demo.demoThreadBlockPage(args.id),
        "updates.status": () => demo.demoUpdateStatus(fixture()),
        "jobs.list": () => demo.demoJobs().map(job => ({...job, output:""})),
        "jobs.detail": () => ({ ...demo.demoJob(args.id), status: ++jobReads < 3 ? "running" : "succeeded" }),
        "updates.check": () => ({ job_id: "fixture-job" }),
        "probe.status": () => ({ available: true, data: demo.demoProbeStatus(fixture()) }),
        "probe.settings.get": () => ({ available: true, data: probeSettings }),
        "probe.settings.save": () => {
          const input = args.settings;
          probeSettings = {
            ...probeSettings,
            codex: { ...probeSettings.codex, ...input.codex },
            probe: { ...probeSettings.probe, ...input.probe },
            notifications: { ...probeSettings.notifications, ...input.probe.notifications }
          };
          return probeSettings;
        },
        "probe.barkTest": () => ({ job_id: "fixture-job" }),
        "cleanup.archiveDryRun": demo.demoArchiveDeletePlan,
        "cleanup.archiveExecute": demo.demoArchiveDeleteResult,
        "cleanup.hiddenDryRun": demo.demoHiddenThreadDeletePlan,
        "cleanup.hiddenExecute": demo.demoHiddenThreadDeleteResult,
        "cleanup.unifiedDryRun": () => demo.demoUnifiedCleanupPlan(fixture(), args.request),
        "cleanup.unifiedExecute": () => demo.demoUnifiedCleanupResult(args.request),
        "probe.events": demo.demoProbeEvents,
        "grok.list": () => grok.filter((item) => item.title.includes(args.q ?? "")),
        "grok.detail": () => ({ summary: grok[0], events: [{ kind: "user_message_chunk", text: "Check the isolated fixture." }, { kind: "assistant_message_chunk", text: "## Result\n\nReadable response with a [link](https://example.com).\n\n```sh\nprintf test\n```" }] }),
        "grok.rename": () => { grok = grok.map((item) => ({ ...item, title: args.title })); return grok[0]; },
        "grok.deletePreview": () => ({ ...grok[0], fingerprint: "fixture-fingerprint", fileCount: 2, bytes: 128 }),
        "grok.deleteExecute": () => { const request = args.request; if (!request.confirmed || request.id !== grok[0]?.id) throw new Error("missing confirmation"); grok = []; return { id: request.id, deleted: true, bytes: 128 }; },
        "claude.list": () => [],
        "antigravity.list": () => {
          const req = args.request ?? {};
          const wantArchived = Boolean(req.archived);
          const q = String(req.q ?? "").toLowerCase();
          return antigravity.filter((item) => item.archived === wantArchived && (!q || item.title.toLowerCase().includes(q) || item.id.toLowerCase().includes(q) || item.cwd.toLowerCase().includes(q)));
        },
        "antigravity.detail": () => {
          const key = args.request?.sessionKey;
          const summary = antigravity.find((item) => item.sessionKey === key) ?? antigravity[0];
          if (!summary) throw new Error("Antigravity session not found");
          return {
            summary,
            events: [
              { id: "ag-user-1", kind: "user_message", text: "Inspect isolated Antigravity fixture." },
              { id: "ag-think-1", kind: "thinking", text: "Isolated reasoning step." },
              { id: "ag-tool-1", kind: "tool_call", callId: "ag-call-1", role: "run_command", status: "completed", detail: "printf ok" },
              { id: "ag-out-1", kind: "tool_result", callId: "ag-call-1", status: "completed", result: "ok" },
              { id: "ag-reply-1", kind: "assistant_message", text: "## Antigravity\n\nAntigravity fixture result." }
            ],
            totalEvents: 5,
            hasMore: false
          };
        },
        "antigravity.rename": () => {
          const { sessionKey, title } = args.request;
          antigravity = antigravity.map((item) => item.sessionKey === sessionKey ? { ...item, title } : item);
          return antigravity.find((item) => item.sessionKey === sessionKey) ?? antigravity[0];
        },
        "antigravity.archive": () => {
          const { sessionKey } = args.request;
          antigravity = antigravity.map((item) => item.sessionKey === sessionKey ? { ...item, archived: true } : item);
          return antigravity.find((item) => item.sessionKey === sessionKey) ?? antigravity[0];
        },
        "antigravity.restore": () => {
          const { sessionKey } = args.request;
          antigravity = antigravity.map((item) => item.sessionKey === sessionKey ? { ...item, archived: false } : item);
          return antigravity.find((item) => item.sessionKey === sessionKey) ?? antigravity[0];
        },
        "dsh.list": () => {
          const req = args.request ?? {};
          const wantArchived = Boolean(req.archived);
          const q = String(req.q ?? "").toLowerCase();
          return dsh.filter((item) => item.archived === wantArchived && (!q || item.title.toLowerCase().includes(q) || item.id.toLowerCase().includes(q) || item.cwd.toLowerCase().includes(q)));
        },
        "dsh.detail": () => {
          const key = args.request?.sessionKey;
          const summary = dsh.find((item) => item.sessionKey === key) ?? dsh[0];
          if (!summary) throw new Error("DSH session not found");
          return {
            summary,
            events: [
              { id: "dsh-user-1", kind: "user_message", text: "Inspect isolated DSH fixture." },
              { id: "dsh-think-1", kind: "thinking", text: "Isolated DSH thinking." },
              { id: "dsh-reply-1", kind: "assistant_message", text: "## DeepSeek Harness\n\nDSH fixture result." }
            ],
            totalEvents: 3,
            hasMore: false
          };
        },
        "dsh.rename": () => {
          const { sessionKey, title } = args.request;
          dsh = dsh.map((item) => item.sessionKey === sessionKey ? { ...item, title } : item);
          return dsh.find((item) => item.sessionKey === sessionKey) ?? dsh[0];
        },
        "dsh.archive": () => {
          const { sessionKey } = args.request;
          dsh = dsh.map((item) => item.sessionKey === sessionKey ? { ...item, archived: true } : item);
          return dsh.find((item) => item.sessionKey === sessionKey) ?? dsh[0];
        },
        "dsh.restore": () => {
          const { sessionKey } = args.request;
          dsh = dsh.map((item) => item.sessionKey === sessionKey ? { ...item, archived: false } : item);
          return dsh.find((item) => item.sessionKey === sessionKey) ?? dsh[0];
        },
      };
      const response = responses[name];
      if (!response) throw new Error(`Unmocked command: ${name}`);
      return response();
    };
    const override = state.overrides.get(name);
    return override ? override(args, respond) : respond();
  };
  await page.exposeFunction("__agenthubInvoke", dispatch);
  await page.addInitScript(() => {
    (globalThis as any).__AGENTHUB_TEST_INVOKE__ = (command: string, args?: Record<string, unknown>) => (globalThis as any).__agenthubInvoke(command, args);
  });
  return calls;
}

export async function assertContrast(page: Page, selector: string, minimum = 4.5, property = "color") {
  const samples = await page.locator(selector).evaluateAll((elements, property) => {
    const rgb = (value: string) => (value.match(/[\d.]+/g) ?? []).map(Number);
    const luminance = (color: number[]) => color.slice(0, 3).reduce((sum, channel, i) => {
      const c = channel / 255;
      return sum + (c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4) * [0.2126, 0.7152, 0.0722][i];
    }, 0);
    return elements.filter((element) => element.getClientRects().length).map((element) => {
      const chain: Element[] = [];
      for (let parent: Element | null = element; parent; parent = parent.parentElement) chain.unshift(parent);
      let background = [255, 255, 255];
      for (const parent of chain) {
        const color = rgb(getComputedStyle(parent).backgroundColor);
        const alpha = color[3] ?? 1;
        background = background.map((channel, i) => (color[i] ?? channel) * alpha + channel * (1 - alpha));
      }
      const color = rgb(getComputedStyle(element).getPropertyValue(property));
      const foreground = background.map((channel, i) => color[i] * (color[3] ?? 1) + channel * (1 - (color[3] ?? 1)));
      const a = luminance(foreground), b = luminance(background);
      return { text: element.textContent?.slice(0, 60) || element.tagName, ratio: (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05) };
    });
  }, property);
  expect(samples.length, `visible samples for ${selector}`).toBeGreaterThan(0);
  for (const sample of samples) expect(sample.ratio, `${sample.text}: contrast ${sample.ratio.toFixed(2)}`).toBeGreaterThanOrEqual(minimum);
}

export async function assertNoOverflow(page: Page) {
  const widths = await page.evaluate(() => ({ scroll: document.documentElement.scrollWidth, available: document.documentElement.clientWidth }));
  expect(widths.scroll, `document width ${widths.scroll}, available width ${widths.available}`).toBeLessThanOrEqual(widths.available);
}
