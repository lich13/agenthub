import { describe, expect, test } from "vitest";

import {
  opsWorkspacePanelTitles,
  opsWorkspaceVisibleCopy
} from "./runtimeViewModel";
import {
  macosForbiddenVisualSurfaces,
  sharedActionLabels,
  sharedCorePanelTitles,
  sharedDisabledStates,
  sharedNavigationLabels,
  visualContractForRuntime
} from "./visualContract";
import type { RuntimeCapabilityMatrix } from "./capabilities";

const remoteCapabilities: RuntimeCapabilityMatrix = {
  runtimeKind: "desktop",
  hostSurface: "linux_server_api",

  codexStatePaths: true,
  updatePrune: true,
  threadCleanup: true,
  threadArchiveActions: true,
  updateServiceLabels: true,
};

const macosTauriCapabilities: RuntimeCapabilityMatrix = {
  runtimeKind: "desktop",
  hostSurface: "desktop_embedded_tauri",

  codexStatePaths: false,
  updatePrune: false,
  threadCleanup: true,
  threadArchiveActions: true,
  updateServiceLabels: false,
};

describe("runtime capability rendering", () => {
  test("shared visual contract keeps remote and local machines on one layout vocabulary", () => {
    const linuxContract = visualContractForRuntime(remoteCapabilities);
    const macContract = visualContractForRuntime(macosTauriCapabilities);

    expect(linuxContract.sharedNavigation).toEqual([...sharedNavigationLabels]);
    expect(macContract.sharedNavigation).toEqual([...sharedNavigationLabels]);
    expect(linuxContract.sharedPanels).toEqual(expect.arrayContaining([...sharedCorePanelTitles]));
    expect(macContract.sharedPanels).toEqual([...sharedCorePanelTitles]);
    expect(linuxContract.sharedActions).toEqual(expect.arrayContaining([
      sharedActionLabels.unifiedDryRun,
      sharedActionLabels.unifiedCleanup,
      sharedActionLabels.unifiedConfirm
    ]));
    expect(macContract.sharedActions).toEqual(expect.arrayContaining([
      sharedActionLabels.unifiedDryRun,
      sharedActionLabels.unifiedCleanup,
      sharedActionLabels.unifiedConfirm
    ]));

    expect(linuxContract.cleanupRequiresDryRun).toBe(true);
    expect(macContract.cleanupRequiresDryRun).toBe(true);
    expect(linuxContract.disabledStates).toEqual(expect.arrayContaining([
      sharedDisabledStates.updateInstallWithoutAvailableVersion,
      sharedDisabledStates.cleanupArmBeforeDryRun,
      sharedDisabledStates.cleanupConfirmBeforeArmed,
      sharedDisabledStates.cleanupDuringMutation
    ]));
    expect(macContract.disabledStates).toEqual(expect.arrayContaining([
      sharedDisabledStates.updateInstallWithoutAvailableVersion,
      sharedDisabledStates.cleanupArmBeforeDryRun,
      sharedDisabledStates.cleanupConfirmBeforeArmed,
      sharedDisabledStates.cleanupDuringMutation
    ]));
  });

  test("task controls and desktop LAN service are absent from the visual contract", () => {
    const contract = visualContractForRuntime(macosTauriCapabilities);
    expect(contract.sharedNavigation).toEqual([
      "Codex",
      "Claude Code",
      "Grok Build",
      "Antigravity",
      "DeepSeek Harness",
      "Probe",
      "设置"
    ]);
    expect(contract.sharedActions).not.toEqual(expect.arrayContaining(["发送", "停止", "恢复 Goal"]));
    expect(contract.desktopTauriOnly).toEqual([]);
  });
});
