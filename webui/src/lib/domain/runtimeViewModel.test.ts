import { describe, expect, test } from "vitest";
import type {
  ArchiveDeletePlan,
  GrokSessionSummary,
  HiddenScanProgress,
  HiddenThreadDeletePlan,
  HiddenThreadDeleteResult,
  UpdateStatus
} from "../../types";
import type { RuntimeCapabilityMatrix } from "./capabilities";
import { canPreviewGrokDelete } from "../../components/grok/GrokWorkspace";
import {
  canStartHiddenThreadDelete,
  failureCategoryLabel,
  opsUpdateActionView,
  opsWorkspacePanelTitles,
  opsWorkspaceViewModel,
  probeStateLabel,
  threadMessageControllerView
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

function archivePlan(): ArchiveDeletePlan {
  return {
    total_threads: 9,
    active_threads: 7,
    archived_threads: 2,
    session_index_lines: 9,
    rollout_files: 9,
    archived_ids: ["a", "b"],
    integrity: "ok"
  };
}

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
    hidden_source_counts: { subagent: 2 },
    integrity: "ok"
  };
}

function grokSession(overrides: Partial<GrokSessionSummary> = {}): GrokSessionSummary {
  return {
    id: "grok-fixture",
    title: "Fixture session",
    cwd: "/tmp/fixture",
    path: "/tmp/fixture/session.jsonl",
    messageCount: 1,
    status: "unknown",
    ...overrides
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

  test("Grok deletion ignores activity and legacy protection", () => {
    expect(canPreviewGrokDelete(grokSession({
      status: "running",
      deletionEligibility: { allowed: true, reason: null }
    }))).toBe(true);
    expect(canPreviewGrokDelete(grokSession({
      status: "recent",
      deletionEligibility: { allowed: false, reason: "fixture active task" }
    }))).toBe(true);
    expect(canPreviewGrokDelete(grokSession({ status: "recent" }))).toBe(true);
    expect(canPreviewGrokDelete(grokSession({ status: "running" }))).toBe(true);
    expect(canPreviewGrokDelete(grokSession({ status: "recent", deletionEligibility: null }))).toBe(true);
  });

  test("derives Ops workspace copy and gating from capabilities without leaking Linux labels to macOS", () => {
    const updateStatus: UpdateStatus = {
      current_version: "0.1.100",
      latest_version: "v0.1.103",
      update_available: true,
      channel: "stable",
      method: "macos_tauri_updater",
      state: "idle",
      failure_category: null,
      recommended_action: "Confirm install in the Tauri updater.",
      capabilities: ["check", "confirm_install"]
    };
    const hiddenDeleteResult: HiddenThreadDeleteResult = {
      skipped_threads: 0,
      failed_threads: 0,
      items: [
        { id: "child-a", status: "deleted", reason: null },
        { id: "child-b", status: "deleted", reason: null }
      ],
      before: hiddenPlan(),
      deleted_threads: 2,
      after_total_threads: 7,
      after_visible_threads: 7,
      after_hidden_threads: 0,
      after_archived_threads: 0,
      after_integrity: "ok",
      visible_threads: 7,
      hidden_threads: 0,
      integrity: "ok",
      deleted_rollout_files: 2
    };

    const view = opsWorkspaceViewModel({
      capabilities: macosTauriCapabilities,
      updateStatus,
      archivePlan: archivePlan(),
      hiddenPlan: hiddenPlan(),
      hiddenDeleteResult,
      archiveCleanup: { dryRunPending: false, armed: true, executePending: false },
      hiddenCleanup: { dryRunPending: false, armed: false, executePending: false }
    });

    expect(view.panelTitles).toEqual(opsWorkspacePanelTitles(macosTauriCapabilities));
    expect(view.updateActions).toEqual(opsUpdateActionView(updateStatus, macosTauriCapabilities));
    expect(view.archiveCleanup.stage).toEqual({ label: "等待确认", tone: "danger" });
    expect(view.archiveCleanup.canArm).toBe(true);
    expect(view.hiddenCleanup.stats).toEqual({
      hidden: 2,
      visible: 7,
      sourceCounts: "subagent:2",
      integrity: "ok"
    });
    expect(view.hiddenCleanup.rolloutDeleteResult).toBe("2");
    expect(view.hiddenCleanup.canArm).toBe(canStartHiddenThreadDelete(hiddenPlan()));
    expect(JSON.stringify(view)).not.toMatch(/systemd|Nginx|Turnstile|管理员密码|Public endpoint|Linux prune|Prune|panel\.example\.com/i);
  });

  test("keeps Linux service labels when capabilities advertise the Linux WebUI surface", () => {
    const view = opsWorkspaceViewModel({
      capabilities: linuxWebCapabilities,
      updateStatus: {
        current_version: "0.1.100",
        latest_version: "v0.1.103",
        update_available: false,
        channel: "stable",
        method: "linux_systemd_job",
        state: "idle",
        failure_category: "systemd_failure",
        recommended_action: "/usr/local/bin/agenthub-webd-update",
        capabilities: ["prune_backups"]
      },
      archivePlan: null,
      hiddenPlan: null,
      hiddenDeleteResult: null,
      archiveCleanup: { dryRunPending: false, armed: false, executePending: false },
      hiddenCleanup: { dryRunPending: true, armed: false, executePending: false }
    });

    expect(view.updateActions.map((action) => action.label)).toEqual(["检查更新", "清理更新备份"]);
    expect(failureCategoryLabel("systemd_failure", linuxWebCapabilities)).toBe("systemd 失败");
    expect(view.hiddenCleanup.stage).toEqual({ label: "扫描中", tone: "warning" });
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

  test("derives thread message controller inputs without exposing store/cache internals to components", () => {
    const selectedThreadSummary = {
      id: "thread-a",
      title: "Thread A",
      status: "Recent",
      message_count: 1
    } as const;
    const selectedDetail = {
      summary: selectedThreadSummary,
      messages: [],
      blocks: [],
      raw_event_count: 0
    };
    const view = threadMessageControllerView({
      threadId: "thread-a",
      selectedThreadSummary,
      selectedDetail
    });

    expect(view.hydration).toEqual({
      threadId: "thread-a",
      selectedThreadSummary,
      selectedDetail
    });
    expect(view.realtime.threadId).toBe("thread-a");
    expect(view.realtime.applyThreadTitleOverride).toBeTypeOf("function");
    expect(Object.keys(view).sort()).toEqual(["hydration", "realtime"]);
    expect(JSON.stringify(view)).not.toMatch(/queryClient|setQueryData|invalidateQueries|useQueryClient/);
  });
});
