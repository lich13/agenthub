import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  getProbeEvents,
  getProbeSettings,
  getProbeStatus,
  listJobs,
  saveProbeSettings,
  runProbeBarkTest,
  runProbeHooksInstall,
  runProbeHooksRemove,
  manageClaudeHooks,
  type HookProviderId,
} from "../api";
import { useConnection } from "./connection";
import type { ClaudeHooksAction, ProbeJobAction, ProbeSettings } from "../../types";
import type { RuntimeCapabilityMatrix } from "../api";
import type { ProbeSectionId } from "../probeUi";
import { preservePreviousQueryData, visiblePollInterval } from "./shared";
import { backgroundJobKey } from "./jobs";

export type { HookProviderId };

export const probeQueryKeys = {
  status: ["probe-status"] as const,
  settings: ["probe-settings"] as const,
  events: ["probe-events"] as const,
  jobs: ["jobs"] as const
};

export function useProbeQueries({ section, historyOpen }: { section: ProbeSectionId; historyOpen: boolean }) {
  const needsSettings = section === "settings";
  return {
    status: useQuery({
      queryKey: probeQueryKeys.status,
      queryFn: getProbeStatus,
      refetchInterval: visiblePollInterval(15000),
      staleTime: 10000,
      placeholderData: preservePreviousQueryData
    }),
    settings: useQuery({
      queryKey: probeQueryKeys.settings,
      queryFn: getProbeSettings,
      enabled: true,
      staleTime: 15000,
      placeholderData: preservePreviousQueryData
    }),
    events: useQuery({
      queryKey: probeQueryKeys.events,
      queryFn: () => getProbeEvents(10),
      enabled: section === "settings",
      refetchInterval: visiblePollInterval(15000),
      staleTime: 10000,
      placeholderData: preservePreviousQueryData
    }),
    jobs: useQuery({
      queryKey: probeQueryKeys.jobs,
      queryFn: listJobs,
      enabled: needsSettings && historyOpen,
      refetchInterval: visiblePollInterval(5000),
      placeholderData: preservePreviousQueryData
    })
  };
}

export function useProbeActions(input: {
  capabilities: RuntimeCapabilityMatrix;
  savePayload: (submittedDeviceKey?: string) => Partial<ProbeSettings>;
  onJobSuccess: (action: ProbeJobAction) => void;
  onJobError: (error: Error, action: ProbeJobAction) => void;
  onSaveSuccess: (settings: ProbeSettings, submittedDeviceKey?: string) => void;
  onSaveError: (error: Error) => void;
}) {
  const qc = useQueryClient();
  const invalidateProbe = () => {
    qc.invalidateQueries({ queryKey: probeQueryKeys.status });
    qc.invalidateQueries({ queryKey: probeQueryKeys.events });
  };

  const runProbeCommand = (action: ProbeJobAction) => {
    if (action === "bark-test") return runProbeBarkTest();
    if (action === "hooks-remove") return runProbeHooksRemove();
    return runProbeHooksInstall();
  };

  return {
    refresh: () => {
      invalidateProbe();
      qc.invalidateQueries({ queryKey: probeQueryKeys.settings });
    },
    job: useMutation({
      mutationKey: backgroundJobKey,
      gcTime: Infinity,
      mutationFn: (action: ProbeJobAction) => {
        return runProbeCommand(action);
      },
      onSuccess: (_result, action) => {
        input.onJobSuccess(action);
        qc.invalidateQueries({ queryKey: probeQueryKeys.jobs });
        invalidateProbe();
      },
      onError: (error: Error, action) => input.onJobError(error, action)
    }),
    save: useMutation({
      mutationFn: (submittedDeviceKey?: string) => saveProbeSettings(input.savePayload(submittedDeviceKey)),
      onSuccess: (settings, submittedDeviceKey) => {
        input.onSaveSuccess(settings, submittedDeviceKey);
        qc.invalidateQueries({ queryKey: probeQueryKeys.settings });
        invalidateProbe();
      },
      onError: (error: Error) => input.onSaveError(error)
    })
  };
}

export function useClaudeHooks(onSuccess: () => void, provider: HookProviderId = "claude") {
  const connection = useConnection();
  const local = connection.target === "local";
  const queryClient = useQueryClient();
  const status = useQuery({
    queryKey: ["claude-hooks", provider, connection.revision],
    queryFn: () => manageClaudeHooks("status", false, provider),
    enabled: local
  });
  const mutation = useMutation({
    mutationFn: (action: ClaudeHooksAction) => manageClaudeHooks(action, true, provider),
    onSuccess: async () => {
      onSuccess();
      await queryClient.invalidateQueries({ queryKey: ["claude-hooks", provider] });
    }
  });
  return { local, revision: connection.revision, status, mutation };
}
