import type { HostSurface, SystemCapabilitiesResponse } from "../../types";
export type RuntimeContext = { kind: "desktop" };
export type RuntimeCapabilityMatrix = {
  runtimeKind: "desktop"; hostSurface: HostSurface; codexStatePaths: boolean;
  threadSubagents?: boolean;
  hiddenCleanupScan?: boolean;
  dshArchiveCleanup?: boolean;
  antigravityNotifications?: boolean;
  dshNotifications?: boolean;
  antigravitySessions?: boolean;
  dshSessions?: boolean;
  grokbotSessions?: boolean;
  githubCi?: boolean;
  grokbotNotifications?: boolean;
  codexDesktop?: boolean;
  updatePrune: boolean; threadCleanup: boolean; threadArchiveActions: boolean; updateServiceLabels: boolean;
};
export const desktopBootstrapCapabilities: RuntimeCapabilityMatrix = {
  runtimeKind: "desktop", hostSurface: "desktop_embedded_tauri", codexStatePaths: false,
  updatePrune: false, threadCleanup: false, threadArchiveActions: false, updateServiceLabels: false,
};
export function runtimeCapabilities(_context?: RuntimeContext): RuntimeCapabilityMatrix { return desktopBootstrapCapabilities; }
export function runtimeCapabilitiesForRuntime(_desktop: boolean | "desktop" = true): RuntimeCapabilityMatrix { return desktopBootstrapCapabilities; }
export function runtimeCapabilitiesFromResponse(status?: Partial<SystemCapabilitiesResponse> | null, fallback = desktopBootstrapCapabilities): RuntimeCapabilityMatrix {
  const core = status?.capabilities;
  if (!core) return fallback;
  const remote = status.host_surface === "linux_server_api";
  return { runtimeKind: "desktop", hostSurface: status.host_surface ?? fallback.hostSurface,
    codexStatePaths: remote && core.systemd, updatePrune: remote && core.prune_backups,
    // Subagent collection arrived in 1.2.11; absent capability still means unsupported.
    threadSubagents: core.thread_subagents === true,
    hiddenCleanupScan: core.hidden_cleanup_scan === true,
    dshArchiveCleanup: core.dsh_archive_cleanup === true,
    antigravityNotifications: !remote && core.antigravity_notifications === true,
    dshNotifications: !remote && core.dsh_notifications === true,
    antigravitySessions: core.antigravity_sessions === true,
    dshSessions: core.dsh_sessions === true,
    githubCi: core.github_ci === true,
    grokbotSessions: !remote && core.grokbot_sessions === true,
    grokbotNotifications: !remote && core.grokbot_notifications === true,
    codexDesktop: !remote && core.codex_desktop === true,
    threadCleanup: core.thread_cleanup === true, threadArchiveActions: core.thread_archive_actions === true,
    updateServiceLabels: remote && core.linux_update_job };
}
