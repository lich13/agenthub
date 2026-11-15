import { createElement } from "react";
import { renderToString } from "react-dom/server";
import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";
import { beforeEach, expect, test, vi } from "vitest";
import { useSessionSelection } from "./sessions";
import { executeSessionBatch, previewSessionBatch } from "../api/sessions";

const observed = vi.hoisted(() => ({
  updates: [] as any[], busy: false, states: [] as any[], stateIndex: 0,
  mutations: [] as any[], resets: [] as ReturnType<typeof vi.fn>[],
  invalidate: vi.fn(),
  lifecycle: false, refs: [] as any[], refIndex: 0, effects: [] as (() => void)[],
  deps: [] as any[], effectIndex: 0, filter: "filtered", revision: 1,
  pending: [false, false], errors: [null, null] as (Error | null)[],
}));
vi.mock("react", async original => {
  const actual = await original<typeof import("react")>();
  return {
  ...actual,
  useRef: (initial: any) => {
    if (!observed.lifecycle) return actual.useRef(initial);
    const index = observed.refIndex++;
    return observed.refs[index] ??= { current: initial };
  },
  useEffect: (effect: () => void, deps: any[]) => {
    if (!observed.lifecycle) return;
    const index = observed.effectIndex++;
    if (!observed.deps[index] || deps.some((value, i) => value !== observed.deps[index][i])) {
      observed.deps[index] = deps; observed.effects.push(effect);
    }
  },
  useState: (initial: any) => {
    const index = observed.stateIndex++;
    const value = index in observed.states ? observed.states[index] : typeof initial === "function" ? initial() : initial;
    if (observed.lifecycle) observed.states[index] = value;
    return [value, (next: any) => {
      observed.updates.push(next);
      if (observed.lifecycle) observed.states[index] = typeof next === "function" ? next(observed.states[index]) : next;
    }];
  },
  };
});
vi.mock("@tanstack/react-query", () => ({
  useQueryClient: () => ({ invalidateQueries: observed.invalidate }),
  useMutation: (options: any) => {
    const index = observed.mutations.length;
    observed.mutations.push(options);
    const reset = vi.fn(); observed.resets.push(reset);
    return { error: observed.errors[index], isPending: observed.lifecycle ? observed.pending[index] : observed.busy, reset, mutate: vi.fn() };
  },
}));
vi.mock("./connection", () => ({ useConnection: () => ({ revision: observed.revision, target: "local" }) }));
vi.mock("../api/sessions", () => ({ executeSessionBatch: vi.fn(), previewSessionBatch: vi.fn() }));

