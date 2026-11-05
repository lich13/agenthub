import { OPS_PANEL_TITLES, opsUpdateActionView } from "./runtimeViewModel";
import { capabilitiesForInput } from "./runtimeViewModel";
import type { RuntimeCapabilityMatrix } from "./capabilities";

export const sharedVisualTokens = {
  shellClass: "app-shell",
  navClass: "side-nav",
  panelClass: "panel",
  primaryButtonClass: "primary-button",
  secondaryButtonClass: "secondary-button",
  dangerButtonClass: "danger-button",
  disabledAttribute: "disabled"
} as const;

export const sharedNavigationLabels = [
  "Codex",
  "Claude Code",
  "Grok Build",
  "Antigravity",
  "DeepSeek Harness",
  "Probe",
  "设置"
] as const;

export const sharedCorePanelTitles = [
  "更新与维护",
  "执行记录",
  "全平台线程清理"
] as const;

export const sharedActionLabels = {
  dryRun: "Dry-run",
  archiveCleanup: "清理归档",
  archiveConfirm: "确认清理归档",
  hiddenDryRun: "扫描隐藏线程",
  hiddenCleanup: "清理隐藏线程",
  hiddenConfirm: "确认清理隐藏",
  unifiedDryRun: "全平台扫描预览",
  unifiedCleanup: "一键清理所选平台",
  unifiedConfirm: "确认清理所选",
  copyThreadId: "复制线程 ID"
};

export const sharedDisabledStates = {
  renameArchivedThread: "取消归档后可改名",
  updateInstallWithoutAvailableVersion: "更新按钮仅在已确认有新版本时显示",
  cleanupArmBeforeDryRun: "cleanup arm disabled until dry-run returns candidates",
  cleanupConfirmBeforeArmed: "cleanup execute disabled until confirmation is armed",
  cleanupDuringMutation: "cleanup controls disabled while dry-run or execute is pending"
};

export const remoteApiOnlyVisualSurfaces = [
  "服务更新",
  "清理更新备份"
] as const;

export const macosForbiddenVisualSurfaces = [
  "登录",
  "Turnstile",
  "修改密码"
] as const;

export const desktopTauriOnlyVisualSurfaces = [
  "远程连接",
  "本机",
  "远程机器",
  "Grok Bot"
] as const;

export type VisualContract = {
  sharedNavigation: string[];
  sharedPanels: string[];
  sharedActions: string[];
  disabledStates: string[];
  remoteApiOnly: string[];
  desktopTauriOnly: string[];
  forbidden: string[];
  cleanupRequiresDryRun: boolean;
  updateActions: string[];
};

export function visualContractForRuntime(input?: RuntimeCapabilityMatrix): VisualContract {
  const capabilities = capabilitiesForInput(input);
  return {
    sharedNavigation: [...sharedNavigationLabels],
    sharedPanels: sharedCorePanelTitles.filter((title) => (
      capabilities.threadCleanup || title !== OPS_PANEL_TITLES.unifiedCleanup
    )),
    sharedActions: [
      ...(capabilities.threadCleanup ? [
        sharedActionLabels.unifiedDryRun,
        sharedActionLabels.unifiedCleanup,
        sharedActionLabels.unifiedConfirm
      ] : [])
    ],
    disabledStates: [
      sharedDisabledStates.updateInstallWithoutAvailableVersion,
      ...(capabilities.threadCleanup ? [
        sharedDisabledStates.cleanupArmBeforeDryRun,
        sharedDisabledStates.cleanupConfirmBeforeArmed,
        sharedDisabledStates.cleanupDuringMutation
      ] : [])
    ],
    remoteApiOnly: capabilities.hostSurface === "linux_server_api" ? [...remoteApiOnlyVisualSurfaces] : [],
    desktopTauriOnly: [],
    forbidden: capabilities.hostSurface === "desktop_embedded_tauri"
      ? [...macosForbiddenVisualSurfaces]
      : [],
    cleanupRequiresDryRun: capabilities.threadCleanup,
    updateActions: opsUpdateActionView(null, capabilities).map((action) => action.label)
  };
}
