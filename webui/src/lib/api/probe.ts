import type {
  OptionalResult,
  ProbeEventsResponse,
  ProbeJobAction,
  ProbeSettings,
  ProbeStatus
} from "../../types";
import { callCommand } from "./transport";
import { currentDemoFixtureKey, jobIdFromRuntimeResult, normalizeOptionalResult, normalizeProbeRuntimePayload, USE_DEMO } from "./shared";

export async function getProbeStatus(): Promise<OptionalResult<ProbeStatus>> {
  if (USE_DEMO) {
    return {
      available: true,
      data: (await import("./demo")).demoProbeStatus(currentDemoFixtureKey())
    };
  }
  return normalizeOptionalResult<ProbeStatus>(await callCommand<ProbeStatus | OptionalResult<ProbeStatus>>("probe.status"));
}

export async function getProbeSettings(): Promise<OptionalResult<ProbeSettings>> {
  if (USE_DEMO) {
    return {
      available: true,
      data: (await import("./demo")).demoProbeSettings(currentDemoFixtureKey())
    };
  }
  const payload = await callCommand<ProbeSettings | OptionalResult<ProbeSettings>>("probe.settings.get");
  const result = normalizeOptionalResult<ProbeSettings>(payload);
  return result.available
    ? { ...result, data: normalizeProbeRuntimePayload(result.data) as ProbeSettings }
    : result;
}

function normalizeProbeSettingsSavePayload(settings: Partial<ProbeSettings>): Partial<ProbeSettings> {
  const nestedDeviceKey = settings.probe?.notifications?.device_key;
  if (typeof nestedDeviceKey !== "string" || !nestedDeviceKey.trim()) {
    return settings;
  }
  return {
    ...settings,
    notifications: {
      ...settings.notifications,
      device_key: nestedDeviceKey.trim()
    }
  };
}

export async function saveProbeSettings(settings: Partial<ProbeSettings>): Promise<ProbeSettings> {
  if (USE_DEMO) return (await import("./demo")).demoSavedProbeSettings(settings, currentDemoFixtureKey());
  const normalizedSettings = normalizeProbeSettingsSavePayload(settings);
  const payload = await callCommand<ProbeSettings>("probe.settings.save", {
    settings: normalizedSettings
  });
  return normalizeProbeRuntimePayload(payload) as ProbeSettings;
}


export async function getProbeEvents(limit = 10): Promise<OptionalResult<ProbeEventsResponse>> {
  if (USE_DEMO) {
    return (await import("./demo")).demoProbeEvents(limit);
  }
  return normalizeOptionalResult<ProbeEventsResponse>(await callCommand<ProbeEventsResponse | OptionalResult<ProbeEventsResponse>>("probe.events", { limit }));
}

export async function runProbeBarkTest(): Promise<{ job_id: string }> {
  return startProbeCommand("probe.barkTest", "probe-bark-test");
}

export async function runProbeHooksInstall(): Promise<{ job_id: string }> {
  return startProbeCommand("probe.installHooks", "probe-hooks-install");
}

export async function runProbeHooksRemove(): Promise<{ job_id: string }> {
  return startProbeCommand("probe.removeHooks", "probe-hooks-remove");
}

async function startProbeCommand(
  command: "probe.barkTest" | "probe.installHooks" | "probe.removeHooks",
  fallback: string,
  ): Promise<{ job_id: string }> {
  if (USE_DEMO) return (await import("./demo")).demoJobId(fallback);
  const result = await callCommand<{ job_id?: string | null; jobId?: string | null }>(command, {});
  return jobIdFromRuntimeResult(result, fallback);
}

export type HookProviderId = "claude" | "grok" | "antigravity";

export async function manageClaudeHooks(
  action: import("../../types").ClaudeHooksAction,
  confirmed = false,
  provider: HookProviderId = "claude"
): Promise<import("../../types").ClaudeHooksStatus> {
  if (USE_DEMO) return { installed: false, managedCount: 0, changed: false };
  return callCommand(
    "claude.hooks",
    provider === "claude"
      ? { request: { action, confirmed } }
      : { request: { action, confirmed }, provider }
  );
}

export async function getGithubCiStatus(refresh = false): Promise<import("../../types").GithubCiStatus> {
  if (USE_DEMO) return (await import("./demo")).demoGithubCiStatus();
  return callCommand("github.ci.status", { refresh });
}
