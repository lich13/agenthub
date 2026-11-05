import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useMemo } from "react";
import type {
  ProviderDetailRequest,
  ProviderListRequest,
  ProviderSessionDetail,
  ProviderSessionSummary,
} from "../../types";
import {
  archiveAntigravitySession,
  getAntigravitySession,
  listAntigravitySessions,
  renameAntigravitySession,
  restoreAntigravitySession,
} from "../api/antigravity";
import {
  archiveDshSession,
  getDshSession,
  listDshSessions,
  renameDshSession,
  restoreDshSession,
} from "../api/dsh";
import { getGrokBotSession, listGrokBotSessions } from "../api/grokbot";
import { compareSessionMessageTime } from "../domain/sessionSelection";
import { machineScope, useConnection } from "./connection";
import { providerHistoryAdapter } from "./historyWindows";
import { preservePreviousQueryData, visiblePollInterval } from "./shared";
import { usePagedHistory } from "./usePagedHistory";

export type ArchivableProviderId = "antigravity" | "dsh";

export type ReadableProviderId = ArchivableProviderId | "grokbot";

export function readArchivableProviderSession(
  provider: ReadableProviderId,
  request: ProviderDetailRequest,
): Promise<ProviderSessionDetail> {
  return provider === "antigravity"
    ? getAntigravitySession(request)
    : provider === "grokbot" ? getGrokBotSession(request) : getDshSession(request);
}

function listArchivableProviderSessions(
  provider: ReadableProviderId,
  request: ProviderListRequest,
): Promise<ProviderSessionSummary[]> {
  return provider === "antigravity"
    ? listAntigravitySessions(request)
    : provider === "grokbot" ? listGrokBotSessions(request) : listDshSessions(request);
}

export function useProviderSessions(
  provider: ReadableProviderId,
  query: { q?: string; archived?: boolean },
  enabled: boolean,
) {
  const connection = useConnection();
  const q = query.q ?? "";
  const archived = Boolean(query.archived);
  return useQuery({
    queryKey: [provider, machineScope(), "sessions", archived ? "archived" : "active", q],
    queryFn: async () =>
      (await listArchivableProviderSessions(provider, { q, archived }))
        .slice()
        .sort(compareSessionMessageTime),
    enabled: enabled && !connection.changing,
    refetchInterval: visiblePollInterval(enabled ? 5000 : false),
    placeholderData: preservePreviousQueryData,
  });
}

export function useProviderDetail(
  provider: ReadableProviderId,
  sessionKey: string | null,
  enabled: boolean,
) {
  const connection = useConnection();
  const query = usePagedHistory<ProviderSessionDetail>({
    ...providerHistoryAdapter,
    queryKey: [provider, machineScope(), "session", sessionKey],
    enabled: Boolean(enabled && sessionKey && !connection.changing),
    read: before =>
      readArchivableProviderSession(provider, {
        sessionKey: sessionKey!,
        limit: 160,
        before,
      }),
    interval: latest => (latest?.summary.status === "running" ? 2000 : 5000),
  });
  const data = useMemo<ProviderSessionDetail | undefined>(() => {
    const pages = query.data?.pages;
    if (!pages?.length) return undefined;
    const latest = pages[0];
    return {
      ...latest,
      events: [...pages].reverse().flatMap(page => page.events),
      beforeCursor: pages[pages.length - 1].beforeCursor,
      hasMore: pages[pages.length - 1].hasMore,
    };
  }, [query.data]);
  return { ...query, data };
}

export function useProviderActions(provider: ArchivableProviderId) {
  const qc = useQueryClient();
  const invalidate = () => qc.invalidateQueries({ queryKey: [provider] });
  const rename = useMutation({
    mutationFn: ({ sessionKey, title }: { sessionKey: string; title: string }) =>
      provider === "antigravity"
        ? renameAntigravitySession({ sessionKey, title })
        : renameDshSession({ sessionKey, title }),
    onSuccess: invalidate,
  });
  const archive = useMutation({
    mutationFn: ({ sessionKey }: { sessionKey: string }) =>
      provider === "antigravity"
        ? archiveAntigravitySession({ sessionKey })
        : archiveDshSession({ sessionKey }),
    onSuccess: invalidate,
  });
  const restore = useMutation({
    mutationFn: ({ sessionKey }: { sessionKey: string }) =>
      provider === "antigravity"
        ? restoreAntigravitySession({ sessionKey })
        : restoreDshSession({ sessionKey }),
    onSuccess: invalidate,
  });
  return { rename, archive, restore };
}