const nativeSelectAll = readFileSync(new URL("../../../../src-tauri/src/select_all.js", import.meta.url), "utf8");
function selection(keys: string[], onSelectionStart?: () => void, onSucceeded?: () => void) {
  observed.stateIndex = 0;
  observed.refIndex = 0; observed.effectIndex = 0;
  if (observed.lifecycle) { observed.mutations = []; observed.resets = []; }
  let result!: ReturnType<typeof useSessionSelection>;
  function Probe() { result = useSessionSelection("codex", observed.filter, keys, onSucceeded, undefined, onSelectionStart); return null; }
  renderToString(createElement(Probe));
  for (const effect of observed.effects.splice(0)) effect();
  return result;
}
function nativeAction(batch: ReturnType<typeof useSessionSelection>, tagName = "BUTTON", scope = "", type = "text", insideList = true) {
  const execCommand = vi.fn();
  const select = vi.fn();
  const target = {
    tagName, type, select, isContentEditable: tagName === "EDITOR",
    closest: (selector: string) => scope && selector.split(",").includes(scope) ? {} : null,
    dispatchEvent(event: any) { event.target = target; if (insideList) batch.onListKeyDown(event); return !event.defaultPrevented; },
  };
  class KeyEvent {
    defaultPrevented = false;
    constructor(_type: string, options: object) { Object.assign(this, options); }
    preventDefault() { this.defaultPrevented = true; }
    stopPropagation() {}
  }
  runInNewContext(nativeSelectAll, { document: { activeElement: target, execCommand }, KeyboardEvent: KeyEvent });
  return { execCommand, select };
}
beforeEach(() => {
  observed.updates = []; observed.busy = false; observed.states = [];
  observed.mutations = []; observed.resets = []; observed.invalidate.mockReset();
  observed.lifecycle = false; observed.refs = []; observed.effects = []; observed.deps = [];
  observed.filter = "filtered"; observed.revision = 1; observed.pending = [false, false]; observed.errors = [null, null];
  vi.mocked(executeSessionBatch).mockReset(); vi.mocked(previewSessionBatch).mockReset();
});
test("native Edit Select All reaches the same selection action for only loaded filtered keys", () => {
  expect(nativeAction(selection(["visible-a", "visible-c"])).execCommand).not.toHaveBeenCalled();
  expect(observed.updates.at(-1).keys).toEqual(["visible-a", "visible-c"]);
});
test.each(["INPUT", "TEXTAREA"])("native Select All keeps text selection in %s", tag => {
  const action = nativeAction(selection(["underlying"]), tag);
  expect(action.select).toHaveBeenCalledOnce();
  expect(action.execCommand).not.toHaveBeenCalled();
  expect(observed.updates).toEqual([]);
});
test.each(["text", "search", "password", "number", "email"])("%s input keeps native text selection", type => {
  const action = nativeAction(selection(["underlying"]), "INPUT", "", type);
  expect(action.select).toHaveBeenCalledOnce();
  expect(action.execCommand).not.toHaveBeenCalled();
  expect(observed.updates).toEqual([]);
});
test("editable content uses native text selection", () => {
  expect(nativeAction(selection(["underlying"]), "EDITOR").execCommand).toHaveBeenCalledWith("selectAll");
  expect(observed.updates).toEqual([]);
});
test.each(["dialog", '[role="dialog"]', '[role="alertdialog"]', '[role="menu"]'])("%s focus cannot select the underlying list or page", scope => {
  expect(nativeAction(selection(["underlying"]), "BUTTON", scope).execCommand).not.toHaveBeenCalled();
  expect(observed.updates).toEqual([]);
  const event = { target: { tagName: "BUTTON", closest: (selector: string) => selector.split(",").includes(scope) }, key: "a", metaKey: true, preventDefault: vi.fn(), stopPropagation: vi.fn() };
  selection(["underlying"]).onListKeyDown(event as any);
  expect(observed.updates).toEqual([]);
});
test.each(["checkbox", "radio", "button"])("%s is a list control, not a text editor", type => {
  const action = nativeAction(selection(["visible"]), "INPUT", "", type);
  expect(action.select).not.toHaveBeenCalled();
  expect(action.execCommand).not.toHaveBeenCalled();
  expect(observed.updates.at(-1).keys).toEqual(["visible"]);
});
test("empty lists consume Command+A without selecting page text", () => {
  expect(nativeAction(selection([])).execCommand).not.toHaveBeenCalled();
  const event = { key: "a", metaKey: true, target: null, preventDefault: vi.fn(), stopPropagation: vi.fn() };
  selection([]).onListKeyDown(event as any);
  expect(event.preventDefault).toHaveBeenCalledOnce();
  expect(observed.updates).toEqual([]);
});
test("static page focus and busy selection do not select global text", () => {
  expect(nativeAction(selection(["underlying"]), "DIV", "", "", false).execCommand).not.toHaveBeenCalled();
  observed.busy = true;
  expect(nativeAction(selection(["underlying"])).execCommand).not.toHaveBeenCalled();
  expect(observed.updates).toEqual([]);
});

test("click, toolbar and native select-all share exclusive selection entry", () => {
  const clearOther = vi.fn();
  const batch = selection(["visible"], clearOther);
  batch.select("visible", { detail: 1, metaKey: true, ctrlKey: false, shiftKey: false }, () => {});
  batch.selectAll();
  nativeAction(batch);
  expect(clearOther).toHaveBeenCalledTimes(3);
});
test("closed archive drawer consumes select-all without changing either selection", () => {
  const clearOther = vi.fn();
  const event = { key: "a", metaKey: true, target: null, preventDefault: vi.fn(), stopPropagation: vi.fn() };
  selection(["cached-archived"], clearOther).onListKeyDown(event as any, false);
  expect(event.preventDefault).toHaveBeenCalledOnce();
  expect(event.stopPropagation).toHaveBeenCalledOnce();
  expect(clearOther).not.toHaveBeenCalled();
  expect(observed.updates).toEqual([]);
});

