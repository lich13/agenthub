import { createElement } from "react";
import { renderToString } from "react-dom/server";
import { QueryClient, useQuery, useQueryClient, type InfiniteData } from "@tanstack/react-query";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import type { ClaudeSessionDetail, GrokSessionDetail, HistoryMetadata, MessageBlock, SubagentDetailResponse, ThreadDetail } from "../../types";
import { claudeHistoryAdapter, codexHistoryAdapter, grokHistoryAdapter, subagentHistoryAdapter } from "./historyWindows";
import { historicalChanges, refreshHistory, usePagedHistory, type HistoryAdapter } from "./usePagedHistory";

vi.mock("@tanstack/react-query", async original => ({
  ...await original<typeof import("@tanstack/react-query")>(),
  useQuery: vi.fn(),
  useQueryClient: vi.fn()
}));

function version(revision = "revision-1", overrides: Partial<HistoryMetadata> = {}): HistoryMetadata {
  return { epoch: "source-fixture", revision, changes: [{ revision, changedIds: [] }], ...overrides };
}

function changedVersion(...changedIds: string[]): HistoryMetadata {
  return version("revision-2", { changes: [
    { revision: "revision-1", changedIds: [] },
    { revision: "revision-2", changedIds }
  ] });
}

type PageOptions = { total?: number; history?: HistoryMetadata | null; results?: Record<number, string> };
const rowId = (index: number) => `row-${index}`;
const resultFor = (index: number, options: PageOptions) => options.results?.[index] ?? `result-${index}`;

function codexPage(rows: number[], options: PageOptions = {}): ThreadDetail {
  const blocks: MessageBlock[] = rows.map(index => ({
    id: rowId(index), call_id: `call-${index}`, role: "tool", kind: "function_call_output",
    tool_name: "exec_command", text: resultFor(index, options), status: "completed", questions: []
  }));
  return {
    summary: { id: "thread-fixture", title: "Fixture", status: "Recent", message_count: options.total ?? 8 },
    blocks, messages: [], raw_event_count: options.total ?? 8, total_blocks: options.total ?? 8,
    has_more_blocks: rows[0] > 0, before_cursor: rows[0] > 0 ? `b:${rows[0]}` : null,
    history: options.history === null ? undefined : options.history ?? version()
  };
}

function claudePage(rows: number[], options: PageOptions = {}): ClaudeSessionDetail {
  return {
    summary: {
      id: "session-fixture", sessionKey: "claude:fixture", title: "Fixture", cwd: "/fixture/workspace",
      path: "/fixture/session.jsonl", messageCount: options.total ?? 8, status: "idle", formatVersion: "1",
      canRename: false, canDelete: false
    },
    events: rows.map(index => ({ id: rowId(index), callId: `call-${index}`, kind: "tool_call", role: "Bash", result: resultFor(index, options), status: "completed" })),
    totalEvents: options.total ?? 8, hasMore: rows[0] > 0, beforeCursor: rows[0] > 0 ? rowId(rows[0]) : null,
    history: options.history === null ? undefined : options.history ?? version()
  };
}

function grokPage(rows: number[], options: PageOptions = {}): GrokSessionDetail {
  return {
    summary: { id: "grok-fixture", title: "Fixture", cwd: "/fixture/workspace", path: "/fixture/session.jsonl", messageCount: options.total ?? 8, status: "idle" },
    events: rows.map(index => ({ id: rowId(index), callId: `call-${index}`, kind: "tool_result", text: resultFor(index, options), status: "completed" })),
    totalEvents: options.total ?? 8, hasMore: rows[0] > 0, beforeCursor: rows[0] > 0 ? rowId(rows[0]) : null,
    history: options.history === null ? undefined : options.history ?? version()
  };
}

function subagentPage(rows: number[], options: PageOptions = {}): SubagentDetailResponse {
  return {
    rootThreadId: "root-fixture", parentThreadId: "parent-fixture",
    agent: { agentId: "child-fixture", name: "Fixture child", status: "completed", available: true },
    detail: { ...codexPage(rows, options), summary: { id: "child-fixture", title: "Fixture child", status: "Recent", message_count: options.total ?? 8 } }
  };
}

