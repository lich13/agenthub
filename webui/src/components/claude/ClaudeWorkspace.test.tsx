import { isValidElement, type ReactElement, type ReactNode } from "react";
import { expect, test, vi } from "vitest";
import type { ClaudeDeletePreview, ClaudeSessionSummary } from "../../types";
import { RenameableSession } from "../common/RenameableSession";
import { SessionDeleteConfirmDialog } from "../common/WorkspaceSplitShell";
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
vi.mock("../../lib/query/connection", () => ({ useConnection: () => ({ target: harness.target }) }));
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
function render() {
  harness.index = 0;
  return elements(ClaudeWorkspace());
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
