import { isValidElement, type ReactElement, type ReactNode } from "react";
import { expect, test, vi } from "vitest";
import type { ClaudeDeletePreview, ClaudeSessionSummary } from "../../types";
import { RenameableSession } from "../common/RenameableSession";
import { SessionDeleteConfirmDialog } from "../common/WorkspaceSplitShell";
import { ClaudeSubagentPanel, ClaudeSubagentHistory } from "./ClaudeSubagentPanel";
import { desktopBootstrapCapabilities } from "../../lib/domain/capabilities";
import { ClaudeWorkspace } from "./ClaudeWorkspace";

const harness = vi.hoisted(() => ({
  index: 0,
  cells: new Map<number, unknown>(),
  sessions: [] as ClaudeSessionSummary[],
  target: "local",
  preview: vi.fn(),
  remove: vi.fn(),
}));

vi.mock("react", async original => ({
  ...await original<typeof import("react")>(),
  useEffect: vi.fn(),
  useLayoutEffect: vi.fn(),
  useRef: (current: unknown) => ({ current }),
  useMemo: (compute: () => unknown) => compute(),
  useCallback: (callback: unknown) => callback,
  useState: (initial: unknown) => {
    const index = harness.index++;
    if (!harness.cells.has(index)) harness.cells.set(index, initial);
    return [harness.cells.get(index), (value: unknown) => harness.cells.set(index,
      typeof value === "function" ? value(harness.cells.get(index)) : value)];
  },
}));
vi.mock("../../hooks/useCopyFeedback", () => ({
  useCopyFeedback: () => ({ feedback: "", setFeedback: vi.fn(), copy: vi.fn() }),
}));
vi.mock("../../lib/query/connection", () => ({ useConnection: () => ({ target: harness.target }), machineScope: () => `${harness.target}:0` }));
vi.mock("../../lib/query/claude", () => ({
  useClaudeSessions: () => ({ data: harness.sessions }),
  useClaudeDetail: () => ({}),
  useClaudeActions: () => ({
    rename: {}, diagnostics: {},
    preview: { mutate: harness.preview },
    remove: { reset: vi.fn(), mutate: harness.remove },
  }),
}));
vi.mock("../../lib/query/sessions", () => ({
  useSessionSelection: () => ({ selection: [], busy: false, selecting: false }),
}));
vi.mock("../../lib/useProviderWorkspace", () => ({
  useNarrowViewport: () => false,
  useProviderTimelineController: () => ({ events: [], navigation: {}, history: {} }),
  groupedTimelineEntries: () => [],
}));

type Element = ReactElement<Record<string, any>>;
function elements(node: ReactNode): Element[] {
  if (Array.isArray(node)) return node.flatMap(elements);
  if (!isValidElement<Record<string, any>>(node)) return [];
  return [node, ...elements(node.props.children), ...elements(node.props.sidebar)];
}
function render(supported = false) {
  harness.index = 0;
  return elements(ClaudeWorkspace({ capabilities: { ...desktopBootstrapCapabilities, claudeSubagents: supported } }));
}

test.each(["local", "remote"])("%s list delete confirms the clicked thread through ordinary deletion", target => {
  harness.target = target;
  harness.cells.clear();
  harness.preview.mockReset();
  harness.remove.mockReset();
  harness.sessions = ["selected", "clicked"].map(id => ({
    id, sessionKey: `${id}-key`, title: id, path: `/fixture/${id}.jsonl`, cwd: "/fixture",
    messageCount: 0, status: "recent", canRename: true, canDelete: true,
  }));
  harness.preview.mockImplementation((sessionKey: string, options: { onSuccess: (value: ClaudeDeletePreview) => void }) => {
    options.onSuccess({ sessionKey, title: "clicked", path: "/fixture/clicked.jsonl", bytes: 0, fileCount: 1 });
  });

  const rows = render().filter(node => node.type === RenameableSession);
  const deletion = rows[1].props.menuActions.find((action: { id: string }) => action.id === "delete");
  expect(deletion.disabled).toBeFalsy();
  deletion.run();
  expect(harness.preview).toHaveBeenCalledWith("clicked-key", expect.any(Object));
  expect(harness.remove).not.toHaveBeenCalled();
  const confirmation = render().find(node => node.type === SessionDeleteConfirmDialog);
  expect(confirmation).toBeDefined();
  confirmation!.props.onConfirm();
  expect(harness.remove).toHaveBeenCalledWith(
    { sessionKey: "clicked-key", confirmed: true }, expect.any(Object),
  );
});


test("Claude subagent entry requires its own capability and closes across machines or parents", () => {
  harness.target = "local";
  harness.cells.clear();
  harness.sessions = [{ id: "parent", sessionKey: "parent-key", title: "Parent", cwd: "/fixture", path: "/fixture/parent.jsonl", messageCount: 1, status: "recent", canRename: true, canDelete: true }];
  const entry = (tree: Element[]) => tree.find(node => node.props["aria-label"] === "本会话子智能体");
  expect(entry(render())).toBeUndefined();
  entry(render(true))!.props.onClick({ currentTarget: { isConnected: false } });
  expect(render(true).find(node => node.type === ClaudeSubagentPanel)?.props.parentKey).toBe("parent-key");
  harness.target = "remote";
  expect(render(true).find(node => node.type === ClaudeSubagentPanel)).toBeUndefined();
  harness.sessions = [{ ...harness.sessions[0], sessionKey: "other-parent" }];
  expect(render(true).find(node => node.type === ClaudeSubagentPanel)).toBeUndefined();
});

test("the flat Claude list opens child history and returns without management actions", () => {
  harness.cells.clear();
  const agent = { agentId: "fixture-child", name: "Review fixture", status: "unknown" as const, available: true };
  const props = { parentKey: "parent-key", collection: { agents: [agent], counts: { creating: 0, running: 0, completed: 0, failed: 0, interrupted: 0, unknown: 1 }, complete: true }, docked: true, onClose: vi.fn() };
  const panel = () => { harness.index = 0; return elements(ClaudeSubagentPanel(props)); };
  panel().find(node => node.props["data-agent-id"] === "fixture-child")!.props.onClick();
  const opened = panel();
  expect(opened.find(node => node.type === ClaudeSubagentHistory)?.props).toMatchObject({ parentKey: "parent-key", agent });
  expect(opened.some(node => node.props["aria-label"]?.match(/改名|删除|发送|审批|接管|附件/))).toBe(false);
  opened.find(node => node.props["aria-label"] === "返回子智能体列表")!.props.onClick();
  expect(panel().find(node => node.type === ClaudeSubagentHistory)).toBeUndefined();
  expect(panel().find(node => node.props["data-agent-id"] === "fixture-child")).toBeDefined();
});
