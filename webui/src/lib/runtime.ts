import { useSyncExternalStore } from "react";

export type MachineTarget = "local" | "remote";
export type RemoteMachineSummary = { id: string; name: string; baseUrl: string };
export type RemoteConnectionView = {
  target: MachineTarget;
  revision: number;
  baseUrl: string | null;
  configured: boolean;
  activeMachineId?: string | null;
  activeMachineName?: string | null;
  machines?: RemoteMachineSummary[];
};
export type RemoteConnectionCredentials = {
  revision: number;
  baseUrl: string;
  apiKey?: string;
  id?: string;
  name?: string;
};
export type RemoteRevisionRequest = { revision: number; machineId?: string };
export type RemoteSelectionRequest = RemoteRevisionRequest & { target: MachineTarget };
export type RemoteInvokeRequest = { revision: number; command: string; args: Record<string, unknown> };
export type RemoteInvokeResponse = unknown;
export type RuntimeContext = { kind: "desktop" };
type RpcArgs = Record<string, unknown> | undefined;
type RuntimeGlobal = typeof globalThis & {
  __TAURI_INTERNALS__?: {
    invoke?: (command: string, args?: RpcArgs) => Promise<unknown>;
    transformCallback?: (callback: (frame: { index: number; message?: unknown; end?: boolean }) => void) => number;
    unregisterCallback?: (id: number) => void;
  };
  __AGENTHUB_DESKTOP_RUNTIME__?: boolean;
  __AGENTHUB_TEST_INVOKE__?: (command: string, args?: RpcArgs) => Promise<unknown> | unknown;
};
export class RuntimeUnavailableError extends Error {
  constructor(message: string, readonly feature: string) { super(message); this.name = "RuntimeUnavailableError"; }
}
export function runtimeContext(): RuntimeContext { return { kind: "desktop" }; }
export function hasNativeRuntime() {
  const runtime = globalThis as RuntimeGlobal;
  return Boolean(runtime.__TAURI_INTERNALS__?.invoke || runtime.__AGENTHUB_TEST_INVOKE__);
}

export async function invokeNative<T = unknown>(command: string, args?: RpcArgs): Promise<T> {
  const runtime = globalThis as RuntimeGlobal;
  try {
    const invoke = runtime.__AGENTHUB_TEST_INVOKE__ ?? runtime.__TAURI_INTERNALS__?.invoke;
    if (invoke) return await invoke(command, args) as T;
  } catch (reason) {
    throw reason instanceof Error ? reason : new Error(typeof reason === "string" ? reason : "桌面操作失败");
  }
  throw new RuntimeUnavailableError("请使用 AgentHub App", command);
}

/** Channel frames are full views. Drop late/out-of-order fetches so an old
 * native state cannot restore a completed question or approval. */
export function createNativeChannel<T>(receive: (view: T) => void) {
  const runtime = (globalThis as RuntimeGlobal).__TAURI_INTERNALS__;
  if (!runtime?.transformCallback || !runtime.unregisterCallback) throw new Error("当前入口不支持桌面协作，请使用 AgentHub App");
  const unregister = runtime.unregisterCallback;
  let closed = false;
  let last = -1;
  const id = runtime.transformCallback(frame => {
    if (closed || !Number.isSafeInteger(frame.index) || frame.index <= last) return;
    last = frame.index;
    if (frame.end) { closed = true; unregister(id); return; }
    if (frame.message) receive(frame.message as T);
  });
  return { value: `__CHANNEL__:${id}`, close: () => { if (!closed) { closed = true; unregister(id); } } };
}

