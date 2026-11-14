import { replaceEqualDeep, type InfiniteData } from "@tanstack/react-query";
import type { ClaudeSessionDetail, ProviderSessionDetail, SubagentDetailResponse, GrokSessionDetail, ThreadDetail } from "../../types";
import { mergeBlocksPreservingHistory } from "../threadMessageStore";
import { historyCursorIndex, stableHistoryCursor } from "../domain/historyCursor";

import type { HistoryAdapter } from "./usePagedHistory";

type WindowBoundary = { hasMore?: boolean; cursor?: string | null };
type HistoryPageAdapter<Page> = {
  identity: (page: Page) => string;
  total: (page: Page) => number | undefined;
  merge: (previous: Page, incoming: Page) => Page;
};

function retainedBoundary(previous: WindowBoundary, incoming: WindowBoundary): WindowBoundary {
  // Polling moves the newest window forward. Paging must continue from the
  // earliest boundary already loaded, not from that sliding window.
  if (previous.hasMore === false) return previous;
  if (incoming.hasMore === false) return incoming;
  return { hasMore: incoming.hasMore, cursor: previous.cursor ?? incoming.cursor };
}

function retainHistoryWindows<Page>(oldData: unknown, newData: unknown, adapter: HistoryPageAdapter<Page>): InfiniteData<Page> {
  const previous = oldData as InfiniteData<Page> | undefined;
  const incoming = newData as InfiniteData<Page>;
  const oldHead = previous?.pages[0];
  const newHead = incoming.pages[0];
  if (!oldHead || !newHead || adapter.identity(oldHead) !== adapter.identity(newHead)) return incoming;
  const oldTotal = adapter.total(oldHead);
  const newTotal = adapter.total(newHead);
  // A shortened native stream invalidates the old positional cursors.
  if (oldTotal !== undefined && newTotal !== undefined && newTotal < oldTotal) return incoming;

  const pages = incoming.pages.map((page, index) => {
    const oldPage = previous.pages[index];
    return oldPage && adapter.identity(oldPage) === adapter.identity(page) ? adapter.merge(oldPage, page) : page;
  });
  // A refreshed window may already reach the beginning and return fewer pages.
  // Keep rendered messages until the thread identity changes or is invalidated.
  pages.push(...previous.pages.slice(pages.length));
  const pageParams = pages.map((_, index) => index < previous.pageParams.length ? previous.pageParams[index] : incoming.pageParams[index]);
  return replaceEqualDeep(previous, { ...incoming, pages, pageParams });
}

function preservedTimeline<T extends { timeline?: ClaudeSessionDetail["timeline"] }>(oldPage: T, page: T) {
  return page.timeline === undefined && oldPage.timeline !== undefined ? { timeline: oldPage.timeline } : {};
}

export function retainClaudeHistory(previous: unknown, incoming: unknown): InfiniteData<ClaudeSessionDetail> {
  return retainHistoryWindows<ClaudeSessionDetail>(previous, incoming, {
    identity: page => JSON.stringify([page.summary.sessionKey, page.summary.id, page.agent?.agentId]),
    total: page => page.totalEvents,
    merge: (oldPage, page) => {
      const boundary = retainedBoundary(
        { hasMore: oldPage.hasMore, cursor: oldPage.beforeCursor },
        { hasMore: page.hasMore, cursor: page.beforeCursor }
      );
      return { ...page, ...preservedTimeline(oldPage, page), events: mergeBlocksPreservingHistory(oldPage.events, page.events), hasMore: Boolean(boundary.hasMore), beforeCursor: boundary.cursor };
    }
  });
}

export function retainSubagentHistory(previous: unknown, incoming: unknown): InfiniteData<SubagentDetailResponse> {
  return retainHistoryWindows<SubagentDetailResponse>(previous, incoming, {
    identity: page => JSON.stringify([page.rootThreadId, page.parentThreadId, page.agent.agentId, page.detail.summary.id]),
    total: page => page.detail.total_blocks,
    merge: (oldPage, page) => {
      const boundary = retainedBoundary(
        { hasMore: oldPage.detail.has_more_blocks, cursor: oldPage.detail.before_cursor },
        { hasMore: page.detail.has_more_blocks, cursor: page.detail.before_cursor }
      );
      return { ...page, detail: { ...page.detail, ...preservedTimeline(oldPage.detail, page.detail), blocks: mergeBlocksPreservingHistory(oldPage.detail.blocks, page.detail.blocks), has_more_blocks: boundary.hasMore, before_cursor: boundary.cursor } };
    }
  });
}

