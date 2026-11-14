import { beforeEach, describe, expect, test, vi } from "vitest";
import { isValidElement, type ReactElement, type ReactNode } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import type { UnifiedCleanupPlan, UnifiedCleanupResult } from "../../types";
import type { RuntimeCapabilityMatrix } from "../../lib/query/system";
import { CleanupPagination, UnifiedThreadCleanup } from "./UnifiedThreadCleanup";

const harness = vi.hoisted(() => ({
  index: 0,
  cells: new Map<number, unknown>(),
  unifiedInput: null as any,
  unifiedActions: null as any
}));

vi.mock("react", async (importOriginal) => {
  const actual = await importOriginal<typeof import("react")>();
  return {
    ...actual,
    useState: (initial: unknown) => {
      const index = harness.index++;
      if (!harness.cells.has(index)) {
        harness.cells.set(
          index,
          typeof initial === "function" ? (initial as () => unknown)() : initial
        );
      }
      return [
        harness.cells.get(index),
        (value: unknown) => {
          const current = harness.cells.get(index);
          harness.cells.set(
            index,
            typeof value === "function"
              ? (value as (previous: unknown) => unknown)(current)
              : value
          );
        }
      ];
    }
  };
});

vi.mock("../../lib/query/ops", () => ({
  useUnifiedCleanupActions: (input: unknown) => {
    harness.unifiedInput = input;
    return harness.unifiedActions;
  }
}));

const capabilities: RuntimeCapabilityMatrix = {
  runtimeKind: "desktop",
  hostSurface: "desktop_embedded_tauri",
  codexStatePaths: false,
  hiddenCleanupScan: true,
  updatePrune: false,
  threadCleanup: true,
  threadArchiveActions: true,
  updateServiceLabels: false
};

function mutation() {
  return {
    data: undefined,
    error: null,
    isPending: false,
    isSuccess: false,
    variables: undefined,
    mutate: vi.fn(),
    reset: vi.fn()
  };
}

function unifiedPlan(count: number): UnifiedCleanupPlan {
  const candidates = Array.from({ length: count }, (_, index) => {
    const id = "candidate-" + String(index).padStart(3, "0");
    return { id, title: id, estimatedBytes: 1024, fingerprint: `fp-${id}`, blockedReason: null };
  });
  return {
    totalCandidates: count + 1,
    totalDeletable: count + 1,
    totalEstimatedBytes: (count + 1) * 1024,
    scopes: [
      {
        scope: "codex_hidden",
        provider: "codex",
        label: "Codex 隐藏线程",
        supported: true,
        scanned: true,
        unavailableReason: null,
        candidateCount: count,
        deletableCount: count,
        estimatedBytes: count * 1024,
        candidates,
        issues: []
      },
      {
        scope: "antigravity_archived",
        provider: "antigravity",
        label: "Antigravity 已归档线程",
        supported: true,
        scanned: true,
        unavailableReason: null,
        candidateCount: 1,
        deletableCount: 1,
        estimatedBytes: 1024,
        candidates: [{ id: "ag:demo", title: "AG 归档", estimatedBytes: 1024, fingerprint: "fp-ag-demo", blockedReason: null }],
        issues: []
      }
    ]
  };
}

function failedUnifiedResult(count: number): UnifiedCleanupResult {
  const issues = Array.from(
    { length: count },
    (_, index) => `candidate-${String(index).padStart(3, "0")}：fixture failure`
  );
  return {
    deletedCount: 1,
    skippedCount: 0,
    failedCount: count,
    deletedBytes: 1024,
    stoppedEarly: true,
    scopes: [
      {
        scope: "codex_hidden",
        provider: "codex",
        label: "Codex 隐藏线程",
        deletedCount: 1,
        skippedCount: 0,
        failedCount: count,
        deletedBytes: 1024,
        residuals: ["/isolated/residual-rollout.jsonl"],
        issues
      }
    ]
  };
}

type Element = ReactElement<Record<string, any>>;

function elements(node: ReactNode): Element[] {
  if (Array.isArray(node)) return node.flatMap(elements);
  if (!isValidElement<Record<string, any>>(node)) return [];
  return [node, ...elements(node.props.children)];
}

function renderTree(): ReactElement {
  harness.index = 0;
  return UnifiedThreadCleanup({ capabilities }) as unknown as ReactElement;
}

function markup(tree: ReactElement): string {
  return renderToStaticMarkup(tree);
}

function pagination(tree: ReactElement, label: string): Element {
  const found = elements(tree).find(
    (element) => element.type === CleanupPagination && element.props.label === label
  );
  if (!found) throw new Error("Expected " + label + " pagination");
  return found;
}

function page(paginationElement: Element, index: number) {
  (paginationElement.props.onChange as (page: number) => void)(index);
}

