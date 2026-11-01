import { useCallback, useEffect, useMemo, useState, type RefObject } from "react";
import { locateTimelineTarget, useSearchWorkspace } from "../components/common/SessionSearch";
import type { HistoryMetadata, SessionSearchResult, UserMessageContent } from "../types";
import type { SessionProvider } from "./api/sessions";
import { userTimelineEntries, type TimelineEntry } from "./domain/timelineViewModel";
import { machineScope } from "./query/connection";
import { useHistoryScroll } from "./useHistoryScroll";
import { useTimelineWindow, type TimelinePage } from "./useTimelineWindow";

export function groupedTimelineEntries(
  provider: SessionProvider,
  resolvedTimeline: TimelineEntry[] | null | undefined,
  groupedEvents: ReadonlyArray<
    | { kind: "item"; key: string; item: { kind: string; text?: string | null; userMessage?: UserMessageContent | null } }
    | { kind: string }
  >,
): TimelineEntry[] {
  if (resolvedTimeline) return resolvedTimeline;
  return userTimelineEntries(
    provider,
    groupedEvents.flatMap(entry =>
      entry.kind === "item" && "item" in entry
        ? [{ id: entry.key, kind: entry.item.kind, text: entry.item.text, userMessage: entry.item.userMessage }]
        : [],
    ),
  );
}

export function useNarrowViewport(breakpoint = "(max-width: 767px)"): boolean {
  const [narrow, setNarrow] = useState(
    () => typeof window !== "undefined" && window.matchMedia(breakpoint).matches,
  );
  useEffect(() => {
    const media = window.matchMedia(breakpoint);
    const update = () => setNarrow(media.matches);
    media.addEventListener("change", update);
    update();
    return () => media.removeEventListener("change", update);
  }, [breakpoint]);
  return narrow;
}

export function useProviderTimelineController<TEvent extends { id?: string | null }>(input: {
  provider: SessionProvider;
  scopePrefix?: string;
  sessionKey: string | null | undefined;
  streamRef: RefObject<HTMLDivElement>;
  loadedEvents: TEvent[];
  historyMeta?: HistoryMetadata | null;
  resolvedTimeline?: TimelineEntry[] | null;
  beforeCursor?: string | null;
  hasNextPage: boolean;
  ready: boolean;
  isLoading: boolean;
  isFetching: boolean;
  readWindowPage: (cursor: string) => Promise<TimelinePage<TEvent>>;
  fetchNextDetailPage: () => Promise<{ beforeCursor?: string | null; hasMore?: boolean | null }>;
  onSelectSearchSession: (result: SessionSearchResult) => void;
  setFeedback: (value: string) => void;
  pendingSearchState?: [SessionSearchResult | null, (value: SessionSearchResult | null) => void];
}) {
  const localSearchState = useState<SessionSearchResult | null>(null);
  const [pendingSearch, setPendingSearch] = input.pendingSearchState ?? localSearchState;
  const { provider, sessionKey, streamRef, loadedEvents, hasNextPage, isLoading, isFetching, setFeedback } = input;
  const scopeBase = `${machineScope()}:${input.scopePrefix ?? provider}:${sessionKey ?? ""}`;

  const navigation = useTimelineWindow({
    scope: scopeBase,
    streamRef,
    history: input.historyMeta,
    latestId: loadedEvents[loadedEvents.length - 1]?.id ?? undefined,
    id: (event: TEvent) => event.id ?? "",
    read: input.readWindowPage,
    onNavigate: () => {
      history.follow.current = false;
      setPendingSearch(null);
    },
    onLocated: follow => {
      history.follow.current = Boolean(follow);
      history.capture();
    },
  });

  const events = navigation.items ?? loadedEvents;
  const selectResult = useCallback(
    (result: SessionSearchResult) => {
      input.onSelectSearchSession(result);
      setPendingSearch(result);
    },
    [input.onSelectSearchSession, setPendingSearch],
  );
  useSearchWorkspace(
    useMemo(() => ({ provider, sessionKey: sessionKey ?? null, selectResult }), [provider, sessionKey, selectResult]),
  );

  const history = useHistoryScroll({
    scopeKey: `${scopeBase}:${navigation.generation}`,
    streamRef,
    cursor: navigation.active ? navigation.before : input.beforeCursor,
    hasMore: navigation.active ? navigation.hasMore : hasNextPage,
    ready: input.ready,
    initialPosition: navigation.active ? { top: 0, follow: false } : undefined,
    busy: navigation.busy || (!navigation.active && isFetching),
    searchActive: Boolean(pendingSearch),
    loadPage: async (cursor, isCurrent) => {
      if (navigation.active) return navigation.loadOlder(cursor, isCurrent);
      const page = await input.fetchNextDetailPage();
      return { cursor: page.beforeCursor, hasMore: Boolean(page.hasMore) };
    },
  });

  const handleTimelineSelect = useCallback(
    (id: string) => {
      history.follow.current = false;
      if (streamRef.current && locateTimelineTarget(id, streamRef.current, "auto")) {
        history.capture();
        return;
      }
      if (input.resolvedTimeline) navigation.select(id);
    },
    [history.capture, history.follow, input.resolvedTimeline, navigation.select, streamRef],
  );

  useEffect(() => {
    if (!pendingSearch || pendingSearch.sessionKey !== sessionKey || isLoading || isFetching || history.loading) {
      return;
    }
    if (navigation.active) {
      navigation.latest();
      return;
    }
    history.follow.current = false;
    const container = streamRef.current?.closest(".provider-detail") ?? undefined;
    if (locateTimelineTarget(pendingSearch.positionKey, container, "auto")) {
      history.capture();
      setFeedback("");
      setPendingSearch(null);
    } else if (hasNextPage) {
      if (!history.error) void history.loadOlder("search");
    } else {
      setFeedback("结果所在历史未加载，请刷新后重试");
      setPendingSearch(null);
    }
  }, [events, hasNextPage, history, isFetching, isLoading, navigation, pendingSearch, sessionKey, setFeedback, setPendingSearch, streamRef]);

  const loadOlderForRail =
    !input.resolvedTimeline && hasNextPage ? () => void history.loadOlder("search") : undefined;
  return { events, navigation, history, pendingSearch, setPendingSearch, handleTimelineSelect, loadOlderForRail };
}
