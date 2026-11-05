import { afterEach, expect, test, vi } from "vitest";
import { planFilename } from "./planExport";

afterEach(() => { vi.unstubAllGlobals(); vi.restoreAllMocks(); vi.resetModules(); });

test("plan download names preserve the visible title and cannot escape the download directory", () => {
  expect(planFilename("# 修复 Plan: API/CLI?\n\nBody", "Fallback")).toBe("修复 Plan API CLI.md");
  expect(planFilename("## [Guide](https://example.com) **v2**\nBody", "Fallback")).toBe("Guide v2.md");
  expect(planFilename("Body without a heading", "Fallback")).toBe("Body without a heading.md");
  expect(planFilename("", "Fallback/Title")).toBe("Fallback Title.md");
  expect(planFilename("# CON", "Fallback")).toBe("_CON.md");
  expect(planFilename("# Plan.md", "Fallback")).toBe("Plan.md");
  expect(new TextEncoder().encode(planFilename(`# ${"计划".repeat(120)}`, "Fallback")).length).toBeLessThanOrEqual(203);
});

test.each(["local", "remote"] as const)("saving a %s Plan sends cleaned Markdown directly to the native machine", async target => {
  vi.resetModules();
  const saved = { filename: "Visible Plan (2).md" };
  const invoke = vi.fn((command: string) => command === "remote.get"
    ? { target, revision: 7, sshHost: "fixture-host", configured: true }
    : saved);
  vi.stubGlobal("__AGENTHUB_TEST_INVOKE__", invoke);
  const runtime = await import("../runtime");
  await runtime.initializeConnection();
  const { downloadPlanMarkdown } = await import("./planExport");
  invoke.mockClear();
  const markdown = "# Visible: Plan?\n\n- **Keep** formatting\n\n```xml\n<rollout_ids>literal example</rollout_ids>\n```";
  const source = `<citation_entries># Hidden title</citation_entries>\n${markdown}\n<oai-mem-citation>internal-id</oai-mem-citation>`;
  await expect(downloadPlanMarkdown(source, planFilename(source, "Fallback"))).resolves.toEqual(saved);
  expect(invoke).toHaveBeenCalledExactlyOnceWith("plans.save", {
    request: { filename: "Visible Plan.md", markdown }
  });
  expect(runtime.connectionSnapshot().target).toBe(target);
});

test("native save failure rejects so the Plan can show failure and allow retry", async () => {
  const invoke = vi.fn().mockRejectedValue(new Error("Downloads is not writable"));
  vi.stubGlobal("__AGENTHUB_TEST_INVOKE__", invoke);
  const { downloadPlanMarkdown } = await import("./planExport");
  await expect(downloadPlanMarkdown("# Plan", "Plan.md")).rejects.toThrow("Downloads is not writable");
  expect(invoke).toHaveBeenCalledTimes(1);
});