function clickButton(tree: ReactElement, label: string) {
  const found = elements(tree).find(
    (element) => element.type === "button" && renderToStaticMarkup(element).includes(label)
  );
  if (!found || found.props.disabled) throw new Error(`Button unavailable: ${label}`);
  found.props.onClick();
}

function createActions() {
  return {
    unifiedDryRun: mutation(),
    hiddenDryRun: mutation(),
    unifiedExecute: mutation(),
    stopHiddenScan: vi.fn()
  };
}

beforeEach(() => {
  harness.index = 0;
  harness.cells.clear();
  harness.unifiedInput = null;
  harness.unifiedActions = createActions();
});

describe("UnifiedThreadCleanup workbench", () => {
  test("renders no more than 100 candidates on each page", () => {
    const plan = unifiedPlan(205);
    renderTree();
    harness.unifiedInput.onUnifiedDryRun(plan);

    let tree = renderTree();
    let html = markup(tree);
    expect(html.match(/<li\b/g) ?? []).toHaveLength(100);
    expect(html).toContain("candidate-000");
    expect(html).toContain("candidate-099");
    expect(html).not.toContain("candidate-100");

    page(pagination(tree, "候选"), 1);
    tree = renderTree();
    html = markup(tree);
    expect(html.match(/<li\b/g) ?? []).toHaveLength(100);
    expect(html).toContain("candidate-100");
    expect(html).toContain("candidate-199");
    expect(html).not.toContain("candidate-200");

    page(pagination(tree, "候选"), 2);
    html = markup(renderTree());
    expect(html.match(/<li\b/g) ?? []).toHaveLength(5);
    expect(html).toContain("candidate-204");
  });

  test("renders no more than 100 unsuccessful delete results on each page and separates reclaimed bytes", () => {
    const plan = unifiedPlan(205);
    renderTree();
    harness.unifiedInput.onUnifiedDryRun(plan);
    renderTree();
    harness.unifiedInput.onUnifiedExecute(failedUnifiedResult(205));

    let tree = renderTree();
    let html = markup(tree);
    expect(html).toContain("已删除 <strong>1</strong> 项");
    expect(html).toContain("实际回收 <strong>1.0 KiB</strong>");
    expect(html).toContain("/isolated/residual-rollout.jsonl");
    expect(html.match(/cleanup-result-item/g) ?? []).toHaveLength(100);
    expect(html).toContain("candidate-099：fixture failure");
    expect(html).not.toContain("candidate-100：fixture failure");

    page(pagination(tree, "清理结果"), 1);
    tree = renderTree();
    html = markup(tree);
    expect(html.match(/cleanup-result-item/g) ?? []).toHaveLength(100);
    expect(html).toContain("candidate-100：fixture failure");
    expect(html).toContain("candidate-199：fixture failure");
    expect(html).not.toContain("candidate-200：fixture failure");

    page(pagination(tree, "清理结果"), 2);
    html = markup(renderTree());
    expect(html.match(/cleanup-result-item/g) ?? []).toHaveLength(5);
    expect(html).toContain("candidate-204：fixture failure");
  });

  test("keeps failures visible when independent targets completed without stopping", () => {
    renderTree();
    harness.unifiedInput.onUnifiedExecute({ ...failedUnifiedResult(1), stoppedEarly: false });
    const html = markup(renderTree());
    expect(html).toContain("已删除 <strong>1</strong> 项");
    expect(html).toContain("candidate-000：fixture failure");
    expect(html).toContain("/isolated/residual-rollout.jsonl");
    expect(html).not.toContain("已停止后续删除");
  });

  test("auto-selects cleanable scopes, supports scope deselection, and requires two-step confirmation", () => {
    const plan = unifiedPlan(3);
    renderTree();
    harness.unifiedInput.onUnifiedDryRun(plan);

    let tree = renderTree();
    const agCheckbox = elements(tree).find(
      (element) =>
        element.type === "input" &&
        element.props["aria-label"] === "选择 Antigravity 已归档线程"
    );
    expect(agCheckbox?.props.checked).toBe(true);
    agCheckbox!.props.onChange();

    tree = renderTree();
    clickButton(tree, "一键清理所选平台");
    expect(harness.unifiedActions.unifiedExecute.mutate).not.toHaveBeenCalled();

    tree = renderTree();
    clickButton(tree, "确认清理所选");
    expect(harness.unifiedActions.unifiedExecute.mutate).toHaveBeenCalledWith({
      scopes: ["codex_hidden"],
      targets: [
        { scope: "codex_hidden", id: "candidate-000", fingerprint: "fp-candidate-000" },
        { scope: "codex_hidden", id: "candidate-001", fingerprint: "fp-candidate-001" },
        { scope: "codex_hidden", id: "candidate-002", fingerprint: "fp-candidate-002" }
      ]
    });
  });
});
