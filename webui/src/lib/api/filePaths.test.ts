import { afterEach, describe, expect, test, vi } from "vitest";

describe("file path runtime adapter", () => {
  afterEach(() => {
    delete globalThis.__AGENTHUB_DESKTOP_RUNTIME__;
    delete globalThis.__AGENTHUB_TEST_INVOKE__;
    vi.resetModules();
  });

  test("desktop reveals a validated path through the existing opener plugin", async () => {
    globalThis.__AGENTHUB_DESKTOP_RUNTIME__ = true;
    const invoke = vi.fn(async (command: string, args: unknown) => ({ command, args }));
    globalThis.__AGENTHUB_TEST_INVOKE__ = invoke;
    const { revealLocalPath } = await import("./filePaths");
    await revealLocalPath("/workspace/中文 文件.md");
    expect(invoke).toHaveBeenCalledWith("plugin:opener|reveal_item_in_dir", { paths: ["/workspace/中文 文件.md"] });
  });

  test("a browser without the native bridge keeps the path action copy-only", async () => {
    const { canRevealLocalPath, revealLocalPath } = await import("./filePaths");
    expect(canRevealLocalPath()).toBe(false);
    await expect(revealLocalPath("/workspace/file.md")).rejects.toThrow("无法在 Finder 中显示");
  });

  test("remote paths never invoke the local opener even when a native bridge exists", async () => {
    const invoke = vi.fn(async () => ({ target: "remote", revision: 1, sshHost: "fixture-host", configured: true }));
    globalThis.__AGENTHUB_TEST_INVOKE__ = invoke;
    const { remoteSelect } = await import("../runtime");
    await remoteSelect("remote");
    const { canRevealLocalPath, revealLocalPath } = await import("./filePaths");
    expect(canRevealLocalPath()).toBe(false);
    await expect(revealLocalPath("/srv/project/file.md")).rejects.toThrow("无法在 Finder 中显示");
    expect(invoke).toHaveBeenCalledTimes(1);
  });

});
