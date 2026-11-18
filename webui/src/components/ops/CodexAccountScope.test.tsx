import { beforeEach, describe, expect, test, vi } from "vitest";
import { isValidElement, type ReactElement, type ReactNode } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import type { RuntimeCapabilityMatrix } from "../../lib/query/system";
import type { CodexAccountScopeResult } from "../../types";
import { CodexAccountScope } from "./CodexAccountScope";

const harness = vi.hoisted(() => ({
  index: 0,
  cells: new Map<number, unknown>(),
  enabled: null as boolean | null,
  scope: null as any
}));

vi.mock("react", async (importOriginal) => {
  const actual = await importOriginal<typeof import("react")>();
  return {
    ...actual,
    useState: (initial: unknown) => {
      const index = harness.index++;
      if (!harness.cells.has(index)) harness.cells.set(index, initial);
      return [harness.cells.get(index), (value: unknown) => harness.cells.set(index, value)];
    }
  };
});

vi.mock("../../lib/query/ops", () => ({
  useCodexAccountScope: (enabled: boolean) => {
    harness.enabled = enabled;
    return harness.scope;
  }
}));

const capabilities: RuntimeCapabilityMatrix = {
  runtimeKind: "desktop",
  hostSurface: "desktop_embedded_tauri",
  codexStatePaths: false,
  codexAccountScope: true,
  updatePrune: false,
  threadCleanup: true,
  threadArchiveActions: true,
  updateServiceLabels: false
};

type Element = ReactElement<Record<string, any>>;

function elements(node: ReactNode): Element[] {
  if (Array.isArray(node)) return node.flatMap(elements);
  if (!isValidElement<Record<string, any>>(node)) return [];
  return [node, ...elements(node.props.children)];
}

function renderTree(input = capabilities): ReactElement | null {
  harness.index = 0;
  return CodexAccountScope({ capabilities: input }) as ReactElement | null;
}

function button(tree: ReactElement, label: string): Element {
  const found = elements(tree).find(
    (element) => element.type === "button" && renderToStaticMarkup(element).includes(`${label}</button>`)
  );
  if (!found) throw new Error(`Button missing: ${label}`);
  return found;
}

function click(tree: ReactElement, label: string) {
  const found = button(tree, label);
  if (found.props.disabled) throw new Error(`Button disabled: ${label}`);
  found.props.onClick();
}

function toggle(tree: ReactElement): Element {
  return elements(tree).find((element) => element.type === "input")!;
}

function mutation(data?: unknown) {
  return { data, error: null, isPending: false, mutate: vi.fn(), reset: vi.fn() };
}

beforeEach(() => {
  harness.cells.clear();
  harness.enabled = null;
  harness.scope = {
    check: mutation(),
    clear: mutation(),
    auto: { hookInstalled: true, enabled: false, pending: false, error: null, set: vi.fn() }
  };
});

describe("Codex account scope card", () => {
  test("stays hidden and idle on services without the capability", () => {
    expect(renderTree({ ...capabilities, codexAccountScope: false })).toBeNull();
    expect(harness.enabled).toBe(false);
    expect(renderTree({ ...capabilities, codexAccountScope: undefined })).toBeNull();
  });

  test("clears only after a check found sessions and one confirmation", () => {
    let tree = renderTree()!;
    expect(harness.enabled).toBe(true);
    expect(renderToStaticMarkup(tree)).toContain("待检查");
    expect(button(tree, "解除").props.disabled).toBe(true);
    click(tree, "检查");
    expect(harness.scope.check.mutate).toHaveBeenCalledOnce();
    expect(harness.scope.clear.reset).toHaveBeenCalledOnce();

    harness.scope.check = mutation({ scopedThreads: 0, scopedFiles: 0 });
    tree = renderTree()!;
    expect(renderToStaticMarkup(tree)).toContain("无隔离会话");
    expect(button(tree, "解除").props.disabled).toBe(true);

    harness.scope.check = mutation({ scopedThreads: 3, scopedFiles: 4 });
    tree = renderTree()!;
    expect(renderToStaticMarkup(tree)).toContain("3 个会话");
    click(tree, "解除");
    expect(harness.scope.clear.mutate).not.toHaveBeenCalled();
    tree = renderTree()!;
    expect(renderToStaticMarkup(tree)).toContain("等待确认");
    click(tree, "取消");
    tree = renderTree()!;
    click(tree, "解除");
    tree = renderTree()!;
    click(tree, "确认解除");
    expect(harness.scope.clear.mutate).toHaveBeenCalledOnce();
    tree = renderTree()!;
    expect(() => button(tree, "确认解除")).toThrow();
  });

  test("shows cleared sessions, leftovers and each unfinished reason", () => {
    const result: CodexAccountScopeResult = {
      clearedThreads: 2,
      clearedFiles: 3,
      remainingFiles: 5,
      items: [
        { threadId: "thread-busy", reason: "busy" },
        { threadId: "thread-failed", reason: "failed", detail: "fixture failure" },
        ...Array.from({ length: 120 }, (_, index) => ({ threadId: `thread-${index}`, reason: "format" as const }))
      ]
    };
    harness.scope.clear = mutation(result);
    const html = renderToStaticMarkup(renderTree()!);
    expect(html).toContain("已解除 <strong>2</strong> 个会话");
    expect(html).toContain("剩余 5 个文件");
    expect(html).toContain("thread-busy：Codex 使用中");
    expect(html).toContain("thread-failed：失败（fixture failure）");
    expect(html.match(/cleanup-result-item/g)).toHaveLength(100);
    expect(html).toContain("1 / 2");

    harness.scope.clear = { ...mutation(), error: new Error("Codex 正在维护会话文件，稍后重试") };
    expect(renderToStaticMarkup(renderTree()!)).toContain('role="alert"');
  });

  test("the automatic switch saves on change and is locked without the Codex hook", () => {
    let tree = renderTree()!;
    expect(toggle(tree).props).toMatchObject({ checked: false, disabled: false });
    toggle(tree).props.onChange({ target: { checked: true } });
    expect(harness.scope.auto.set).toHaveBeenCalledExactlyOnceWith(true);
    expect(renderToStaticMarkup(tree)).not.toContain("未安装 Codex Hook");

    harness.scope.auto = { ...harness.scope.auto, hookInstalled: false, enabled: true };
    tree = renderTree()!;
    expect(toggle(tree).props).toMatchObject({ checked: true, disabled: true });
    expect(renderToStaticMarkup(tree)).toContain("未安装 Codex Hook");
  });
});
