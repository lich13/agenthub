import { afterEach, expect, test, vi } from "vitest";

afterEach(() => { vi.unstubAllGlobals(); vi.resetModules(); });

test("ownership diagnostics are local reads and never forwarded remotely", async () => {
  const result = { ownership: "inactive", reasonCodes: [], processes: [] };
  const invoke = vi.fn(async (command: string) => command === "remote.select"
    ? { target: "remote", revision: 1, configured: true, sshHost: "fixture-host" } : result);
  vi.stubGlobal("__AGENTHUB_TEST_INVOKE__", invoke);
  const { getClaudeOwnershipDiagnostics } = await import("./claude");
  const runtime = await import("../runtime");
  expect(await getClaudeOwnershipDiagnostics("fixture-key")).toEqual(result);
  expect(invoke).toHaveBeenCalledExactlyOnceWith("claude.ownershipDiagnostics", { sessionKey: "fixture-key" });
  await runtime.remoteSelect("remote");
  invoke.mockClear();
  await expect(getClaudeOwnershipDiagnostics("fixture-key")).rejects.toThrow("仅支持本机");
  expect(invoke).not.toHaveBeenCalled();
});

test("a pending local diagnostic is discarded after the selected machine changes", async () => {
  let finish!: (value: unknown) => void;
  vi.stubGlobal("__AGENTHUB_TEST_INVOKE__", vi.fn(async (command: string) => command === "remote.select"
    ? { target: "remote", revision: 1, configured: true, sshHost: "fixture-host" }
    : new Promise(resolve => { finish = resolve; })));
  const { getClaudeOwnershipDiagnostics } = await import("./claude");
  const pending = getClaudeOwnershipDiagnostics("fixture-key");
  await (await import("../runtime")).remoteSelect("remote");
  finish({ ownership: "inactive", reasonCodes: [], processes: [] });
  await expect(pending).rejects.toThrow("连接已变化");
});


test("Claude main requests omit agentId and child payloads match on local and SSH transports", async () => {
  const invoke = vi.fn(async (command: string) => command === "remote.select"
    ? { target: "remote", revision: 1, configured: true, sshHost: "fixture-host" } : {});
  vi.stubGlobal("__AGENTHUB_TEST_INVOKE__", invoke);
  const { getClaudeSession } = await import("./claude");
  await getClaudeSession("parent-key");
  expect(invoke.mock.calls[0]).toEqual(["claude.detail", { sessionKey: "parent-key", before: undefined, limit: 160 }]);
  invoke.mockClear();
  await getClaudeSession("parent-key", "child-cursor", "child-id");
  const payload = { sessionKey: "parent-key", before: "child-cursor", limit: 160, agentId: "child-id" };
  expect(invoke).toHaveBeenCalledExactlyOnceWith("claude.detail", payload);
  await (await import("../runtime")).remoteSelect("remote");
  invoke.mockClear();
  await getClaudeSession("parent-key", "child-cursor", "child-id");
  expect(invoke).toHaveBeenCalledExactlyOnceWith("remote.invoke", { request: { revision: 1, command: "claude.detail", args: payload } });
});
