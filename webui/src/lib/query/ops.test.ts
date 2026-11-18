import { beforeEach, expect, test, vi } from "vitest";
import { clearAllCodexAccountScope, useCodexAccountScope, useOpsActions } from "./ops";
import { clearCodexAccountScope, getCodexAccountScopeStatus, saveProbeSettings } from "../api";

const state = vi.hoisted(() => ({ queries: [] as any[], setQueryData: vi.fn(), invalidate: vi.fn() }));
vi.mock("@tanstack/react-query", () => ({
  useQuery: (options: any) => { state.queries.push(options); return { data: options.data }; },
  useQueryClient: () => ({ setQueryData: state.setQueryData, invalidateQueries: state.invalidate }),
  useMutation: (options: any) => ({ ...options, mutate: vi.fn((value?: unknown) => options.mutationFn(value)) }),
}));
vi.mock("../runtime", () => ({ beforeMachineChange: vi.fn(), connectionSnapshot: vi.fn(), machineScope: () => "remote:fixture:" }));
vi.mock("../api", () => ({
  clearCodexAccountScope: vi.fn(), getCodexAccountScopeStatus: vi.fn(), getProbeSettings: vi.fn(), getProbeStatus: vi.fn(),
  saveProbeSettings: vi.fn(), dryRunUnifiedCleanup: vi.fn(), startUnifiedCleanup: vi.fn(), dryRunHiddenThreadDelete: vi.fn(),
  getUpdateStatus: vi.fn(), listJobs: vi.fn(), updates: { check: vi.fn(), install: vi.fn(), prune: vi.fn() },
}));
beforeEach(() => { vi.clearAllMocks(); state.queries = []; });

test("clearing continues while a call makes progress and leaves files", async () => {
  const busy = { threadId: "thread-busy", reason: "busy" as const };
  vi.mocked(clearCodexAccountScope)
    .mockResolvedValueOnce({ clearedThreads: 2, clearedFiles: 3, remainingFiles: 4, items: [{ threadId: "thread-late", reason: "busy" }] })
    .mockResolvedValueOnce({ clearedThreads: 1, clearedFiles: 1, remainingFiles: 2, items: [busy] })
    .mockResolvedValueOnce({ clearedThreads: 0, clearedFiles: 0, remainingFiles: 2, items: [busy] });
  await expect(clearAllCodexAccountScope()).resolves.toEqual({ clearedThreads: 3, clearedFiles: 4, remainingFiles: 2, items: [busy] });
  expect(clearCodexAccountScope).toHaveBeenCalledTimes(3);

  vi.mocked(clearCodexAccountScope).mockResolvedValueOnce({ clearedThreads: 1, clearedFiles: 1, remainingFiles: 0, items: [] });
  await expect(clearAllCodexAccountScope()).resolves.toEqual({ clearedThreads: 1, clearedFiles: 1, remainingFiles: 0, items: [] });
  expect(clearCodexAccountScope).toHaveBeenCalledTimes(4);
});

test("account scope actions stay off unsupported services and save only their own switch", async () => {
  useCodexAccountScope(false);
  expect(state.queries.map(query => query.enabled)).toEqual([false, false]);

  const saved = { probe: { hooks: { manage_stop_hook: true, clear_account_scope: true } } };
  vi.mocked(saveProbeSettings).mockResolvedValue(saved as any);
  vi.mocked(clearCodexAccountScope).mockResolvedValue({ clearedThreads: 1, clearedFiles: 1, remainingFiles: 0, items: [] });
  const scope = useCodexAccountScope(true);
  expect(scope.auto).toMatchObject({ hookInstalled: false, enabled: false });
  scope.auto.set(true);
  expect(saveProbeSettings).toHaveBeenCalledExactlyOnceWith({ probe: { hooks: { clear_account_scope: true } } });
  (scope as any).clear.onSuccess();
  expect(getCodexAccountScopeStatus).toHaveBeenCalledOnce();
});

test("a finished update job writes the status the machine-scoped query reads", () => {
  const actions = useOpsActions({ capabilities: {} as any });
  (actions.updateJob as any).onSuccess({ status: { current_version: "2.1.27" } });
  expect(state.setQueryData).toHaveBeenCalledExactlyOnceWith(["update-status", "remote:fixture:"], { current_version: "2.1.27" });
});
