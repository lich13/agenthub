import type { QueryClient } from "@tanstack/react-query";
import { getProbeStatus } from "../api/probe";
import { connectionSnapshot } from "./connection";

export const probeStatusQueryKey = ["probe-status"] as const;

export async function refreshProbeStatus(client: QueryClient) {
  const revision = connectionSnapshot().revision;
  await client.cancelQueries({ queryKey: probeStatusQueryKey, exact: true });
  if (connectionSnapshot().revision !== revision) return;
  // fetchQuery exposes failures on the existing status query. Runtime routing
  // discards responses from a machine whose connection revision has changed.
  return client.fetchQuery({
    queryKey: probeStatusQueryKey,
    queryFn: () => getProbeStatus({ refresh: true }),
    staleTime: 0,
  }).catch(() => undefined);
}
