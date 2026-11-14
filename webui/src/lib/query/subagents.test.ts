import { beforeEach, describe, expect, test, vi } from "vitest";
import type { ClaudeSessionDetail, SubagentDetailResponse } from "../../types";
import { getClaudeSession } from "../api/claude";
import { useClaudeSubagentDetail } from "./claude";
import { getSubagentDetail } from "../api/threads";
import { machineScope } from "./connection";
import { useSubagentDetail } from "./subagents";
import { usePagedHistory } from "./usePagedHistory";

vi.mock("./usePagedHistory", () => ({ usePagedHistory: vi.fn() }));
vi.mock("../api/claude", () => ({ getClaudeSession: vi.fn() }));
vi.mock("../api/threads", () => ({ getSubagentDetail: vi.fn() }));
vi.mock("./connection", () => ({ machineScope: vi.fn(() => "local:0") }));

type QueryOptions = {
  queryKey: unknown[];
  enabled: boolean;
  read: (before: string | undefined, signal: AbortSignal) => Promise<SubagentDetailResponse>;
  cursor: (page: SubagentDetailResponse) => string | undefined;
  interval: (latest?: SubagentDetailResponse) => number;
};

function options(rootThreadId = "root-fixture", agentId: string | null | undefined = "child-fixture"): QueryOptions {
  useSubagentDetail(rootThreadId, agentId);
  const calls = vi.mocked(usePagedHistory).mock.calls;
  return calls[calls.length - 1][0] as unknown as QueryOptions;
}

function response(): SubagentDetailResponse {
  return {
    rootThreadId: "root-fixture", parentThreadId: "parent-fixture",
    agent: { agentId: "child-fixture", name: "示例子智能体", available: true, status: "running" },
    detail: {
      summary: { id: "child-fixture", title: "示例子智能体", status: "Running", message_count: 1 },
      blocks: [], messages: [], raw_event_count: 1, has_more_blocks: false
    }
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(machineScope).mockReturnValue("local:0");
  vi.mocked(getSubagentDetail).mockReset();
});

describe("subagent detail queries", () => {
  test("first and older-page requests retain the original root and selected child", async () => {
    const data = response();
    vi.mocked(getSubagentDetail).mockResolvedValue(data);
    const query = options();
    const signal = new AbortController().signal;
    expect(await query.read(undefined, signal)).toBe(data);
    expect(await query.read("block:120", signal)).toBe(data);
    expect(getSubagentDetail).toHaveBeenNthCalledWith(1, { rootThreadId: "root-fixture", agentId: "child-fixture", before: null, limit: 120 });
    expect(getSubagentDetail).toHaveBeenNthCalledWith(2, { rootThreadId: "root-fixture", agentId: "child-fixture", before: "block:120", limit: 120 });
  });

  test("query caches distinguish machine revision, root thread, and nested child", () => {
    const initial = options().queryKey;
    expect(options("other-root").queryKey).not.toEqual(initial);
    expect(options("root-fixture", "nested-fixture").queryKey).not.toEqual(initial);
    vi.mocked(machineScope).mockReturnValue("remote:1");
    expect(options().queryKey).not.toEqual(initial);
    const remote = options().queryKey;
    vi.mocked(machineScope).mockReturnValue("remote:2");
    expect(options().queryKey).not.toEqual(remote);
  });

  test.each([null, ""])("missing child identity %s disables fetching", agentId => {
    expect(options("root-fixture", agentId).enabled).toBe(false);
    expect(getSubagentDetail).not.toHaveBeenCalled();
  });

  test.each(["root", "child"])("a response with a mismatched %s identity is rejected", async field => {
    const data = response();
    if (field === "root") data.rootThreadId = "other-root";
    else data.agent.agentId = "other-child";
    vi.mocked(getSubagentDetail).mockResolvedValue(data);
    await expect(options().read(undefined, new AbortController().signal))
      .rejects.toThrow("子智能体记录已变化，请重新打开");
  });

  test("an aborted selection discards a late native response", async () => {
    let resolve!: (data: SubagentDetailResponse) => void;
    vi.mocked(getSubagentDetail).mockImplementation(() => new Promise(done => { resolve = done; }));
    const controller = new AbortController();
    const pending = options().read(undefined, controller.signal);
    controller.abort();
    resolve(response());
    await expect(pending).rejects.toMatchObject({ name: "AbortError" });
  });

  test("only an advertised earlier page produces a cursor", () => {
    const query = options();
    const data = response();
    data.detail.before_cursor = "block:120";
    expect(query.cursor(data)).toBeUndefined();
    data.detail.has_more_blocks = true;
    expect(query.cursor(data)).toBe("block:120");
    data.detail.before_cursor = null;
    expect(query.cursor(data)).toBeUndefined();
  });

  test("native failures stay visible and do not silently retry", async () => {
    const error = new Error("示例记录暂时不可读");
    vi.mocked(getSubagentDetail).mockRejectedValue(error);
    const query = options();
    await expect(query.read(undefined, new AbortController().signal)).rejects.toBe(error);
    expect(getSubagentDetail).toHaveBeenCalledTimes(1);
  });

  test.each(["running", "creating"] as const)("a %s child refreshes every two seconds", status => {
    const data = response();
    data.agent.status = status;
    expect(options().interval(data)).toBe(2000);
  });

  test.each(["completed", "failed", "interrupted", "unknown"] as const)("a %s child without active descendants keeps the idle interval", status => {
    const data = response();
    data.agent.status = status;
    expect(options().interval(data)).toBe(5000);
  });

  test.each(["running", "creating"] as const)("a settled child still refreshes quickly while its direct child is %s", status => {
    const data = response();
    data.agent.status = "completed";
    data.detail.subagents = {
      agents: [{ agentId: "nested-fixture", name: "直属示例", status, available: true }],
      counts: { creating: 0, running: 0, completed: 0, failed: 0, interrupted: 0, unknown: 0, [status]: 1 },
      complete: true
    };
    expect(options().interval(data)).toBe(2000);
  });

  test("settled child collections keep the idle interval", () => {
    const latest = response();
    latest.agent.status = "completed";
    latest.detail.subagents = {
      agents: [], counts: { creating: 0, running: 0, completed: 0, failed: 0, interrupted: 0, unknown: 0 }, complete: true
    };
    expect(options().interval(latest)).toBe(5000);
  });

  test("an unloaded query and a legacy response without the collection keep a safe idle interval", () => {
    const query = options();
    expect(query.interval()).toBe(5000);
    const legacy = response();
    legacy.agent.status = "completed";
    expect(legacy.detail.subagents).toBeUndefined();
    expect(query.interval(legacy)).toBe(5000);
  });
});