function keyEvent(key: string) {
  return { key, target: null, preventDefault: vi.fn(), stopPropagation: vi.fn() };
}
test("drawer Escape and unhandled keys bubble when there is no local selection", () => {
  for (const [key, open] of [["Escape", false], ["Escape", true], ["ArrowDown", true]] as const) {
    const event = keyEvent(key);
    selection(["cached-archived"]).onListKeyDown(event as any, open);
    expect(event.preventDefault).not.toHaveBeenCalled();
    expect(event.stopPropagation).not.toHaveBeenCalled();
  }
});
test("handled Escape clears stale feedback and consumes only its own selection", () => {
  observed.states[0] = { scope: "1\0local\0codex\0filtered", selecting: true, keys: ["visible"], anchor: "visible", notice: null };
  const event = keyEvent("Escape");
  selection(["visible"]).onListKeyDown(event as any);
  expect(event.stopPropagation).toHaveBeenCalledOnce();
  expect(observed.updates.at(-1).keys).toEqual([]);
  expect(observed.resets.every(reset => reset.mock.calls.length === 1)).toBe(true);
  expect(observed.updates.filter(value => value === null)).toHaveLength(2);
});
test("clearing a busy drawer preserves execution and completion invalidation", async () => {
  observed.busy = true;
  const onSucceeded = vi.fn();
  const batch = selection(["visible"], undefined, onSucceeded);
  batch.clear();
  expect(batch.busy).toBe(true);
  expect(observed.resets[0]).toHaveBeenCalledOnce();
  expect(observed.resets[1]).not.toHaveBeenCalled();
  observed.mutations[1].onSuccess({ generation: 0, operation: "delete", response: { items: [{ sessionKey: "visible", status: "succeeded" }] } });
  expect(onSucceeded).toHaveBeenCalledWith(["visible"], "delete");
  expect(observed.invalidate).toHaveBeenCalledOnce();
});

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>(done => { resolve = done; });
  return { promise, resolve };
}
test.each([false, true])("execution keeps its fixed pages across filtering, machine changed=%s", async changedMachine => {
  observed.lifecycle = true;
  const keys = Array.from({ length: 201 }, (_, i) => `key-${i}`);
  const done = vi.fn(); selection(keys, undefined, done);
  observed.states[1] = { provider: "codex", operation: "archive", items: keys.map(sessionKey => ({ sessionKey, allowed: true, fingerprint: "fixture" })) };
  selection(keys, undefined, done);
  const execution = observed.mutations[1];
  const first = deferred<any>();
  vi.mocked(executeSessionBatch).mockImplementationOnce(() => first.promise).mockImplementation(async request => ({ items: request.items.map(i => ({ sessionKey: i.sessionKey, status: "succeeded" })) }));
  const work = execution.mutationFn();
  observed.pending[1] = true; observed.filter = "new filter";
  if (changedMachine) observed.revision += 1;
  const next = selection(keys.slice(0, 2), undefined, done);
  if (!changedMachine) { expect(next.busy).toBe(true); expect(observed.resets[1]).not.toHaveBeenCalled(); }
  next.clear();
  first.resolve({ items: keys.slice(0, 100).map(sessionKey => ({ sessionKey, status: "succeeded" })) });
  if (changedMachine) {
    await expect(work).rejects.toThrow("连接范围已变化");
    execution.onSuccess({ generation: 0, operation: "archive", response: { items: [] } });
    expect(done).not.toHaveBeenCalled(); expect(observed.invalidate).not.toHaveBeenCalled();
  } else {
    execution.onSuccess(await work);
    expect(vi.mocked(executeSessionBatch).mock.calls.map(([request]) => request.items.length)).toEqual([100,100,1]);
    expect(done).toHaveBeenCalledWith(keys, "archive");
    expect(observed.states[2].items).toHaveLength(201);
  }
});
test("cancel detaches a pending preview immediately and ignores its late response", async () => {
  observed.lifecycle = true; selection(["key"]);
  const page = deferred<any>(); vi.mocked(previewSessionBatch).mockReturnValue(page.promise);
  const prepare = observed.mutations[0]; const work = prepare.mutationFn({ operation: "archive", keys: Array.from({length:101}, (_,i)=>`key-${i}`) });
  observed.pending[0] = true; const batch = selection(["key"]); batch.cancel();
  expect(observed.resets[0]).toHaveBeenCalledOnce();
  page.resolve({ provider: "codex", operation: "archive", items: [] });
  await expect(work).rejects.toThrow("选择范围已变化");
  expect(previewSessionBatch).toHaveBeenCalledOnce();
  prepare.onSuccess({ generation:0, response:{items:[{sessionKey:"old"}]} });
  expect(observed.states[1]).toBeNull();
});
test("single preview resets previous execution errors and cannot start while busy", () => {
  const batch = selection(["key"]); batch.prepareSingle("key", "delete", {} as HTMLElement);
  expect(observed.resets[1]).toHaveBeenCalledOnce();
  observed.busy = true; const busy = selection(["key"]);
  busy.prepareSingle("key", "delete", {} as HTMLElement);
  expect(observed.resets.at(-1)).not.toHaveBeenCalled();
});

