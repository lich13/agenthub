import { renderToStaticMarkup } from "react-dom/server";
import { expect, test } from "vitest";
import { SessionBatchControls } from "./SessionBatchControls";
import type { SessionSelection } from "../../lib/query/sessions";

test.each([false, true])("cancel remains available during preview but not execution: %s", executing => {
  const batch = {
    returnFocus: { current: null }, selection: ["fixture"], selecting: true,
    preview: null, notice: null, result: null, busy: true,
    prepare: { isPending: !executing, error: null }, execute: { isPending: executing, error: null },
  } as unknown as SessionSelection;
  const html = renderToStaticMarkup(<SessionBatchControls batch={batch} operations={["delete"]} />);
  const cancel = html.match(/<button[^>]*aria-label="取消多选"[^>]*>/)?.[0];
  expect(cancel).toBeDefined();
  expect(cancel!.includes("disabled")).toBe(executing);
});

test("collapsed archive controls keep execution errors and residuals visible", () => {
  const batch = {
    returnFocus: { current: null }, selection: ["fixture"], selecting: true,
    preview: null, notice: null, busy: false, prepare: { error: null },
    execute: { error: new Error("fixture transport failure") },
    result: { items: [{ sessionKey: "fixture", status: "failed", message: "fixture deletion failure", residuals: ["fixture residual"] }] },
  } as unknown as SessionSelection;
  const html = renderToStaticMarkup(<SessionBatchControls batch={batch} operations={["restore", "delete"]} showSelection={false} />);
  expect(html).toContain("fixture transport failure");
  expect(html).toContain("fixture deletion failure");
  expect(html).toContain("fixture residual");
  expect(html).not.toContain("已选会话操作");
});