function claudeSubagentPage(rows: number[], options: PageOptions = {}): ClaudeSessionDetail {
  return { ...claudePage(rows, options), agent: { agentId: "child-fixture", name: "Fixture child", status: "unknown", available: true } };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>(done => { resolve = done; });
  return { promise, resolve };
}

function providerTests<Page>(name: string, page: (rows: number[], options?: PageOptions) => Page, adapter: HistoryAdapter<Page>, cursor: (beforeIndex: number) => string, results: (page: Page) => Record<string, string | null | undefined>, resultCursor = (targetIndex: number) => cursor(targetIndex + 1)) {
  const previous = (): InfiniteData<Page> => ({
    pages: [page([6, 7]), page([4, 5]), page([2, 3])],
    pageParams: [undefined, cursor(6), cursor(4)]
  });
  const refresh = (old: InfiniteData<Page>, head: Page, read: (before?: string) => Promise<Page>, signal = new AbortController().signal) => refreshHistory(old, head, read, adapter, signal);

  describe(`${name} paged history refresh`, () => {
    test("three unchanged loaded pages need only the newest-window request", async () => {
      const old = previous();
      const read = vi.fn(async (_before?: string) => page([6, 7]));
      const next = await refresh(old, await read(), read);
      expect(read.mock.calls).toEqual([[]]);
      expect(next).toBe(old);
      expect(next.pageParams).toEqual([undefined, cursor(6), cursor(4)]);
      expect(adapter.cursor(next.pages[2])).toBe(cursor(2));
    });

    test("refreshes a changed visible tool result by ID without fetching an unloaded change", async () => {
      const old = previous();
      const history = changedVersion("row-4", "row-0");
      const read = vi.fn(async (_before?: string) => page([3, 4], { history, results: { 4: "finished-tool-result" } }));
      const next = await refresh(old, page([6, 7], { history }), read);
      expect(read.mock.calls).toEqual([[resultCursor(4)]]);
      expect(results(next.pages[1])["row-4"]).toBe("finished-tool-result");
      expect(next.pages[2]).toBe(old.pages[2]);
      expect(next.pages.flatMap(adapter.ids)).not.toContain("row-0");
      expect(adapter.history(next.pages[0])?.revision).toBe("revision-2");
      expect(results(old.pages[1])["row-4"]).toBe("result-4");
    });

    test("refreshes old rows accumulated in the first page and retains the newest metadata", async () => {
      const old = { pages: [page([2, 3, 4, 5, 6, 7])], pageParams: [undefined] };
      const history = changedVersion("row-3", "row-5");
      const read = vi.fn(async (before?: string) => {
        if (before === resultCursor(5)) return page([4, 5], { results: { 5: "updated-five" } });
        if (before === resultCursor(3)) return page([2, 3], { results: { 3: "updated-three" } });
        throw new Error(`Unexpected fixture cursor: ${before}`);
      });
      const next = await refresh(old, page([6, 7], { history }), read);
      expect(read.mock.calls).toEqual([[resultCursor(5)], [resultCursor(3)]]);
      expect(adapter.ids(next.pages[0])).toEqual(["row-2", "row-3", "row-4", "row-5", "row-6", "row-7"]);
      expect(results(next.pages[0])).toMatchObject({ "row-3": "updated-three", "row-5": "updated-five" });
      expect(adapter.history(next.pages[0])?.revision).toBe("revision-2");
      expect(adapter.cursor(next.pages[0])).toBe(cursor(2));
    });

    test("safely refreshes every loaded older window when the prior revision left the journal", async () => {
      const history = version("revision-4", { changes: [{ revision: "revision-3", changedIds: [] }, { revision: "revision-4", changedIds: [] }] });
      const read = vi.fn(async (before?: string) => {
        if (before === resultCursor(5)) return page([4, 5], { history, results: { 4: "updated-four" } });
        if (before === resultCursor(3)) return page([2, 3], { history, results: { 2: "updated-two" } });
        throw new Error(`Unexpected fixture cursor: ${before}`);
      });
      const next = await refresh(previous(), page([6, 7], { history }), read);
      expect(read.mock.calls).toEqual([[resultCursor(5)], [resultCursor(3)]]);
      expect(results(next.pages[1])["row-4"]).toBe("updated-four");
      expect(results(next.pages[2])["row-2"]).toBe("updated-two");
      expect(next.pageParams).toEqual([undefined, cursor(6), cursor(4)]);
    });

    test("discards retained windows when the journal records a branch reset", async () => {
      const history = version("revision-3", { changes: [
        { revision: "revision-1", changedIds: [] },
        { revision: "revision-2", changedIds: [], reset: true },
        { revision: "revision-3", changedIds: [] }
      ] });
      const head = page([6, 7], { history });
      const read = vi.fn(async () => { throw new Error("A branch reset must not refresh the old windows"); });
      const next = await refresh(previous(), head, read);
      expect(next).toEqual({ pages: [adapter.invalidate?.(head) ?? head], pageParams: [undefined] });
      expect(read).not.toHaveBeenCalled();
    });

    test.each([
      ["source epoch replacement", { history: version("revision-2", { epoch: "replacement-fixture" }) }],
      ["a shortened native stream", { total: 4, history: changedVersion() }]
    ] satisfies [string, PageOptions][])("discards loaded windows after %s", async (_label, options) => {
      const head = page([2, 3], options);
      const read = vi.fn(async () => { throw new Error("Unexpected historical read"); });
      const next = await refresh(previous(), head, read);
      expect(next).toEqual({ pages: [adapter.invalidate?.(head) ?? head], pageParams: [undefined] });
      expect(read).not.toHaveBeenCalled();
    });

    test("fills every intervening window when an append is larger than the newest page", async () => {
      const history = changedVersion("row-12", "row-13");
      const options = { total: 14, history };
      const read = vi.fn(async (before?: string) => {
        if (before === cursor(12)) return page([10, 11], options);
        if (before === cursor(10)) return page([8, 9], options);
        if (before === cursor(8)) return page([6, 7], options);
        throw new Error(`Unexpected fixture cursor: ${before}`);
      });
      const next = await refresh(previous(), page([12, 13], options), read);
      expect(read.mock.calls).toEqual([[cursor(12)], [cursor(10)], [cursor(8)]]);
      expect(adapter.ids(next.pages[0])).toEqual(["row-6", "row-7", "row-8", "row-9", "row-10", "row-11", "row-12", "row-13"]);
      expect(next.pages.slice(1).flatMap(adapter.ids)).toEqual(["row-4", "row-5", "row-2", "row-3"]);
      expect(next.pageParams).toEqual([undefined, cursor(6), cursor(4)]);
      expect(adapter.total(next.pages[0])).toBe(14);
    });

    test("rejects a late historical response after cancellation without mutating the retained pages", async () => {
      const old = previous();
      const snapshot = structuredClone(old);
      const response = deferred<Page>();
      const controller = new AbortController();
      const pending = refresh(old, page([6, 7], { history: changedVersion("row-4") }), () => response.promise, controller.signal);
      controller.abort();
      response.resolve(page([3, 4], { results: { 4: "late-result" } }));
      await expect(pending).rejects.toMatchObject({ name: "AbortError" });
      expect(old).toEqual(snapshot);
    });

    test("rejects a historical response from a different source epoch", async () => {
      const old = previous();
      const snapshot = structuredClone(old);
      const read = vi.fn(async () => page([3, 4], { history: version("revision-2", { epoch: "replacement-fixture" }) }));
      await expect(refresh(old, page([6, 7], { history: changedVersion("row-4") }), read)).rejects.toThrow("历史来源已变化");
      expect(old).toEqual(snapshot);
    });
  });
}

