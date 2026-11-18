import { isValidElement, type ReactElement, type ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import type { SessionSearchRequest, SessionSearchResponse } from "../../types";
import { SearchPanel } from "./SessionSearch";

// The project uses Vitest's Node environment, so the panel's hooks are driven
// directly. The query stub fetches once per enabled key, as the real cache does
// within its stale time, so calls to the mocked RPC count search requests.
const hooks = vi.hoisted(() => ({
  index: 0, dirty: false,
  cells: new Map<number, unknown>(),
  deps: new Map<number, readonly unknown[] | undefined>(),
  cleanup: new Map<number, (() => void) | void>(),
  effects: [] as (() => void)[],
  queries: new Map<string, { data?: unknown }>(),
  searchSessions: vi.fn(),
  context: {
    workspace: { provider: "codex", sessionKey: "fixture-thread", selectResult: () => undefined },
    register: () => () => undefined
  }
}));

vi.mock("react", async original => ({
  ...await original<typeof import("react")>(),
  useContext: () => hooks.context,
  useState: (initial: unknown) => {
    const index = hooks.index++;
    if (!hooks.cells.has(index)) hooks.cells.set(index, initial);
    return [hooks.cells.get(index), (value: unknown) => {
      const next = typeof value === "function" ? value(hooks.cells.get(index)) : value;
      if (!Object.is(next, hooks.cells.get(index))) { hooks.cells.set(index, next); hooks.dirty = true; }
    }];
  },
  useRef: (initial: unknown) => {
    const index = hooks.index++;
    if (!hooks.cells.has(index)) hooks.cells.set(index, { current: initial });
    return hooks.cells.get(index);
  },
  useMemo: (compute: () => unknown, deps: readonly unknown[]) => {
    const index = hooks.index++;
    const previous = hooks.cells.get(index) as { deps: readonly unknown[]; value: unknown } | undefined;
    if (previous && deps.every((value, position) => Object.is(value, previous.deps[position]))) return previous.value;
    const value = compute();
    hooks.cells.set(index, { deps, value });
    return value;
  },
  useEffect: (effect: () => void | (() => void), deps?: readonly unknown[]) => {
    const index = hooks.index++;
    const previous = hooks.deps.get(index);
    if (!deps || !previous || deps.some((value, position) => !Object.is(value, previous[position]))) {
      hooks.deps.set(index, deps);
      hooks.effects.push(() => { hooks.cleanup.get(index)?.(); hooks.cleanup.set(index, effect()); });
    }
  }
}));
vi.mock("@tanstack/react-query", () => ({
  useQuery: (options: { queryKey: readonly unknown[]; queryFn: () => Promise<unknown>; enabled: boolean }) => {
    if (!options.enabled) return { data: undefined, isFetching: false, error: null };
    const key = JSON.stringify(options.queryKey);
    let query = hooks.queries.get(key);
    if (!query) {
      const started: { data?: unknown } = {};
      hooks.queries.set(key, query = started);
      void options.queryFn().then(data => { started.data = data; });
    }
    return { data: query.data, isFetching: !query.data, error: null };
  }
}));
vi.mock("../../lib/api/search", () => ({ searchSessions: hooks.searchSessions }));
vi.mock("../../lib/query/connection", () => ({ useConnection: () => ({ revision: 1, target: "local", sshHost: null }) }));

type Element = ReactElement<Record<string, any>>;

function elements(node: ReactNode): Element[] {
  if (Array.isArray(node)) return node.flatMap(elements);
  if (!isValidElement<Record<string, any>>(node)) return [];
  return [node, ...elements(node.props.children)];
}

function openPanel() {
  let tree: ReactNode;
  const render = () => {
    let remaining = 10;
    do {
      hooks.index = 0;
      hooks.dirty = false;
      tree = SearchPanel();
      hooks.effects.splice(0).forEach(effect => effect());
      if (!remaining--) throw new Error("Fixture render did not settle");
    } while (hooks.dirty);
  };
  const all = (predicate: (element: Element) => boolean) => elements(tree).filter(predicate);
  const input = () => all(element => element.type === "input")[0]!;
  render();
  all(element => element.props.className === "global-search-trigger")[0]!.props.onClick();
  render();
  return {
    input,
    type: (value: string) => { input().props.onChange({ target: { value } }); render(); },
    wait: (milliseconds: number) => { vi.advanceTimersByTime(milliseconds); render(); },
    settle: async () => { await Promise.resolve(); render(); },
    results: () => all(element => element.props.role === "option")
  };
}

const request = (query: string): SessionSearchRequest => ({
  provider: "codex", scope: "thread", sessionKey: "fixture-thread", query, cursor: null, limit: 50
});

beforeEach(() => {
  hooks.index = 0; hooks.dirty = false; hooks.cells.clear(); hooks.deps.clear(); hooks.cleanup.clear(); hooks.effects.length = 0;
  hooks.queries.clear();
  hooks.searchSessions.mockReset().mockImplementation(async (sent: SessionSearchRequest): Promise<SessionSearchResponse> => ({
    provider: sent.provider, scope: sent.scope, query: sent.query, truncated: false, nextCursor: null,
    results: [{
      resultId: `fixture-result:${sent.query}`, sessionKey: "fixture-thread", nativeId: "fixture-native",
      title: "Fixture thread", matchKind: "user_message", positionKey: "fixture-position", snippet: `${sent.query} fixture`
    }]
  }));
  vi.useFakeTimers();
  vi.stubGlobal("window", { addEventListener: vi.fn(), removeEventListener: vi.fn(), setTimeout, clearTimeout });
});
afterEach(() => {
  hooks.cleanup.forEach(cleanup => cleanup?.());
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe("SearchPanel requests", () => {
  test("continuous typing sends one trimmed request 200 ms after the last change", async () => {
    const view = openPanel();
    view.type("f");
    view.wait(150);
    view.type("fo");
    view.wait(150);
    view.type("  foo ");
    expect(view.input().props.value).toBe("  foo ");
    view.wait(199);
    expect(hooks.searchSessions).not.toHaveBeenCalled();
    view.wait(1);
    expect(hooks.searchSessions).toHaveBeenCalledExactlyOnceWith(request("foo"));
    await view.settle();
    expect(view.results().map(result => result.key)).toEqual(["fixture-result:foo"]);
  });

  test("trailing whitespace keeps the request and its results", async () => {
    const view = openPanel();
    view.type("foo");
    view.wait(200);
    await view.settle();
    expect(view.results()).toHaveLength(1);

    view.type("foo ");
    expect(view.input().props.value).toBe("foo ");
    expect(view.results()).toHaveLength(1);
    view.wait(200);
    await view.settle();
    expect(hooks.searchSessions).toHaveBeenCalledExactlyOnceWith(request("foo"));
    expect(view.results()).toHaveLength(1);
  });

  test("a changed query clears the previous results at once and searches after the pause", async () => {
    const view = openPanel();
    view.type("foo");
    view.wait(200);
    await view.settle();
    expect(view.results()).toHaveLength(1);

    view.type("food");
    expect(view.results()).toHaveLength(0);
    view.wait(199);
    expect(hooks.searchSessions).toHaveBeenCalledTimes(1);
    view.wait(1);
    expect(hooks.searchSessions).toHaveBeenCalledTimes(2);
    expect(hooks.searchSessions).toHaveBeenLastCalledWith(request("food"));
    await view.settle();
    expect(view.results().map(result => result.key)).toEqual(["fixture-result:food"]);
  });
});