const listeners = new Set<() => void>();
const readCleanups = new Set<() => Promise<void>>();
export function beforeMachineChange(cleanup: () => Promise<void>): () => void {
  readCleanups.add(cleanup);
  return () => { readCleanups.delete(cleanup); };
}
let connection = {
  target: "local" as MachineTarget,
  revision: 0,
  baseUrl: null as string | null,
  configured: false,
  activeMachineId: null as string | null,
  activeMachineName: null as string | null,
  machines: [] as RemoteMachineSummary[],
  ready: false,
  writes: 0,
  changing: false,
  error: "",
  notice: ""
};
function normalizeIncomingView(view: RemoteConnectionView) {
  return {
    target: view.target,
    revision: view.revision,
    baseUrl: view.baseUrl ?? null,
    configured: view.configured,
    activeMachineId: view.activeMachineId ?? null,
    activeMachineName: view.activeMachineName ?? null,
    machines: view.machines ?? [],
  };
}
function publish(patch: Partial<typeof connection>) { connection = { ...connection, ...patch }; listeners.forEach(listener => listener()); }
export function connectionSnapshot() { return connection; }
export function machineScope() {
  if (connection.target !== "remote") return "local:";
  return connection.activeMachineId
    ? `remote:${connection.activeMachineId}:${connection.baseUrl ?? ""}`
    : `remote:${connection.baseUrl ?? ""}`;
}
export function useConnection() { return useSyncExternalStore(listener => { listeners.add(listener); return () => listeners.delete(listener); }, connectionSnapshot, connectionSnapshot); }
let initialization: Promise<void> | undefined;
export function initializeConnection() {
  return initialization ??= (async () => {
    try { publish({ ...normalizeIncomingView(await remoteGet()), ready: true, error: "" }); }
    catch (error) { publish({ ready: true, error: (error as Error).message }); }
  })();
}
export function remoteGet() { return invokeNative<RemoteConnectionView>("remote.get"); }
async function changeConnection<T>(command: string, request: unknown): Promise<T> {
  if (connection.writes || connection.changing) throw new Error("操作进行中，请稍后切换");
  if (command !== "remote.verify" && readCleanups.size > 0) {
    let timeout: ReturnType<typeof setTimeout> | undefined;
    await Promise.race([
      Promise.allSettled([...readCleanups].map(cleanup => cleanup())),
      new Promise(resolve => { timeout = setTimeout(resolve, 1000); })
    ]);
    clearTimeout(timeout);
    if (connection.writes || connection.changing) throw new Error("操作进行中，请稍后切换");
  }
  publish({ changing: true });
  try {
    const result = await invokeNative<T>(command, { request });
    if (command !== "remote.verify") {
      publish({
        ...normalizeIncomingView(result as RemoteConnectionView),
        error: "",
        notice: command === "remote.save" ? "已保存" : ""
      });
    }
    return result;
  }
  finally { publish({ changing: false }); }
}
export async function remoteVerify(request: RemoteConnectionCredentials) {
  return changeConnection<import("../types").SystemCapabilitiesResponse>("remote.verify", request);
}
export async function remoteSave(request: RemoteConnectionCredentials) {
  const view = await changeConnection<RemoteConnectionView>("remote.save", request);
  return view;
}
export async function remoteRemove(machineId?: string) {
  const payload: RemoteRevisionRequest = machineId
    ? { revision: connection.revision, machineId }
    : { revision: connection.revision };
  const view = await changeConnection<RemoteConnectionView>("remote.remove", payload);
  return view;
}
export async function remoteSelect(target: MachineTarget, machineId?: string) {
  if (target === "local" && connection.target === "local") return connection;
  if (
    target === "remote" &&
    connection.target === "remote" &&
    (!machineId || connection.activeMachineId === machineId)
  ) {
    return connection;
  }
  const payload: RemoteSelectionRequest = machineId
    ? { revision: connection.revision, target, machineId }
    : { revision: connection.revision, target };
  const view = await changeConnection<RemoteConnectionView>("remote.select", payload);
  return view;
}
export function remoteInvoke<T = unknown>(request: RemoteInvokeRequest) { return invokeNative<T>("remote.invoke", { request }); }

const reads = new Set(["cleanup.hiddenDryRun", "system.capabilities", "system.version", "system.platform", "system.providers", "threads.list", "threads.detail", "threads.subagentDetail", "threads.blocks", "jobs.list", "jobs.detail", "probe.status", "probe.settings.get", "probe.events", "updates.status", "grok.list", "grok.detail", "grok.deletePreview", "claude.list", "claude.detail", "claude.deletePreview", "antigravity.list", "antigravity.detail", "dsh.list", "dsh.detail", "sessions.attachmentRead", "sessions.search", "sessions.bulkPreview"]);
// Local desktop subscriptions/catalogs are reads, but never routed to remote.invoke.
reads.add("codex.desktop.attach");
reads.add("codex.desktop.catalog");
reads.add("codex.desktop.diagnostics");
const pendingJobs = new Set<string>();
export async function runtimeRpc<T = unknown>(command: string, args?: RpcArgs, localOnly = false): Promise<T> {
  if (connection.changing) throw new Error("连接切换中，请稍后重试");
  const captured = connection;
  const write = !reads.has(command);
  if (write) publish({ writes: connection.writes + 1 });
  try {
    const value = captured.target === "remote" && !localOnly
      ? await remoteInvoke<T>({ revision: captured.revision, command, args: args ?? {} })
      : await invokeNative<T>(command, args);
    if (captured.revision !== connection.revision) throw new Error("连接已变化，已丢弃旧响应");
    if (value && typeof value === "object") {
      const result = value as Record<string, unknown>;
      const jobId = result.job_id ?? result.jobId;
      if (write && typeof jobId === "string" && jobId && !pendingJobs.has(jobId)) {
        // A server write continues after its RPC returns; the existing job poller
        // releases this machine lock only after observing a terminal result.
        pendingJobs.add(jobId);
        publish({ writes: connection.writes + 1 });
      }
      if (command === "jobs.detail" && typeof result.id === "string"
        && ["succeeded", "failed", "cancelled"].includes(String(result.status))
        && pendingJobs.delete(result.id)) {
        publish({ writes: connection.writes - 1 });
      }
    }
    return value;
  } finally { if (write) publish({ writes: connection.writes - 1 }); }
}