providerTests("Codex", codexPage, codexHistoryAdapter, index => `b:${index}:row-${index}`, page => Object.fromEntries(page.blocks.map(block => [block.id, block.text])), index => `a:row-${index}`);
providerTests("Claude", claudePage, claudeHistoryAdapter, rowId, page => Object.fromEntries(page.events.map(event => [event.id, event.result])));
providerTests("Claude child", claudeSubagentPage, claudeHistoryAdapter, rowId, page => Object.fromEntries(page.events.map(event => [event.id, event.result])));
providerTests("Grok", grokPage, grokHistoryAdapter, rowId, page => Object.fromEntries(page.events.map(event => [event.id!, event.text])));
providerTests("Subagent", subagentPage, subagentHistoryAdapter, index => `b:${index}:row-${index}`, page => Object.fromEntries(page.detail.blocks.map(block => [block.id, block.text])), index => `a:row-${index}`);

describe("Codex stable history cursor capability", () => {
  test("metadata enables stable page and changed-row anchors for root and child threads", () => {
    const root = codexPage([4, 5]);
    const child = subagentPage([4, 5]);
    expect(codexHistoryAdapter.cursor(root)).toBe("b:4:row-4");
    expect(codexHistoryAdapter.cursorAfter(root, 0)).toBe("a:row-4");
    expect(subagentHistoryAdapter.cursor(child)).toBe("b:4:row-4");
    expect(subagentHistoryAdapter.cursorAfter(child, 1)).toBe("a:row-5");
  });

  test("older services retain positional page and changed-row cursors", () => {
    const root = codexPage([4, 5], { history: null });
    const child = subagentPage([4, 5], { history: null });
    expect(codexHistoryAdapter.cursor(root)).toBe("b:4");
    expect(codexHistoryAdapter.cursorAfter(root, 0)).toBe("b:5");
    expect(subagentHistoryAdapter.cursor(child)).toBe("b:4");
    expect(subagentHistoryAdapter.cursorAfter(child, 1)).toBe("b:6");
  });
});