export function retainGrokHistory(previous: unknown, incoming: unknown): InfiniteData<GrokSessionDetail> {
  return retainHistoryWindows<GrokSessionDetail>(previous, incoming, {
    identity: page => page.summary.id,
    total: page => page.totalEvents,
    merge: (oldPage, page) => {
      const boundary = retainedBoundary({ hasMore: oldPage.hasMore, cursor: oldPage.beforeCursor }, { hasMore: page.hasMore, cursor: page.beforeCursor });
      // Old services don't supply stable event identities or pagination.
      const events = page.events.every(event => event.id) && oldPage.events.every(event => event.id)
        ? mergeBlocksPreservingHistory(oldPage.events as (typeof page.events[number] & { id: string })[], page.events as (typeof page.events[number] & { id: string })[]) : page.events;
      return { ...page, ...preservedTimeline(oldPage, page), events, hasMore: boundary.hasMore, beforeCursor: boundary.cursor };
    }
  });
}

export function retainCodexHistory(previous: unknown, incoming: unknown): InfiniteData<ThreadDetail> {
  return retainHistoryWindows<ThreadDetail>(previous, incoming, {
    identity: page => page.summary.id,
    total: page => page.total_blocks,
    merge: (oldPage, page) => {
      const boundary = retainedBoundary({ hasMore: oldPage.has_more_blocks, cursor: oldPage.before_cursor }, { hasMore: page.has_more_blocks, cursor: page.before_cursor });
      return { ...page, ...preservedTimeline(oldPage, page), blocks: mergeBlocksPreservingHistory(oldPage.blocks, page.blocks), has_more_blocks: boundary.hasMore, before_cursor: boundary.cursor };
    }
  });
}

export const claudeHistoryAdapter: HistoryAdapter<ClaudeSessionDetail> = {
  ids: page => page.events.map(event => event.id), history: page => page.history, total: page => page.totalEvents,
  cursor: page => page.hasMore ? page.beforeCursor ?? undefined : undefined,
  cursorAfter: (page, index) => page.events[index + 1]?.id,
  merge: retainClaudeHistory
};
export const grokHistoryAdapter: HistoryAdapter<GrokSessionDetail> = {
  ids: page => page.events.map(event => event.id ?? event.callId ?? event.userMessage?.id ?? "").filter(Boolean), history: page => page.history, total: page => page.totalEvents,
  cursor: page => page.hasMore ? page.beforeCursor ?? undefined : undefined,
  cursorAfter: (page, index) => page.events[index + 1]?.id,
  merge: retainGrokHistory
};
export const codexHistoryAdapter: HistoryAdapter<ThreadDetail> = {
  ids: page => page.blocks.map(block => block.id), history: page => page.history, total: page => page.total_blocks,
  cursor: page => page.has_more_blocks ? stableHistoryCursor(page) ?? undefined : undefined,
  cursorAfter: (page, index) => page.history && page.blocks[index]
    ? `a:${page.blocks[index].id}` : `b:${(page.before_cursor ? historyCursorIndex(page.before_cursor) : 0) + index + 1}`,
  invalidate: page => ({ ...page, history: page.history ? { ...page.history, reset: true } : undefined }),
  merge: retainCodexHistory
};
export const subagentHistoryAdapter: HistoryAdapter<SubagentDetailResponse> = {
  ids: page => codexHistoryAdapter.ids(page.detail), history: page => page.detail.history, total: page => page.detail.total_blocks,
  cursor: page => codexHistoryAdapter.cursor(page.detail), cursorAfter: (page, index) => codexHistoryAdapter.cursorAfter(page.detail, index),
  merge: retainSubagentHistory
};

export function retainProviderHistory(previous: unknown, incoming: unknown): InfiniteData<ProviderSessionDetail> {
  return retainHistoryWindows<ProviderSessionDetail>(previous, incoming, {
    identity: page => JSON.stringify([page.summary.sessionKey, page.summary.id]),
    total: page => page.totalEvents,
    merge: (oldPage, page) => {
      const boundary = retainedBoundary(
        { hasMore: oldPage.hasMore, cursor: oldPage.beforeCursor },
        { hasMore: page.hasMore, cursor: page.beforeCursor },
      );
      return {
        ...page,
        ...preservedTimeline(oldPage, page),
        events: mergeBlocksPreservingHistory(oldPage.events, page.events),
        hasMore: Boolean(boundary.hasMore),
        beforeCursor: boundary.cursor,
      };
    }
  });
}

export const providerHistoryAdapter: HistoryAdapter<ProviderSessionDetail> = {
  ids: page => page.events.map(event => event.id),
  history: page => page.history,
  total: page => page.totalEvents,
  cursor: page => page.hasMore ? page.beforeCursor ?? undefined : undefined,
  cursorAfter: (page, index) => page.events[index + 1]?.id,
  merge: retainProviderHistory,
};
