import { afterEach, describe, expect, test, vi } from "vitest";
import type { RemoteConnectionView } from "./runtime";

const local: RemoteConnectionView = { target: "local", revision: 0, baseUrl: null, configured: false };
const remote: RemoteConnectionView = { target: "remote", revision: 1, baseUrl: "https://api.example.com/agenthub/", configured: true };
const deferred = <T>() => {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>(done => { resolve = done; });
  return { promise, resolve };
};

async function loadRuntime(handler: (command: string, args?: Record<string, unknown>) => unknown = () => local) {
  vi.resetModules();
  const invoke = vi.fn(handler);
  vi.stubGlobal("__AGENTHUB_TEST_INVOKE__", invoke);
  const runtime = await import("./runtime");
  return { ...runtime, invoke };
}

afterEach(() => { vi.unstubAllGlobals(); vi.restoreAllMocks(); vi.resetModules(); });

describe("native machine connection", () => {
  test("initializes once from the native view without exposing credentials", async () => {
    const runtime = await loadRuntime(() => remote);
    await Promise.all([runtime.initializeConnection(), runtime.initializeConnection()]);
    expect(runtime.invoke).toHaveBeenCalledExactlyOnceWith("remote.get", undefined);
    expect(runtime.connectionSnapshot()).toMatchObject({ ...remote, ready: true, writes: 0 });
    expect(runtime.connectionSnapshot()).not.toHaveProperty("apiKey");
    expect(runtime.machineScope()).toBe("remote:https://api.example.com/agenthub/");
  });

  test("local business calls retain the typed native command and never use browser fetch", async () => {
    const fetch = vi.fn(); vi.stubGlobal("fetch", fetch);
    const runtime = await loadRuntime((command, args) => ({ command, args }));
    await expect(runtime.runtimeRpc("threads.list", { status: "all", q: "plan", limit: 20 })).resolves.toEqual({
      command: "threads.list", args: { status: "all", q: "plan", limit: 20 }
    });
    expect(fetch).not.toHaveBeenCalled();
  });

  test("remote selection routes the same business identity through its revision", async () => {
    const runtime = await loadRuntime((command, args) => command === "remote.select" ? remote : { command, args });
    await runtime.remoteSelect("remote");
    await expect(runtime.runtimeRpc("threads.detail", { id: "same-thread" })).resolves.toEqual({
      command: "remote.invoke", args: { request: { revision: 1, command: "threads.detail", args: { id: "same-thread" } } }
    });
    expect(runtime.invoke).toHaveBeenCalledWith("remote.select", { request: { target: "remote", revision: 0 } });
  });

  test("a read finishing after a connection switch is discarded", async () => {
    const oldRead = deferred<unknown>();
    const runtime = await loadRuntime(command => command === "remote.select" ? remote : oldRead.promise);
    const pending = runtime.runtimeRpc("threads.detail", { id: "same-thread" });
    const rejected = expect(pending).rejects.toThrow("已丢弃旧响应");
    await runtime.remoteSelect("remote");
    oldRead.resolve({ title: "local stale title" });
    await rejected;
    expect(runtime.connectionSnapshot()).toMatchObject(remote);
  });

  test("writes block select, save, verify and remove until the native mutation settles", async () => {
    const mutation = deferred<unknown>();
    const runtime = await loadRuntime(command => command === "threads.rename" ? mutation.promise : remote);
    const pending = runtime.runtimeRpc("threads.rename", { threadId: "same-thread", name: "New title" });
    expect(runtime.connectionSnapshot().writes).toBe(1);
    for (const operation of [
      () => runtime.remoteSelect("remote"),
      () => runtime.remoteSave({ revision: 0, baseUrl: remote.baseUrl!, apiKey: "fixture-key" }),
      () => runtime.remoteVerify({ revision: 0, baseUrl: remote.baseUrl!, apiKey: "fixture-key" }),
      () => runtime.remoteRemove()
    ]) await expect(operation()).rejects.toThrow("操作进行中，请稍后切换");
    expect(runtime.invoke).toHaveBeenCalledTimes(1);
    mutation.resolve({ ok: true }); await pending;
    await runtime.remoteSelect("remote");
    expect(runtime.connectionSnapshot()).toMatchObject({ ...remote, writes: 0 });
  });

  test("a rejected mutation releases the write lock", async () => {
    const runtime = await loadRuntime(command => {
      if (command === "threads.rename") throw "Native rename failed";
      return remote;
    });
    await expect(runtime.runtimeRpc("threads.rename")).rejects.toThrow("Native rename failed");
    expect(runtime.connectionSnapshot().writes).toBe(0);
    await runtime.remoteSelect("remote");
  });

  test.each(["succeeded", "failed", "cancelled"])("background writes block switching until the job is %s", async status => {
    let phase = "running";
    const runtime = await loadRuntime(command => {
      if (command === "updates.check") return { job_id: "update-1" };
      if (command === "jobs.detail") return { id: "update-1", status: phase };
      return remote;
    });
    await runtime.runtimeRpc("updates.check");
    expect(runtime.connectionSnapshot().writes).toBe(1);
    await expect(runtime.remoteSelect("remote")).rejects.toThrow("操作进行中");
    await runtime.runtimeRpc("jobs.detail", { id: "update-1" });
    expect(runtime.connectionSnapshot().writes).toBe(1);
    phase = status;
    await runtime.runtimeRpc("jobs.detail", { id: "update-1" });
    await runtime.runtimeRpc("jobs.detail", { id: "update-1" });
    expect(runtime.connectionSnapshot().writes).toBe(0);
    await runtime.remoteSelect("remote");
    expect(runtime.connectionSnapshot().target).toBe("remote");
  });

  test("an offline remote read stays remote and never falls back to local data", async () => {
    const runtime = await loadRuntime(command => {
      if (command === "remote.select") return remote;
      throw new Error("远程连接失败");
    });
    await runtime.remoteSelect("remote");
    await expect(runtime.runtimeRpc("threads.list")).rejects.toThrow("远程连接失败");
    expect(runtime.connectionSnapshot()).toMatchObject(remote);
    expect(runtime.invoke.mock.calls.map(([command]) => command)).toEqual(["remote.select", "remote.invoke"]);
  });

  test("credentials are sent only to save and are absent from subsequent views and business calls", async () => {
    const runtime = await loadRuntime(command => command === "remote.save" ? remote : []);
    const credentials = { revision: 0, baseUrl: remote.baseUrl!, apiKey: "fixture-secret-key" };
    await runtime.remoteSave(credentials);
    expect(runtime.invoke).toHaveBeenCalledWith("remote.save", { request: credentials });
    expect(JSON.stringify(runtime.connectionSnapshot())).not.toContain(credentials.apiKey);
    await runtime.runtimeRpc("threads.list");
    expect(runtime.invoke.mock.calls[1]).toEqual(["remote.invoke", { request: { revision: 1, command: "threads.list", args: {} } }]);
  });

  test("native Plan saving stays local while update reads use the selected machine", async () => {
    const runtime = await loadRuntime((command, args) => command === "remote.select" ? remote : { command, args });
    await runtime.remoteSelect("remote");
    await expect(runtime.runtimeRpc("plans.save", {}, true)).resolves.toEqual({ command: "plans.save", args: {} });
    await runtime.runtimeRpc("updates.status");
    expect(runtime.invoke.mock.calls.map(([command]) => command)).toEqual(["remote.select", "plans.save", "remote.invoke"]);
    expect(runtime.connectionSnapshot()).toMatchObject({ ...remote, writes: 0 });
  });

  test("waits for registered read cleanup before switching and skips it during verification", async () => {
    const cleanup = deferred<void>();
    const runtime = await loadRuntime(command => command === "remote.verify" ? {} : remote);
    const beforeChange = vi.fn(() => cleanup.promise);
    const unsubscribe = runtime.beforeMachineChange(beforeChange);

    await runtime.remoteVerify({ revision: 0, baseUrl: remote.baseUrl!, apiKey: "" });
    expect(beforeChange).not.toHaveBeenCalled();

    const switching = runtime.remoteSelect("remote");
    expect(beforeChange).toHaveBeenCalledOnce();
    expect(runtime.invoke.mock.calls.map(([command]) => command)).not.toContain("remote.select");
    cleanup.resolve();
    await switching;
    expect(runtime.invoke.mock.calls.map(([command]) => command)).toContain("remote.select");
    unsubscribe();
  });

  test("hidden cleanup dry-run is read-only and its stale response is discarded after switching", async () => {
    const dryRun = deferred<unknown>();
    const runtime = await loadRuntime(command => command === "cleanup.hiddenDryRun" ? dryRun.promise : remote);
    const pending = runtime.runtimeRpc("cleanup.hiddenDryRun", { mode: "start" });
    expect(runtime.connectionSnapshot().writes).toBe(0);
    const discarded = expect(pending).rejects.toThrow("连接已变化，已丢弃旧响应");

    await runtime.remoteSelect("remote");
    dryRun.resolve({ candidates: [] });
    await discarded;
    expect(runtime.connectionSnapshot()).toMatchObject({ ...remote, writes: 0 });
  });

  test("an uninitialized browser refuses business calls without a native bridge", async () => {
    vi.resetModules(); vi.stubGlobal("__AGENTHUB_TEST_INVOKE__", undefined);
    const fetch = vi.fn(); vi.stubGlobal("fetch", fetch);
    const { runtimeRpc } = await import("./runtime");
    await expect(runtimeRpc("threads.list")).rejects.toThrow("请使用 AgentHub App");
    expect(fetch).not.toHaveBeenCalled();
  });

  test("switches across multiple named remote machines and isolates machineScope per machineId", async () => {
    const machines = [
      { id: "m-alpha", name: "主力节点", baseUrl: "https://alpha.example.com/agenthub/" },
      { id: "m-beta", name: "构建节点", baseUrl: "https://beta.example.com/agenthub/" },
    ];
    let currentView: RemoteConnectionView = {
      target: "remote",
      revision: 1,
      baseUrl: machines[0].baseUrl,
      configured: true,
      activeMachineId: "m-alpha",
      activeMachineName: "主力节点",
      machines,
    };
    const runtime = await loadRuntime((command, args) => {
      if (command === "remote.get") return currentView;
      if (command === "remote.select") {
        const req = (args as { request: { target: "local" | "remote"; machineId?: string; revision: number } }).request;
        const picked = machines.find(m => m.id === req.machineId) ?? machines[0];
        currentView = {
          ...currentView,
          target: req.target,
          revision: req.revision + 1,
          baseUrl: picked.baseUrl,
          activeMachineId: picked.id,
          activeMachineName: picked.name,
        };
        return currentView;
      }
      if (command === "remote.remove") {
        const req = (args as { request: { machineId?: string; revision: number } }).request;
        const remaining = machines.filter(m => m.id !== req.machineId);
        currentView = {
          target: "local",
          revision: req.revision + 1,
          baseUrl: remaining[0]?.baseUrl ?? null,
          configured: remaining.length > 0,
          activeMachineId: remaining[0]?.id ?? null,
          activeMachineName: remaining[0]?.name ?? null,
          machines: remaining,
        };
        return currentView;
      }
      return { command, args };
    });

    await runtime.initializeConnection();
    expect(runtime.machineScope()).toBe("remote:m-alpha:https://alpha.example.com/agenthub/");

    // Selecting the already-active machineId is a no-op.
    await runtime.remoteSelect("remote", "m-alpha");
    expect(runtime.invoke).toHaveBeenCalledTimes(1);

    // Switching to another named remote machine updates activeMachineId and machineScope.
    await runtime.remoteSelect("remote", "m-beta");
    expect(runtime.invoke).toHaveBeenLastCalledWith("remote.select", {
      request: { revision: 1, target: "remote", machineId: "m-beta" },
    });
    expect(runtime.machineScope()).toBe("remote:m-beta:https://beta.example.com/agenthub/");

    // Removing a specific machine passes machineId and resets to local when active was removed.
    await runtime.remoteRemove("m-beta");
    expect(runtime.invoke).toHaveBeenLastCalledWith("remote.remove", {
      request: { revision: 2, machineId: "m-beta" },
    });
    expect(runtime.machineScope()).toBe("local:");
    expect(runtime.connectionSnapshot().machines).toHaveLength(1);
  });
});