describe("Claude child detail queries", () => {
  function childOptions(parent = "parent-key", agent: string | undefined = "child-fixture") {
    useClaudeSubagentDetail(parent, agent);
    return vi.mocked(usePagedHistory).mock.calls.at(-1)![0] as unknown as Omit<QueryOptions, "read" | "interval"> & {
      read: (before: string | undefined, signal: AbortSignal) => Promise<ClaudeSessionDetail>; interval: () => number;
    };
  }
  const child = (): ClaudeSessionDetail => ({
    summary: { id: "parent", sessionKey: "parent-key", title: "Parent", cwd: "/fixture", path: "/fixture/parent.jsonl", messageCount: 0, status: "running", canRename: true, canDelete: true },
    agent: { agentId: "child-fixture", name: "Fixture child", status: "unknown", available: true }, events: [], totalEvents: 0, hasMore: false,
  });
  test("all pages pass parent and agent, unknown keeps polling and unloaded selections do not fetch", async () => {
    vi.mocked(getClaudeSession).mockResolvedValue(child());
    const query = childOptions();
    await query.read(undefined, new AbortController().signal);
    await query.read("oldest-child-event", new AbortController().signal);
    expect(getClaudeSession).toHaveBeenNthCalledWith(1, "parent-key", undefined, "child-fixture");
    expect(getClaudeSession).toHaveBeenNthCalledWith(2, "parent-key", "oldest-child-event", "child-fixture");
    expect(query.interval()).toBe(5000);
    expect(childOptions("parent-key", "").enabled).toBe(false);
    expect(childOptions("other-parent").queryKey).not.toEqual(query.queryKey);
    expect(childOptions("parent-key", "other-child").queryKey).not.toEqual(query.queryKey);
    vi.mocked(machineScope).mockReturnValue("remote:1");
    expect(childOptions().queryKey).not.toEqual(query.queryKey);
  });
  test.each(["parent", "agent"])("rejects mismatched %s identity", async field => {
    const response = child();
    if (field === "parent") response.summary.sessionKey = "other-parent";
    else response.agent!.agentId = "other-agent";
    vi.mocked(getClaudeSession).mockResolvedValue(response);
    await expect(childOptions().read(undefined, new AbortController().signal)).rejects.toThrow("子智能体记录已变化");
  });
  test("aborted selections discard late child responses", async () => {
    let finish!: (data: ClaudeSessionDetail) => void;
    vi.mocked(getClaudeSession).mockImplementation(() => new Promise(resolve => { finish = resolve; }));
    const controller = new AbortController();
    const pending = childOptions().read(undefined, controller.signal);
    controller.abort(); finish(child());
    await expect(pending).rejects.toMatchObject({ name: "AbortError" });
  });
});
