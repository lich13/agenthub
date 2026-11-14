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
function nativeAction(batch: ReturnType<typeof useSessionSelection>, tagName = "BUTTON", dialog = false) {
  const execCommand = vi.fn();
  const target = {
    tagName, isContentEditable: tagName === "EDITOR", closest: () => dialog ? {} : null,
    dispatchEvent(event: any) { event.target = target; batch.onListKeyDown(event); return !event.defaultPrevented; },
  };
  class KeyEvent {
    defaultPrevented = false;
    constructor(_type: string, options: object) { Object.assign(this, options); }
    preventDefault() { this.defaultPrevented = true; }
    stopPropagation() {}
  }
  runInNewContext(nativeSelectAll, { document: { activeElement: target, execCommand }, KeyboardEvent: KeyEvent });
  return execCommand;
}
beforeEach(() => { observed.updates = []; observed.busy = false; });
test("native Edit Select All reaches the same selection action for only loaded filtered keys", () => {
  expect(nativeAction(selection(["visible-a", "visible-c"]))).not.toHaveBeenCalled();
  expect(observed.updates.at(-1).keys).toEqual(["visible-a", "visible-c"]);
});
test.each(["INPUT", "TEXTAREA", "EDITOR"])("native Select All keeps text selection in %s", tag => {
  expect(nativeAction(selection(["underlying"]), tag)).toHaveBeenCalledWith("selectAll");
  expect(observed.updates).toEqual([]);
});
test("dialog focus cannot select the underlying list", () => {
  expect(nativeAction(selection(["underlying"]), "BUTTON", true)).toHaveBeenCalledWith("selectAll");
  expect(observed.updates).toEqual([]);
});
test("busy selection remains unchanged", () => {
  observed.busy = true;
  nativeAction(selection(["underlying"]));
  expect(observed.updates).toEqual([]);
});
