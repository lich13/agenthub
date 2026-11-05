import { afterEach, describe, expect, test, vi } from "vitest";
import { runtimeCapabilitiesFromResponse } from "../domain/capabilities";
import type { DesktopAttachmentEvent, DesktopView } from "./desktop";

type ChannelFrame = { index: number; message?: DesktopView | DesktopAttachmentEvent; end?: boolean };
type ChannelCallback = (frame: ChannelFrame) => void;
type InvokeHandler = (command: string, args?: Record<string, unknown>) => unknown;

function view(threadId: string, sessionId = "session-fixture", revision = 1): DesktopView {
  return {
    sessionId, threadId, revision, connected: true, writable: true, runtime: "codex",
    activeTurnId: null, model: null, reasoningEffort: null, mode: null,
    requests: [], queue: [], error: null
  };
}

function installBridge(handler?: InvokeHandler) {
  const callbacks = new Map<number, ChannelCallback>();
  let nextId = 1;
  const transformCallback = vi.fn((callback: ChannelCallback) => {
    const id = nextId++;
    callbacks.set(id, callback);
    return id;
  });
  const unregisterCallback = vi.fn((id: number) => { callbacks.delete(id); });
  const invoke = vi.fn(async (command: string, args?: Record<string, unknown>) => {
    if (handler) return await handler(command, args);
    if (command === "remote.select") {
      return { target: "remote", revision: 1, sshHost: "fixture-host", configured: true };
    }
    if (command === "codex.desktop.attach" || command === "codex.desktop.action") return view("thread-fixture");
    if (command === "codex.desktop.attachments") return { bindingId: "binding-fixture" };
    if (command === "codex.desktop.detach") return { ok: true };
    return {};
  });
  vi.stubGlobal("__TAURI_INTERNALS__", { transformCallback, unregisterCallback });
  vi.stubGlobal("__AGENTHUB_TEST_INVOKE__", invoke);
  return { callbacks, transformCallback, unregisterCallback, invoke };
}

afterEach(() => {
  vi.unstubAllGlobals();
  vi.resetModules();
});

