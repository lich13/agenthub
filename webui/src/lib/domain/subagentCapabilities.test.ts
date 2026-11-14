import { describe, expect, test } from "vitest";
import type { SystemCapabilities, SystemCapabilitiesResponse } from "../../types";
import { desktopBootstrapCapabilities, runtimeCapabilitiesFromResponse } from "./capabilities";

const core: SystemCapabilities = {
  threads: true, jobs: true, probe: true, settings: true, job_history: true,
  app_updater: false, systemd: false, linux_update_job: false, prune_backups: false
};

describe("subagent capability negotiation", () => {
  test.each(["desktop_embedded_tauri", "linux_server_api"] as const)("%s requires an explicit supported capability", hostSurface => {
    const response: SystemCapabilitiesResponse = { api_version: 2, host_surface: hostSurface, capabilities: core };
    expect(runtimeCapabilitiesFromResponse(response).threadSubagents).toBe(false);
    expect(runtimeCapabilitiesFromResponse({ ...response, capabilities: { ...core, thread_subagents: false } }).threadSubagents).toBe(false);
    expect(runtimeCapabilitiesFromResponse({ ...response, capabilities: { ...core, thread_subagents: true } }).threadSubagents).toBe(true);
  });

  test("bootstrap and a missing response never enable child reads", () => {
    expect(desktopBootstrapCapabilities.threadSubagents).not.toBe(true);
    expect(runtimeCapabilitiesFromResponse(undefined).threadSubagents).not.toBe(true);
    expect(runtimeCapabilitiesFromResponse(null).threadSubagents).not.toBe(true);
  });
});


test("Claude child browsing requires its own advertised capability", () => {
  expect(desktopBootstrapCapabilities.claudeSubagents).not.toBe(true);
  const capabilities = { ...core, thread_subagents: true };
  delete capabilities.claude_subagents;
  expect(runtimeCapabilitiesFromResponse({ capabilities }).claudeSubagents).toBe(false);
  expect(runtimeCapabilitiesFromResponse({ capabilities: { ...capabilities, claude_subagents: true } }).claudeSubagents).toBe(true);
});
