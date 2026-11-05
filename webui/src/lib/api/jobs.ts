import type {
  ArchiveDeletePlan,
  DshArchiveDeletePlan,
  DshArchiveDeleteRequest,
  DshArchiveDeleteResult,
  ArchiveDeleteResult,
  HiddenThreadDeletePlan,
  HiddenThreadDeleteResult,
  JobRecord,
  OptionalResult,
  UnifiedCleanupDryRunRequest,
  UnifiedCleanupExecuteRequest,
  UnifiedCleanupPlan,
  UnifiedCleanupResult
} from "../../types";
import { callCommand } from "./transport";
import { normalizeOptionalResult, USE_DEMO } from "./shared";

export async function dryRunArchiveDelete(): Promise<ArchiveDeletePlan> {
  if (USE_DEMO) {
    return (await import("./demo")).demoArchiveDeletePlan();
  }
  return callCommand<ArchiveDeletePlan>("cleanup.archiveDryRun", {});
}

export async function startArchiveDelete(request: {
  candidates: import("../../types").HiddenThreadSelection[];
}): Promise<ArchiveDeleteResult> {
  if (USE_DEMO) return (await import("./demo")).demoArchiveDeleteResult();
  const confirmation = {
    confirmed: true,
    candidates: request.candidates
  };
  return callCommand<ArchiveDeleteResult>(
    "cleanup.archiveExecute",
    { request: confirmation }
  );
}

export async function dryRunHiddenThreadDelete(request?: import("../../types").HiddenScanRequest): Promise<HiddenThreadDeletePlan> {
  if (USE_DEMO) {
    return (await import("./demo")).demoHiddenThreadDeletePlan();
  }
  return callCommand<HiddenThreadDeletePlan>("cleanup.hiddenDryRun", request ? { request } : {});
}

export async function startHiddenThreadDelete(request: {
  expectedCount: number;
  candidates: import("../../types").HiddenThreadSelection[];
}): Promise<HiddenThreadDeleteResult> {
  if (USE_DEMO) {
    return (await import("./demo")).demoHiddenThreadDeleteResult();
  }
  const confirmation = {
    confirmed: true,
    expectedCount: request.expectedCount,
    candidates: request.candidates
  };
  return callCommand<HiddenThreadDeleteResult>(
    "cleanup.hiddenExecute",
    { request: confirmation }
  );
}

export async function listJobs(): Promise<JobRecord[]> {
  if (USE_DEMO) {
    return (await import("./demo")).demoJobs();
  }
  const payload = await callCommand<JobRecord[] | OptionalResult<JobRecord[]>>("jobs.list", { limit: 30 });
  const result = normalizeOptionalResult<JobRecord[]>(payload);
  return result.available && Array.isArray(result.data) ? result.data : [];
}

export async function getJob(id: string, includeOutput = true): Promise<JobRecord> {
  if (USE_DEMO) {
    return (await import("./demo")).demoJob(id);
  }
  return callCommand<JobRecord>("jobs.detail", { id, includeOutput });
}

export async function dryRunDshArchiveDelete(): Promise<DshArchiveDeletePlan> {
  if (USE_DEMO) return { available: false, archivedThreads: 0, candidates: [], issues: [] };
  return callCommand<DshArchiveDeletePlan>("cleanup.dshArchiveDryRun", {});
}

export async function startDshArchiveDelete(request: DshArchiveDeleteRequest): Promise<DshArchiveDeleteResult> {
  if (USE_DEMO) return { items: [], deletedThreads: 0, deletedBytes: 0 };
  return callCommand<DshArchiveDeleteResult>("cleanup.dshArchiveExecute", { request });
}

export async function dryRunUnifiedCleanup(request?: UnifiedCleanupDryRunRequest): Promise<UnifiedCleanupPlan> {
  if (USE_DEMO) return (await import("./demo")).demoUnifiedCleanupPlan(undefined, request);
  return callCommand<UnifiedCleanupPlan>("cleanup.unifiedDryRun", request ? { request } : {});
}

export async function startUnifiedCleanup(request: UnifiedCleanupExecuteRequest): Promise<UnifiedCleanupResult> {
  if (USE_DEMO) {
    return (await import("./demo")).demoUnifiedCleanupResult(request);
  }
  return callCommand<UnifiedCleanupResult>("cleanup.unifiedExecute", { request });
}
