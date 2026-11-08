import type { PlatformOverview, SystemCapabilities, SystemCapabilitiesResponse } from "../../types";

export type DemoFixtureKey = "linux-api" | "macos-tauri";
export type DemoFixture = {
  platform: PlatformOverview;
  system: SystemCapabilitiesResponse;
};

const macApplicationSupport = "~/Library/Application Support/AgentHub";
const linuxDemoRoot = "/srv/agenthub-demo";

type CapabilityFieldValues = Record<DemoFixtureKey, boolean>;

const capabilityFields = {
  threads: { "linux-api": true, "macos-tauri": true },
  thread_subagents: { "linux-api": true, "macos-tauri": true },
  codex_desktop: { "linux-api": false, "macos-tauri": true },
  jobs: { "linux-api": true, "macos-tauri": true },
  github_ci: { "linux-api": true, "macos-tauri": true },
  probe: { "linux-api": true, "macos-tauri": true },
  settings: { "linux-api": true, "macos-tauri": true },
  job_history: { "linux-api": true, "macos-tauri": true },
  app_updater: { "linux-api": true, "macos-tauri": true },
  systemd: { "linux-api": true, "macos-tauri": false },
  linux_update_job: { "linux-api": true, "macos-tauri": false },
  prune_backups: { "linux-api": true, "macos-tauri": false },
  thread_cleanup: { "linux-api": true, "macos-tauri": true },
  antigravity_notifications: { "linux-api": false, "macos-tauri": true },
  dsh_notifications: { "linux-api": false, "macos-tauri": true },
  antigravity_sessions: { "linux-api": true, "macos-tauri": true },
  grokbot_sessions: { "linux-api": false, "macos-tauri": true },
  grokbot_notifications: { "linux-api": false, "macos-tauri": true },
  dsh_sessions: { "linux-api": true, "macos-tauri": true },
  hidden_cleanup_scan: { "linux-api": true, "macos-tauri": true },
  thread_archive_actions: { "linux-api": true, "macos-tauri": true },
} satisfies Record<keyof SystemCapabilities, CapabilityFieldValues>;

const macosEnumerableCapabilityKeys: readonly (keyof SystemCapabilities)[] = [
  "threads",
  "thread_subagents",
  "codex_desktop",
  "jobs",
  "probe",
  "settings",
  "job_history",
  "app_updater",
  "thread_cleanup",
  "thread_archive_actions",
  "antigravity_sessions",
  "dsh_sessions",
] satisfies Array<keyof SystemCapabilities>;

function buildDemoCapabilities(fixture: DemoFixtureKey): SystemCapabilities {
  if (fixture !== "macos-tauri") {
    return Object.fromEntries(
      Object.entries(capabilityFields).map(([field, values]) => [field, values[fixture]])
    ) as SystemCapabilities;
  }
  const capabilities: Partial<SystemCapabilities> = {};
  for (const field of Object.keys(capabilityFields) as Array<keyof SystemCapabilities>) {
    Object.defineProperty(capabilities, field, {
      value: capabilityFields[field][fixture],
      enumerable: macosEnumerableCapabilityKeys.includes(field),
      configurable: true
    });
  }
  return capabilities as SystemCapabilities;
}

export function buildDemoFixture(fixture: DemoFixtureKey): DemoFixture {
  if (fixture !== "linux-api" && fixture !== "macos-tauri") throw new Error("Unsupported host surface");
  const desktop = fixture === "macos-tauri";
  const root = desktop ? macApplicationSupport : linuxDemoRoot;
  return {
    platform: {
      kind: desktop ? "macos" : "linux",
      data_dir: root,
      config_file: `${root}/config.toml`,
      log_dir: desktop ? "~/Library/Logs/AgentHub" : `${root}/logs`,
      service_name: desktop ? "AgentHub.app" : "agenthub-webd",
      service_kind: desktop ? "tauri" : "systemd"
    },
    system: {
      api_version: 2,
      host_surface: desktop ? "desktop_embedded_tauri" : "linux_server_api",
      capabilities: buildDemoCapabilities(fixture)
    },

  };
}

export function buildDemoPlatformOverview(fixture: DemoFixtureKey): PlatformOverview {
  return buildDemoFixture(fixture).platform;
}

export function buildDemoSystemCapabilities(fixture: DemoFixtureKey): SystemCapabilitiesResponse {
  return buildDemoFixture(fixture).system;
}
