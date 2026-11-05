import { useMutationState, useQueries, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect } from "react";
import { getJob } from "../api/jobs";

export const backgroundJobKey = ["background-job"] as const;
export const isTerminalJob = (status?: string) => status === "succeeded" || status === "failed" || status === "cancelled";

export function useStartedJob(id?: string | null) {
  return useQuery({ queryKey: ["job", id], queryFn: () => getJob(id!), enabled: false });
}

// Track only jobs started in this client, including after their workspace is hidden.
export function useBackgroundJobs(enabled: boolean) {
  const qc = useQueryClient();
  const ids = useMutationState({
    filters: { mutationKey: backgroundJobKey, status: "success" },
    select: (mutation) => (mutation.state.data as { job_id?: string } | undefined)?.job_id
  });
  const queries = useQueries({ queries: [...new Set(enabled ? ids.filter((id): id is string => Boolean(id)) : [])].map((id) => ({
    queryKey: ["job", id],
    queryFn: async () => {
      let job = await getJob(id, false);
      if (isTerminalJob(job.status)) {
        job = await getJob(id);
        for (const key of ["jobs", "update-status", "probe-status", "probe-events"]) {
          void qc.invalidateQueries({ queryKey: [key] });
        }
      }
      return job;
    },
    staleTime: Infinity,
    refetchInterval: (query: { state: { data?: { status: string } } }) => isTerminalJob(query.state.data?.status) ? false as const : 2000
  })) });
  useEffect(() => {
    const completed = new Set(queries.filter(query => isTerminalJob(query.data?.status)).map(query => query.data?.id));
    for (const mutation of qc.getMutationCache().findAll({ mutationKey: backgroundJobKey, status: "success" })) {
      const id = (mutation.state.data as { job_id?: string } | undefined)?.job_id;
      if (!id || completed.has(id)) qc.getMutationCache().remove(mutation);
    }
  }, [qc, queries]);
  return queries;
}