describe("history revision journal", () => {
  test("unchanged revisions need no historical rows", () => {
    expect(historicalChanges(version(), version())).toEqual(new Set());
  });

  test("collects and deduplicates only changes after the retained revision", () => {
    expect(historicalChanges(version(), version("revision-3", { changes: [
      { revision: "revision-0", changedIds: ["too-old"] },
      { revision: "revision-1", changedIds: ["already-applied"] },
      { revision: "revision-2", changedIds: ["row-2", "row-4"] },
      { revision: "revision-3", changedIds: ["row-4", "row-6"] }
    ] }))).toEqual(new Set(["row-2", "row-4", "row-6"]));
  });

  test.each([
    ["missing old metadata", undefined, version()],
    ["missing new metadata", version(), undefined],
    ["changed source epoch", version(), version("revision-2", { epoch: "replacement-fixture" })],
    ["missing retained revision", version(), version("revision-3")],
    ["explicit reset", version(), version("revision-1", { reset: true })],
    ["intermediate reset", version(), version("revision-3", { changes: [
      { revision: "revision-1", changedIds: [] }, { revision: "revision-2", changedIds: [], reset: true }, { revision: "revision-3", changedIds: [] }
    ] })]
  ] satisfies [string, HistoryMetadata | undefined, HistoryMetadata | undefined][])("%s requires the safe fallback", (_label, old, incoming) => {
    expect(historicalChanges(old, incoming)).toBeNull();
  });
});

