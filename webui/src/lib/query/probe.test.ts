import { beforeEach, expect, test, vi } from "vitest";
import { useProbeActions, useProbeQueries } from "./probe";
import { getProbeStatus } from "../api";
import { refreshProbeStatus } from "./probeStatus";

const state = vi.hoisted(() => ({ queries: [] as any[], invalidate: vi.fn() }));
vi.mock("@tanstack/react-query", () => ({
  useQuery: (options: any) => { state.queries.push(options); return options; },
  useQueryClient: () => ({ invalidateQueries: state.invalidate }),
  useMutation: (options: any) => options,
}));
vi.mock("./probeStatus", () => ({ probeStatusQueryKey: ["probe-status"], refreshProbeStatus: vi.fn() }));
vi.mock("../api", () => ({
  getProbeStatus: vi.fn(), getProbeSettings: vi.fn(), getProbeEvents: vi.fn(), listJobs: vi.fn(),
  saveProbeSettings: vi.fn(), runProbeBarkTest: vi.fn(), runProbeHooksInstall: vi.fn(), runProbeHooksRemove: vi.fn(), manageClaudeHooks: vi.fn(),
}));
beforeEach(() => { vi.clearAllMocks(); state.queries = []; });

test("ordinary polling uses cached status, manual refresh and save completion force server refresh", () => {
  useProbeQueries({ historyOpen: false });
  state.queries[0].queryFn();
  expect(getProbeStatus).toHaveBeenCalledExactlyOnceWith();
  expect(refreshProbeStatus).not.toHaveBeenCalled();
  const actions = useProbeActions({ savePayload: () => ({}), onJobSuccess: vi.fn(), onJobError: vi.fn(), onSaveSuccess: vi.fn(), onSaveError: vi.fn() });
  actions.refresh();
  expect(refreshProbeStatus).toHaveBeenCalledOnce();
  (actions.save as any).onSuccess({}, undefined);
  expect(refreshProbeStatus).toHaveBeenCalledTimes(2);
});