describe("Codex desktop transport", () => {
  test("keeps only the newest full snapshot when channel frames arrive out of order", async () => {
    const bridge = installBridge();
    const { attachDesktopThread } = await import("./desktop");
    const receive = vi.fn();
    const attached = await attachDesktopThread("thread-fixture", receive);
    const emit = bridge.callbacks.get(1)!;
    const newest = view("thread-fixture", "session-fixture", 8);

    emit({ index: 8, message: newest });
    emit({ index: 7, message: view("thread-fixture", "session-fixture", 7) });

    expect(receive).toHaveBeenCalledExactlyOnceWith(newest);
    expect(attached.initial.revision).toBe(1);
    await attached.detach();
  });

  test("detach closes the channel and unregisters its callback", async () => {
    const bridge = installBridge();
    const { attachDesktopThread } = await import("./desktop");
    const receive = vi.fn();
    const attached = await attachDesktopThread("thread-fixture", receive);
    const emit = bridge.callbacks.get(1)!;
    const latest = view("thread-fixture", "session-latest", 2);

    emit({ index: 2, message: latest });

    await attached.detach();
    emit({ index: 3, message: view("thread-fixture", "session-after-detach", 3) });

    expect(bridge.unregisterCallback).toHaveBeenCalledExactlyOnceWith(1);
    expect(bridge.callbacks.size).toBe(0);
    expect(receive).toHaveBeenCalledExactlyOnceWith(latest);
    expect(bridge.invoke).toHaveBeenLastCalledWith("codex.desktop.detach", {
      request: { revision: 0, sessionId: "session-latest" }
    });
  });

  test.each([
    ["default owner confirmation", undefined, true],
    ["passive subscription", false, false]
  ])("forwards %s to the native attach request", async (_label, confirmOwner, expected) => {
    const bridge = installBridge();
    const { attachDesktopThread } = await import("./desktop");
    const attached = await attachDesktopThread("thread-fixture", vi.fn(), confirmOwner);
    const call = bridge.invoke.mock.calls.find(([command]) => command === "codex.desktop.attach");

    expect(call).toBeDefined();
    expect((call?.[1] as { request: Record<string, unknown> }).request).toEqual({
      revision: 0,
      threadId: "thread-fixture",
      confirmOwner: expected
    });
    await attached.detach();
  });

  test("drops frames for another thread and for a previous machine revision", async () => {
    const bridge = installBridge();
    const { attachDesktopThread } = await import("./desktop");
    const runtime = await import("../runtime");
    const receive = vi.fn();
    const attached = await attachDesktopThread("thread-fixture", receive);
    const emit = bridge.callbacks.get(1)!;

    emit({ index: 1, message: view("another-thread") });
    expect(receive).not.toHaveBeenCalled();

    await runtime.remoteSelect("remote");
    emit({ index: 2, message: view("thread-fixture") });

    expect(receive).not.toHaveBeenCalled();
    await attached.detach();
  });

  test("unregisters the channel callback when attach fails", async () => {
    const bridge = installBridge(command => {
      if (command === "codex.desktop.attach") throw new Error("fixture attach failure");
      if (command === "codex.desktop.detach") return { ok: true };
      return {};
    });
    const { attachDesktopThread } = await import("./desktop");

    await expect(attachDesktopThread("thread-fixture", vi.fn())).rejects.toThrow("fixture attach failure");

    expect(bridge.transformCallback).toHaveBeenCalledOnce();
    expect(bridge.unregisterCallback).toHaveBeenCalledExactlyOnceWith(1);
    expect(bridge.callbacks.size).toBe(0);
  });

  test("refuses every desktop API on a remote machine before remote.invoke", async () => {
    const bridge = installBridge();
    const runtime = await import("../runtime");
    const desktop = await import("./desktop");
    await runtime.remoteSelect("remote");

    const calls = [
      () => desktop.attachDesktopThread("thread-fixture", vi.fn()),
      () => desktop.actOnDesktopThread("session-fixture", { type: "resumeQueue" }),
      () => desktop.openDesktopThread("thread-fixture"),
      () => desktop.desktopCatalog({ sessionId: null, projectHandle: null, kind: "models", query: "", cursor: null }),
      () => desktop.chooseDesktopItem("project"),
      () => desktop.createDesktopThread("project-fixture")
    ];

    for (const call of calls) await expect(Promise.resolve().then(call)).rejects.toThrow("桌面协作仅支持本机 Codex");

    expect(bridge.invoke.mock.calls.map(([command]) => command)).toEqual(["remote.select"]);
    expect(bridge.invoke.mock.calls.map(([command]) => command)).not.toContain("remote.invoke");
    expect(bridge.transformCallback).not.toHaveBeenCalled();
  });

  test("preserves the action and message identity in repeated transport attempts", async () => {
    const bridge = installBridge();
    const { actOnDesktopThread } = await import("./desktop");
    const action = { type: "send" as const, messageId: "message-fixture", text: "fixture text", references: ["attachment-fixture"] };

    await actOnDesktopThread("session-fixture", action);
    await actOnDesktopThread("session-fixture", action);

    const requests = bridge.invoke.mock.calls
      .filter(([command]) => command === "codex.desktop.action")
      .map(([, args]) => (args as { request: { sessionId: string; action: typeof action } }).request);
    expect(requests).toHaveLength(2);
    expect(requests.map(request => request.action.messageId)).toEqual(["message-fixture", "message-fixture"]);
    expect(requests[0].action).toBe(action);
    expect(requests[1].action).toBe(action);
    expect(requests.map(request => request.sessionId)).toEqual(["session-fixture", "session-fixture"]);
  });

  test("requests a native diagnostics copy with only the fixed copy flag", async () => {
    const bridge = installBridge();
    const { desktopDiagnostics } = await import("./desktop");

    await desktopDiagnostics(true);

    const call = bridge.invoke.mock.calls.find(([command]) => command === "codex.desktop.diagnostics");
    expect(call).toBeDefined();
    expect(call).toEqual(["codex.desktop.diagnostics", { request: { revision: 0, copy: true } }]);
    const request = (call?.[1] as { request: Record<string, unknown> }).request;
    expect(Object.keys(request).sort()).toEqual(["copy", "revision"]);
    expect(request).not.toHaveProperty("text");
    expect(request).not.toHaveProperty("clipboardText");
  });

  test("sends only bounds and opaque handles through a dedicated attachment channel", async () => {
    const bridge = installBridge();
    const { bindDesktopAttachments } = await import("./desktop");
    const bounds = { x: 24, y: 36, width: 420, height: 180 };
    const handles = ["file-handle-fixture"];
    const binding = await bindDesktopAttachments("session-fixture", bounds, handles, vi.fn());

    const firstCall = bridge.invoke.mock.calls.find(([command]) => command === "codex.desktop.attachments");
    expect(firstCall).toBeTruthy();
    const [command, rawArgs] = firstCall!;
    expect(command).toBe("codex.desktop.attachments");
    const args = rawArgs as { request: Record<string, any>; onEvent: string };
    expect(args.request).toEqual({
      revision: 0,
      sessionId: "session-fixture",
      action: { type: "bind", bounds, handles }
    });
    expect(Object.keys(args.request).sort()).toEqual(["action", "revision", "sessionId"]);
    expect(Object.keys(args.request.action).sort()).toEqual(["bounds", "handles", "type"]);
    expect(args.onEvent).toBe("__CHANNEL__:1");
    expect(bridge.transformCallback).toHaveBeenCalledOnce();
    expect(JSON.stringify(args.request)).not.toMatch(/path|bytes/i);

    const nextBounds = { x: 30, y: 42, width: 400, height: 160 };
    await binding.update(nextBounds, handles);
    await binding.paste();
    await binding.release(handles);
    await binding.close();

    const requests = bridge.invoke.mock.calls
      .filter(([name]) => name === "codex.desktop.attachments")
      .map(([, callArgs]) => (callArgs as { request: { revision: number; sessionId: string; action: unknown } }).request);
    expect(requests).toEqual([
      { revision: 0, sessionId: "session-fixture", action: { type: "bind", bounds, handles } },
      { revision: 0, sessionId: "session-fixture", action: { type: "bounds", bindingId: "binding-fixture", bounds: nextBounds, handles } },
      { revision: 0, sessionId: "session-fixture", action: { type: "paste", bindingId: "binding-fixture" } },
      { revision: 0, sessionId: "session-fixture", action: { type: "release", bindingId: "binding-fixture", handles } },
      { revision: 0, sessionId: "session-fixture", action: { type: "unbind", bindingId: "binding-fixture" } }
    ]);
    expect(bridge.unregisterCallback).toHaveBeenCalledExactlyOnceWith(1);
  });

  test("keeps screenshot events alive after repeated bounds refreshes", async () => {
    const bridge = installBridge();
    const { bindDesktopAttachments } = await import("./desktop");
    const receive = vi.fn();
    const binding = await bindDesktopAttachments(
      "session-fixture",
      { x: 24, y: 36, width: 420, height: 180 },
      [],
      receive
    );
    const emit = bridge.callbacks.get(1)!;
    const screenshot = {
      bindingId: "binding-fixture",
      items: [{ handle: "screenshot-handle-fixture", name: "screenshot.png", kind: "image" }],
      error: null
    };

    await binding.update({ x: 30, y: 42, width: 400, height: 160 }, []);
    await binding.update({ x: 36, y: 48, width: 380, height: 140 }, []);
    await binding.paste();
    emit({ index: 1, message: screenshot });

    const attachmentCalls = bridge.invoke.mock.calls.filter(([name]) => name === "codex.desktop.attachments");
    expect(attachmentCalls).toHaveLength(4);
    expect(attachmentCalls.map(([, args]) => (args as { onEvent: string }).onEvent)).toEqual([
      "__CHANNEL__:1",
      "__CHANNEL__:1",
      "__CHANNEL__:1",
      "__CHANNEL__:1"
    ]);
    expect(bridge.callbacks.has(1)).toBe(true);
    expect(receive).toHaveBeenCalledExactlyOnceWith(screenshot);

    await binding.close();
  });

  test("records the legacy per-call Channel drop that closes the attachment stream", async () => {
    const bridge = installBridge();
    const { bindDesktopAttachments } = await import("./desktop");
    const receive = vi.fn();
    const binding = await bindDesktopAttachments(
      "session-fixture",
      { x: 24, y: 36, width: 420, height: 180 },
      [],
      receive
    );
    const emit = bridge.callbacks.get(1)!;
    const screenshot = {
      bindingId: "binding-fixture",
      items: [{ handle: "legacy-screenshot-handle", name: "legacy.png", kind: "image" }],
      error: null
    };

    // A temporary Channel deserialized for bounds would send this frame from
    // Drop, unregistering the persistent frontend callback before paste emits.
    await binding.update({ x: 30, y: 42, width: 400, height: 160 }, []);
    emit({ index: 0, end: true });
    emit({ index: 1, message: screenshot });

    expect(bridge.unregisterCallback).toHaveBeenCalledExactlyOnceWith(1);
    expect(bridge.callbacks.has(1)).toBe(false);
    expect(receive).not.toHaveBeenCalled();

    await binding.close();
  });

  test("drops events from a closed binding when the composer switches sessions", async () => {
    const bridge = installBridge();
    const { bindDesktopAttachments } = await import("./desktop");
    const oldReceive = vi.fn();
    const oldBinding = await bindDesktopAttachments(
      "old-session-fixture",
      { x: 0, y: 0, width: 200, height: 100 },
      [],
      oldReceive
    );
    const oldEmit = bridge.callbacks.get(1)!;
    await oldBinding.close();

    const currentReceive = vi.fn();
    const currentBinding = await bindDesktopAttachments(
      "new-session-fixture",
      { x: 0, y: 0, width: 200, height: 100 },
      [],
      currentReceive
    );
    const currentEmit = bridge.callbacks.get(2)!;
    const lateScreenshot = {
      bindingId: "binding-fixture",
      items: [{ handle: "late-screenshot-handle", name: "late.png", kind: "image" }],
      error: null
    };

    oldEmit({ index: 1, message: lateScreenshot });
    expect(oldReceive).not.toHaveBeenCalled();
    expect(currentReceive).not.toHaveBeenCalled();

    currentEmit({
      index: 1,
      message: {
        bindingId: "binding-fixture",
        items: [{ handle: "current-screenshot-handle", name: "current.png", kind: "image" }],
        error: null
      }
    });
    expect(currentReceive).toHaveBeenCalledExactlyOnceWith({
      bindingId: "binding-fixture",
      items: [{ handle: "current-screenshot-handle", name: "current.png", kind: "image" }],
      error: null
    });

    await currentBinding.close();
  });

  test("ignores other bindings, stale machine revisions, and events after close", async () => {
    const bridge = installBridge();
    const { bindDesktopAttachments } = await import("./desktop");
    const runtime = await import("../runtime");
    const receive = vi.fn();
    const binding = await bindDesktopAttachments("session-fixture", { x: 0, y: 0, width: 200, height: 100 }, [], receive);
    const emit = bridge.callbacks.get(1)!;
    const item = { handle: "file-handle-fixture", name: "image.png", kind: "file" };

    emit({ index: 1, message: { bindingId: "another-binding", items: [item], error: null } });
    emit({ index: 2, message: { bindingId: "binding-fixture", items: [item], error: null } });
    expect(receive).toHaveBeenCalledExactlyOnceWith({ bindingId: "binding-fixture", items: [item], error: null });

    await runtime.remoteSelect("remote");
    emit({ index: 3, message: { bindingId: "binding-fixture", items: [item], error: null } });
    expect(receive).toHaveBeenCalledOnce();

    await binding.close();
    emit({ index: 4, message: { bindingId: "binding-fixture", items: [item], error: null } });
    expect(receive).toHaveBeenCalledOnce();
    expect(bridge.unregisterCallback).toHaveBeenCalledExactlyOnceWith(1);
  });
});

test("desktop collaboration requires an explicit local runtime capability", () => {
  const baseline = {
    threads: true, jobs: true, probe: true, settings: true, job_history: true, app_updater: true,
    systemd: false, linux_update_job: false, prune_backups: false
  };
  const missing = runtimeCapabilitiesFromResponse({
    api_version: 2, host_surface: "desktop_embedded_tauri", capabilities: { ...baseline }
  });
  const remoteClaim = runtimeCapabilitiesFromResponse({
    api_version: 2, host_surface: "linux_server_api", capabilities: { ...baseline, codex_desktop: true }
  });
  const localClaim = runtimeCapabilitiesFromResponse({
    api_version: 2, host_surface: "desktop_embedded_tauri", capabilities: { ...baseline, codex_desktop: true }
  });

  expect(missing.codexDesktop).toBe(false);
  expect(remoteClaim.codexDesktop).toBe(false);
  expect(localClaim.codexDesktop).toBe(true);
});
