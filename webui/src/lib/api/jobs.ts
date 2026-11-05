import type {
  HiddenThreadDeletePlan,
  JobRecord,
  OptionalResult,
  UnifiedCleanupDryRunRequest,
  UnifiedCleanupExecuteRequest,
  UnifiedCleanupPlan,
  UnifiedCleanupResult
} from "../../types";
import { callCommand } from "./transport";
import { normalizeOptionalResult, USE_DEMO } from "./shared";

export async function dryRunHiddenThreadDelete(request?: import("../../types").HiddenScanRequest): Promise<HiddenThreadDeletePlan> {
  if (USE_DEMO) {
    return (await import("./demo")).demoHiddenThreadDeletePlan();
  }
  return callCommand<HiddenThreadDeletePlan>("cleanup.hiddenDryRun", request ? { request } : {});
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