test.each([0, 1])("selection clears old mutation %s errors even after selection has closed", mutation => {
  for (const action of ["click", "cmd", "shift", "all"]) {
    observed.mutations = []; observed.resets = []; observed.errors[mutation] = new Error("Old failure");
    const batch = selection(["a", "b"]);
    const open = vi.fn();
    if (action === "all") batch.selectAll();
    else batch.select("b", { detail: 1, metaKey: action === "cmd", shiftKey: action === "shift", ctrlKey: false }, open);
    expect(observed.resets[0]).toHaveBeenCalledOnce();
    expect(observed.resets[1]).toHaveBeenCalledOnce();
    expect(open).toHaveBeenCalledTimes(action === "click" ? 1 : 0);
  }
});

test.each([[150, false], [201, false], [201, true]] as const)("a failed second page keeps all %s results without replay, switched machine=%s", async (count, changedMachine) => {
  observed.lifecycle = true;
  const keys = Array.from({ length: count }, (_, i) => `partial-${i}`);
  const done = vi.fn(); selection(keys, undefined, done);
  observed.states[0] = { ...observed.states[0], selecting: true, keys };
  observed.states[1] = { provider: "codex", operation: "archive", items: keys.map(sessionKey => ({ sessionKey, allowed: true, fingerprint: "fixture" })) };
  selection(keys, undefined, done);
  let reject!: (error: Error) => void;
  const pending = new Promise<never>((_, fail) => { reject = fail; });
  vi.mocked(executeSessionBatch).mockResolvedValueOnce({ items: keys.slice(0, 100).map(sessionKey => ({ sessionKey, status: "succeeded" })) }).mockReturnValueOnce(pending);
  const work = observed.mutations[1].mutationFn();
  await Promise.resolve();
  if (changedMachine) { observed.revision += 1; selection(keys, undefined, done); }
  const assertion = expect(work).rejects.toThrow("Connection lost");
  reject(new Error("Connection lost")); await assertion;
  expect(executeSessionBatch).toHaveBeenCalledTimes(2);
  expect(observed.states[1]).toBeNull();
  if (changedMachine) {
    expect(done).not.toHaveBeenCalled(); expect(observed.invalidate).not.toHaveBeenCalled();
    expect(observed.states[2]).toBeNull();
  } else {
    expect(done).toHaveBeenCalledExactlyOnceWith(keys.slice(0,100), "archive");
    expect(observed.states[0].keys).toEqual(keys.slice(100));
    expect(observed.states[2].items).toHaveLength(count);
    expect(observed.states[2].items.slice(100, 200)).toEqual(keys.slice(100, 200).map(sessionKey => ({ sessionKey, status: "failed", message: "请求中断，结果未确认" })));
    expect(observed.states[2].items.slice(200)).toEqual(keys.slice(200).map(sessionKey => ({ sessionKey, status: "failed", message: "前序请求中断，未执行" })));
    expect(observed.invalidate).toHaveBeenCalledOnce();
  }
});


test("a first-page failure keeps selection without an empty success result", async () => {
  observed.lifecycle = true;
  const keys = ["first", "second"];
  const done = vi.fn(); selection(keys, undefined, done);
  observed.states[0] = { ...observed.states[0], selecting: true, keys };
  observed.states[1] = { provider: "codex", operation: "delete", items: keys.map(sessionKey => ({ sessionKey, allowed: true, descendants: [] })) };
  selection(keys, undefined, done);
  vi.mocked(executeSessionBatch).mockRejectedValueOnce(new Error("Connection lost"));
  await expect(observed.mutations[1].mutationFn()).rejects.toThrow("Connection lost");
  expect(done).not.toHaveBeenCalled();
  expect(observed.states[0].keys).toEqual(keys);
  expect(observed.states[2]).toBeNull();
  expect(observed.invalidate).not.toHaveBeenCalled();
});
