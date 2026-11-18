import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useRef } from "react";
import {
  clearCodexAccountScope,
  dryRunUnifiedCleanup,
  startUnifiedCleanup,
  dryRunHiddenThreadDelete,
  getCodexAccountScopeStatus,
  getProbeSettings,
  getProbeStatus,
  getUpdateStatus,
  listJobs,
  saveProbeSettings,
  updates,
  type RuntimeCapabilityMatrix,
  type UnifiedUpdateAction
} from "../api";
import type {
  CodexAccountScopeResult,
  HiddenThreadDeletePlan,
  UnifiedCleanupPlan,
  UnifiedCleanupResult,
  UnifiedCleanupTargetSelection
} from "../../types";
import { backgroundJobKey } from "./jobs";
import { beforeMachineChange, connectionSnapshot, machineScope } from "../runtime";
import { HiddenScanRunner } from "./hiddenScan";
import { probeQueryKeys } from "./probe";
import { visiblePollInterval } from "./shared";

export const opsQueryKeys = {
  updateStatus: ["update-status"] as const,
  jobs: ["jobs"] as const,
  threads: ["threads"] as const
};

export function useOpsQueries({ historyOpen }: { historyOpen: boolean }) {
  return {
    update: useQuery({
      queryKey: [...opsQueryKeys.updateStatus, machineScope()] as const,
      queryFn: getUpdateStatus,
      staleTime: 15000
    }),
    jobs: useQuery({
      queryKey: [...opsQueryKeys.jobs, machineScope()] as const,
      queryFn: listJobs,
      enabled: historyOpen,
      refetchInterval: visiblePollInterval(5000)
    })
  };
}

export function useOpsActions({ capabilities }: { capabilities: RuntimeCapabilityMatrix }) {
  const qc = useQueryClient();
  const invalidateJobs = () => qc.invalidateQueries({ queryKey: opsQueryKeys.jobs });

  return {
    updateJob: useMutation({
      mutationKey: backgroundJobKey,
      gcTime: Infinity,
      mutationFn: ({ action }: { action: UnifiedUpdateAction }) => {
        if (action === "check") return updates.check();
        if (action === "install") return updates.install();
        return updates.prune(capabilities);
      },
      onSuccess: (result) => {
        if (result.status) {
          qc.setQueryData([...opsQueryKeys.updateStatus, machineScope()], result.status);
        }
        invalidateJobs();
        qc.invalidateQueries({ queryKey: opsQueryKeys.updateStatus });
      }
    })
  };
}

export function useUnifiedCleanupActions(input: {
  capabilities: RuntimeCapabilityMatrix;
  onUnifiedDryRun: (plan: UnifiedCleanupPlan) => void;
  onUnifiedExecute: (result: UnifiedCleanupResult) => void;
  onHiddenDryRun: (plan: HiddenThreadDeletePlan) => void;
}) {
  const qc = useQueryClient();
  const { capabilities } = input;
  const scan = useRef<HiddenScanRunner | null>(null);
  const generation = useRef(0);
  useEffect(() => {
    const abandon = async () => {
      generation.current += 1;
      await scan.current?.stop();
    };
    const unsubscribe = beforeMachineChange(abandon);
    return () => {
      unsubscribe();
      void abandon();
    };
  }, []);
  const requireThreadCleanup = () => {
    if (!capabilities.threadCleanup) {
      throw new Error("当前运行时不支持线程清理动作");
    }
  };
  const invalidateAllCleanedProviders = () => {
    for (const key of [
      opsQueryKeys.jobs,
      opsQueryKeys.threads,
      ["claude_code"] as const,
      ["grok"] as const,
      ["antigravity"] as const,
      ["dsh"] as const
    ]) {
      void qc.invalidateQueries({ queryKey: key });
    }
  };

  return {
    stopHiddenScan: () => scan.current?.stop(),
    unifiedDryRun: useMutation({
      mutationFn: () => {
        requireThreadCleanup();
        return dryRunUnifiedCleanup();
      },
      onSuccess: input.onUnifiedDryRun
    }),
    hiddenDryRun: useMutation({
      mutationFn: async () => {
        requireThreadCleanup();
        const current = ++generation.current;
        const revision = connectionSnapshot().revision;
        const valid = () => generation.current === current && connectionSnapshot().revision === revision;
        await scan.current?.stop();
        if (!valid()) throw new Error("连接已变化，已停止旧扫描");
        const runner = new HiddenScanRunner(dryRunHiddenThreadDelete, (plan) => {
          if (valid()) input.onHiddenDryRun(plan);
        });
        scan.current = runner;
        try {
          return await runner.run();
        } catch (error) {
          void runner.stop();
          throw error;
        }
      }
    }),
    unifiedExecute: useMutation({
      mutationFn: ({
        scopes,
        targets
      }: {
        scopes: string[];
        targets: UnifiedCleanupTargetSelection[];
      }) => {
        requireThreadCleanup();
        return startUnifiedCleanup({ confirmed: true, scopes, targets });
      },
      onSuccess: (result) => {
        input.onUnifiedExecute(result);
        invalidateAllCleanedProviders();
      }
    })
  };
}

/** The service time-boxes each call; continue while a call still clears files and leaves some. */
export async function clearAllCodexAccountScope(): Promise<CodexAccountScopeResult> {
  let clearedThreads = 0;
  let clearedFiles = 0;
  for (;;) {
    const round = await clearCodexAccountScope();
    clearedThreads += round.clearedThreads;
    clearedFiles += round.clearedFiles;
    // Every call retries what is still scoped, so the last one lists the current reasons.
    if (round.remainingFiles === 0 || round.clearedFiles === 0) return { ...round, clearedThreads, clearedFiles };
  }
}

export function useCodexAccountScope(enabled: boolean) {
  const qc = useQueryClient();
  const settings = useQuery({ queryKey: probeQueryKeys.settings, queryFn: getProbeSettings, enabled, staleTime: 15000 });
  const probe = useQuery({ queryKey: probeQueryKeys.status, queryFn: () => getProbeStatus(), enabled, staleTime: 10000 });
  const check = useMutation({ mutationFn: getCodexAccountScopeStatus });
  const clear = useMutation({ mutationFn: clearAllCodexAccountScope, onSuccess: () => check.mutate() });
  const save = useMutation({
    mutationFn: (value: boolean) => saveProbeSettings({ probe: { hooks: { clear_account_scope: value } } }),
    onSuccess: saved => qc.setQueryData(probeQueryKeys.settings, { available: true, data: saved })
  });
  return {
    check,
    clear,
    auto: {
      // The switch only takes effect inside the installed Codex Stop hook.
      hookInstalled: probe.data?.data?.hook_status === "managed",
      enabled: settings.data?.data?.probe.hooks?.clear_account_scope === true,
      pending: save.isPending,
      error: save.error,
      set: (value: boolean) => save.mutate(value)
    }
  };
}
