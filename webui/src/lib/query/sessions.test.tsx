import { createElement } from "react";
import { renderToString } from "react-dom/server";
import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";
import { beforeEach, expect, test, vi } from "vitest";
import { useSessionSelection } from "./sessions";

const observed = vi.hoisted(() => ({ updates: [] as any[], busy: false }));
vi.mock("react", async original => ({
  ...await original<typeof import("react")>(),
  useState: (initial: any) => [typeof initial === "function" ? initial() : initial, (next: any) => observed.updates.push(next)],
}));
vi.mock("@tanstack/react-query", () => ({
  useQueryClient: () => ({}),
  useMutation: () => ({ isPending: observed.busy, reset: vi.fn() }),
}));
vi.mock("./connection", () => ({ useConnection: () => ({ revision: 1, target: "local" }) }));

const nativeSelectAll = readFileSync(new URL("../../../../src-tauri/src/select_all.js", import.meta.url), "utf8");
function selection(keys: string[]) {
  let result!: ReturnType<typeof useSessionSelection>;
  function Probe() { result = useSessionSelection("codex", "filtered", keys); return null; }
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
beforeEach(() => { observed.updates = []; observed.busy = false; });
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