describe("legacy Grok history", () => {
  test("replaces full snapshots without history metadata, IDs or pagination", async () => {
    const summary = grokPage([]).summary;
    const old: InfiniteData<GrokSessionDetail> = { pages: [{ summary, events: [{ kind: "tool_result", text: "old-result" }] }], pageParams: [undefined] };
    const head: GrokSessionDetail = { summary, events: [{ kind: "tool_result", text: "fresh-result" }] };
    const read = vi.fn(async () => head);
    const next = await refreshHistory(old, head, read, grokHistoryAdapter, new AbortController().signal);
    expect(next.pages).toEqual([head]);
    expect(grokHistoryAdapter.cursor(next.pages[0])).toBeUndefined();
    expect(read).not.toHaveBeenCalled();
  });

  test("refreshes retained older pages when pagination exists without revision metadata", async () => {
    const old = { pages: [grokPage([6, 7], { history: null }), grokPage([4, 5], { history: null })], pageParams: [undefined, "row-6"] };
    const read = vi.fn(async () => grokPage([4, 5], { history: null, results: { 4: "fresh-result" } }));
    const next = await refreshHistory(old, grokPage([6, 7], { history: null }), read, grokHistoryAdapter, new AbortController().signal);
    expect(read.mock.calls).toEqual([["row-6"]]);
    expect(next.pages[1].events.find(event => event.id === "row-4")?.text).toBe("fresh-result");
    expect(next.pageParams).toEqual(old.pageParams);
  });

  test("drops an obsolete call identity after one bounded full-snapshot fallback", async () => {
    const summary = grokPage([]).summary;
    const old = { pages: [{ summary, events: [{ callId: "old-call", kind: "tool_result", text: "obsolete" }] }], pageParams: [undefined] };
    const head = { summary, events: [{ callId: "new-call", kind: "tool_result", text: "fresh" }] };
    const read = vi.fn(async () => head);
    const next = await refreshHistory(old, head, read, grokHistoryAdapter, new AbortController().signal);
    expect(read.mock.calls).toEqual([[undefined]]);
    expect(next).toEqual({ pages: [head], pageParams: [undefined] });
  });
});

describe("shared polling query boundary", () => {
  let client: QueryClient;
  beforeEach(() => {
    vi.clearAllMocks();
    client = new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: Infinity } } });
    vi.mocked(useQueryClient).mockReturnValue(client);
    vi.mocked(useQuery).mockReturnValue({ data: undefined, error: null, isFetching: false } as ReturnType<typeof useQuery>);
  });
  afterEach(() => { client.clear(); vi.unstubAllGlobals(); });

  function queryOptions(read: (before: string | undefined, signal: AbortSignal) => Promise<ClaudeSessionDetail>, interval = () => 2000) {
    function Probe() {
      usePagedHistory({ ...claudeHistoryAdapter, queryKey: ["fixture-history", "machine-a", "session-a"], enabled: true, read, interval });
      return null;
    }
    renderToString(createElement(Probe));
    return vi.mocked(useQuery).mock.calls.at(-1)![0] as unknown as {
      retry: boolean;
      queryFn: (context: { signal: AbortSignal }) => Promise<InfiniteData<ClaudeSessionDetail>>;
      refetchInterval: (query: { state: { data?: InfiniteData<ClaudeSessionDetail> } }) => number | false;
    };
  }

  test.each([1000, 2000, 5000])("hidden documents stop a %i ms polling interval", interval => {
    const options = queryOptions(async () => claudePage([6, 7]), () => interval);
    vi.stubGlobal("document", { visibilityState: "visible" });
    expect(options.refetchInterval({ state: {} })).toBe(interval);
    vi.stubGlobal("document", { visibilityState: "hidden" });
    expect(options.refetchInterval({ state: {} })).toBe(false);
    expect(options.retry).toBe(false);
  });

  test("only the newest page reaches the provider polling policy", () => {
    const newest = claudePage([6, 7]);
    const interval = vi.fn(() => 2000);
    const options = queryOptions(async () => newest, interval);
    vi.stubGlobal("document", { visibilityState: "visible" });
    options.refetchInterval({ state: { data: { pages: [newest, claudePage([4, 5])], pageParams: [undefined, "row-6"] } } });
    expect(interval).toHaveBeenCalledWith(newest);
  });

  test("a cancelled newest-window request rejects its late response", async () => {
    const response = deferred<ClaudeSessionDetail>();
    const read = vi.fn(() => response.promise);
    const options = queryOptions(read);
    const controller = new AbortController();
    const pending = options.queryFn({ signal: controller.signal });
    controller.abort();
    response.resolve(claudePage([6, 7]));
    await expect(pending).rejects.toMatchObject({ name: "AbortError" });
    expect(read).toHaveBeenCalledWith(undefined, controller.signal);
    expect(client.getQueryData(["fixture-history", "machine-a", "session-a"])).toBeUndefined();
  });
});
