import { describe, expect, test } from "vitest";
import { buildDemoFixture, buildDemoPlatformOverview, buildDemoSystemCapabilities, type DemoFixtureKey } from "./demoCore";

const fixtureKeys: DemoFixtureKey[] = ["linux-api", "macos-tauri"];
const sharedKeys = ["antigravity_sessions", "app_updater", "codex_desktop", "dsh_sessions", "job_history", "jobs", "probe", "settings", "thread_archive_actions", "thread_cleanup", "thread_subagents", "threads"];
const retiredKeys = ["admin_password", "csrf", "nginx", "public_endpoint", "security_settings", "turnstile", "web_auth"];

describe("machine demo fixtures", () => {
  test("both hosts use API version 1 and omit retired browser authentication fields", () => {
    for (const fixture of fixtureKeys) {
      const view = buildDemoFixture(fixture);
      expect(view.system.api_version).toBe(2);
      expect(view).not.toHaveProperty("security");
      expect(view.platform).not.toHaveProperty("webui_dir");
      for (const field of retiredKeys) expect(view.system.capabilities).not.toHaveProperty(field);
      expect(JSON.stringify(view)).not.toMatch(/192\.0\.2\.10|panel\.example\.com|\/opt\/agenthub/i);
    }
  });

  test("local desktop exposes shared capabilities and its native updater", () => {
    const view = buildDemoFixture("macos-tauri");
    expect(view.system.host_surface).toBe("desktop_embedded_tauri");
    expect(Object.keys(view.system.capabilities).sort()).toEqual(sharedKeys);
    expect(view.system.capabilities).toMatchObject({ app_updater: true, codex_desktop: true, thread_cleanup: true, thread_archive_actions: true, systemd: false, linux_update_job: false, prune_backups: false });
    expect(view.platform).toMatchObject({ kind: "macos", service_kind: "tauri", service_name: "AgentHub.app" });
    expect(JSON.stringify(view)).not.toMatch(/systemd|Nginx|Turnstile|管理员密码|公网入口|Linux prune|Linux update/i);
  });

  test("remote API fixture retains Linux service, update and backup capabilities", () => {
    const view = buildDemoSystemCapabilities("linux-api");
    expect(view.host_surface).toBe("linux_server_api");
    expect(Object.keys(view.capabilities).sort()).toEqual([...sharedKeys, "systemd", "linux_update_job", "prune_backups", "dsh_archive_cleanup", "antigravity_notifications", "dsh_notifications", "grokbot_sessions", "grokbot_notifications", "hidden_cleanup_scan", "codex_delete_tree"].sort());
    expect(view.capabilities).toMatchObject({ codex_desktop: false, systemd: true, linux_update_job: true, prune_backups: true, hidden_cleanup_scan: false });
    expect(buildDemoPlatformOverview("linux-api")).toMatchObject({ kind: "linux", data_dir: "/srv/agenthub-demo", config_file: "/srv/agenthub-demo/config.toml", log_dir: "/srv/agenthub-demo/logs", service_kind: "systemd", service_name: "agenthub-webd" });
  });

  test("retired standalone and LAN browser fixtures are unavailable", () => {
    for (const key of ["linux-web", "desktop-lan-webui"]) expect(() => buildDemoFixture(key as DemoFixtureKey)).toThrow("Unsupported host surface");
  });
});

test("new provider notifications are local-only and absent on old servers", async () => {
  const { runtimeCapabilitiesFromResponse } = await import("./capabilities");
  const local = runtimeCapabilitiesFromResponse(buildDemoSystemCapabilities("macos-tauri"));
  const remote = runtimeCapabilitiesFromResponse(buildDemoSystemCapabilities("linux-api"));
  expect(local.grokbotSessions).toBe(true);
  expect(local.grokbotNotifications).toBe(true);
  expect(remote.grokbotSessions).toBe(false);
  expect(remote.grokbotNotifications).toBe(false);
  expect(local.antigravityNotifications).toBe(true);
  expect(local.dshNotifications).toBe(true);
  expect(local.antigravitySessions).toBe(true);
  expect(local.dshSessions).toBe(true);
  expect(remote.antigravityNotifications).toBe(false);
  expect(remote.dshNotifications).toBe(false);
  expect(remote.antigravitySessions).toBe(true);
  expect(remote.dshSessions).toBe(true);
  const old = buildDemoSystemCapabilities("linux-api");
  delete old.capabilities.antigravity_notifications;
  delete old.capabilities.dsh_notifications;
  delete old.capabilities.antigravity_sessions;
  delete old.capabilities.dsh_sessions;
  expect(runtimeCapabilitiesFromResponse(old).antigravityNotifications).toBe(false);
  expect(runtimeCapabilitiesFromResponse(old).dshNotifications).toBe(false);
  expect(runtimeCapabilitiesFromResponse(old).antigravitySessions).toBe(false);
  expect(runtimeCapabilitiesFromResponse(old).dshSessions).toBe(false);
});
