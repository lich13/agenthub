import { createElement } from "react";
import { renderToString } from "react-dom/server";
import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";
import { beforeEach, expect, test, vi } from "vitest";
import { useSessionSelection } from "./sessions";

const observed = vi.hoisted(() => ({
  updates: [] as any[], busy: false, states: [] as any[], stateIndex: 0,
  mutations: [] as any[], resets: [] as ReturnType<typeof vi.fn>[],
  invalidate: vi.fn(),
}));
vi.mock("react", async original => ({
  ...await original<typeof import("react")>(),
  useState: (initial: any) => {
    const index = observed.stateIndex++;
    const value = index in observed.states ? observed.states[index] : typeof initial === "function" ? initial() : initial;
    return [value, (next: any) => observed.updates.push(next)];
  },
}));
vi.mock("@tanstack/react-query", () => ({
  useQueryClient: () => ({ invalidateQueries: observed.invalidate }),
  useMutation: (options: any) => {
    observed.mutations.push(options);
    const reset = vi.fn(); observed.resets.push(reset);
    return { isPending: observed.busy, reset, mutate: vi.fn() };
  },
}));
vi.mock("./connection", () => ({ useConnection: () => ({ revision: 1, target: "local" }) }));

const nativeSelectAll = readFileSync(new URL("../../../../src-tauri/src/select_all.js", import.meta.url), "utf8");
function selection(keys: string[], onSelectionStart?: () => void, onSucceeded?: () => void) {
  observed.stateIndex = 0;
  let result!: ReturnType<typeof useSessionSelection>;
  function Probe() { result = useSessionSelection("codex", "filtered", keys, onSucceeded, undefined, onSelectionStart); return null; }
  renderToString(createElement(Probe));
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
  expect(observed.resets.every(reset => reset.mock.calls.length === 0)).toBe(true);
  observed.mutations[1].onSuccess({ generation: 0, operation: "delete", response: { items: [{ sessionKey: "visible", status: "succeeded" }] } });
  expect(onSucceeded).toHaveBeenCalledWith(["visible"], "delete");
  expect(observed.invalidate).toHaveBeenCalledOnce();
});
