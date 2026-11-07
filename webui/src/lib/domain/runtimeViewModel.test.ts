import { describe, expect, test } from "vitest";
import type {
  HiddenScanProgress,
  HiddenThreadDeletePlan,
  UpdateStatus
} from "../../types";
import type { RuntimeCapabilityMatrix } from "./capabilities";
import {
  canStartHiddenThreadDelete,
  failureCategoryLabel,
  opsUpdateActionView,
  probeStateLabel,
} from "./runtimeViewModel";

const linuxWebCapabilities: RuntimeCapabilityMatrix = {
  runtimeKind: "web",
  hostSurface: "linux_server_api",
  webAuth: true,
  logout: true,
  securitySettings: true,
  codexStatePaths: true,
  updatePrune: true,
  threadCleanup: true,
  threadArchiveActions: true,
  updateServiceLabels: true,
  desktopWebuiControl: false,
  forkAction: true,
  approvalActions: true
};

const macosTauriCapabilities: RuntimeCapabilityMatrix = {
  runtimeKind: "desktop",
  hostSurface: "desktop_embedded_tauri",
  webAuth: false,
  logout: false,
  securitySettings: false,
  codexStatePaths: false,
  updatePrune: false,
  threadCleanup: true,
  threadArchiveActions: true,
  updateServiceLabels: false,
  desktopWebuiControl: true,
  forkAction: false,
  approvalActions: false
};

describe("probeStateLabel", () => {
  test("labels stale and missing hook states for repair workflows", () => {
    expect(probeStateLabel("stale")).toBe("需修复");
    expect(probeStateLabel("missing")).toBe("未安装");
  });
});

function hiddenPlan(): HiddenThreadDeletePlan {
  return {
    total_threads: 9,
    visible_threads: 7,
    hidden_threads: 2,
    archived_threads: 0,
    session_index_lines: 9,
    rollout_files: 9,
    hidden_ids: ["child-a", "child-b"],
    candidates: [
      { id: "child-a", title: "child a", fingerprint: "fingerprint-a", allowed: true, reason: null },
      { id: "child-b", title: "child b", fingerprint: "fingerprint-b", allowed: true, reason: null }
    ],
    scan: { id: "fixture-scan", phase: "complete", processed: 2, total: 2, cursor: 0, nextCursor: 2, hasMore: false, error: null },
    hidden_source_counts: { subagent: 2 },
    integrity: "ok"
  };
}

describe("runtime view-model helpers", () => {
  test("arms hidden deletion for located candidates regardless of legacy protection", () => {
    expect(canStartHiddenThreadDelete(hiddenPlan())).toBe(true);

    const blocked = hiddenPlan();
    blocked.candidates = blocked.candidates.map(candidate => ({
      ...candidate,
      fingerprint: null,
      allowed: false,
      reason: "thread is running"
    }));
    expect(canStartHiddenThreadDelete(blocked)).toBe(true);
    expect(canStartHiddenThreadDelete({ ...hiddenPlan(), integrity: "corrupt" })).toBe(true);
  });

  test("requires a complete, non-stopped scan before allowing hidden deletion", () => {
    const base = hiddenPlan();
    const incompleteCandidates = base.candidates.slice(0, 1);
    const scan = (overrides: Partial<HiddenScanProgress>): HiddenScanProgress => ({
      id: "scan-fixture",
      phase: "scanning",
      processed: 1,
      total: 2,
      cursor: 0,
      nextCursor: 1,
      hasMore: true,
      error: null,
      ...overrides
    });
    const incomplete = {
      ...base,
      hidden_ids: incompleteCandidates.map(item => item.id),
      candidates: incompleteCandidates,
      scan: scan({})
    };
    const stopped = { ...base, scan: scan({
      phase: "stopped",
      processed: 2,
      cursor: 2,
      nextCursor: 2,
      hasMore: false
    }) };
    const completedWithMissingCandidates = {
      ...incomplete,
      scan: scan({
        phase: "complete",
        processed: 2,
        cursor: 0,
        nextCursor: 2,
        hasMore: false
      })
    };

    expect(canStartHiddenThreadDelete(incomplete)).toBe(false);
    expect(canStartHiddenThreadDelete(stopped)).toBe(false);
    expect(canStartHiddenThreadDelete(completedWithMissingCandidates)).toBe(false);
  });


  test("host policy uses host surface instead of incidental update flags", () => {
    expect(failureCategoryLabel("systemd_failure", {
      ...linuxWebCapabilities,
      updateServiceLabels: false
    })).toBe("systemd 失败");
    expect(failureCategoryLabel("systemd_failure", {
      ...macosTauriCapabilities,
      updateServiceLabels: true
    })).toBe("服务失败");
  });

});
