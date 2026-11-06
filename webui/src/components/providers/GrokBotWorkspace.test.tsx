import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderToStaticMarkup } from "react-dom/server";
import { beforeEach, expect, test, vi } from "vitest";
import type { ProviderSessionSummary } from "../../types";
import type { ThreadMenuAction } from "../common/TaskMenu";
import { GrokBotWorkspace } from "./GrokBotWorkspace";

const state = vi.hoisted(() => ({
  summary: {} as ProviderSessionSummary, actions: [] as ThreadMenuAction[], rename: undefined as unknown,
  read: vi.fn(), copy: vi.fn(), feedback: vi.fn(), refetch: vi.fn(),
}));
vi.mock("../../hooks/useCopyFeedback", () => ({ useCopyFeedback: () => ({ copy: state.copy, setFeedback: state.feedback, feedback: "" }) }));
vi.mock("../../lib/api/filePaths", () => ({ canRevealLocalPath: () => true, revealLocalPath: vi.fn() }));
vi.mock("../../lib/query/providerSessions", () => ({
  useProviderSessions: () => ({ data: [state.summary], refetch: state.refetch }),
  useProviderDetail: () => ({ data: { summary: state.summary, events: [] }, refetch: state.refetch }),
  readArchivableProviderSession: state.read,
}));
vi.mock("../../lib/useProviderWorkspace", async original => ({
  ...await original<typeof import("../../lib/useProviderWorkspace")>(),
  useNarrowViewport: () => false,
  useProviderTimelineController: () => ({ events: [], history: {}, navigation: {}, handleTimelineSelect: vi.fn() }),
}));
vi.mock("../common/RenameableSession", () => ({ RenameableSession: (props: { onRename?: unknown; children: React.ReactNode }) => { state.rename = props.onRename; return <div>{props.children}</div>; } }));
vi.mock("../common/TaskMenu", async original => {
  const actual = await original<typeof import("../common/TaskMenu")>();
  return { ...actual, TaskMenu: ({ children }: { children: React.ReactNode }) => <div>{children}</div>, ThreadMenuItems: (props: { actions: ThreadMenuAction[] }) => { state.actions = props.actions; return <actual.ThreadMenuItems {...props} />; } };
});
function render() { return renderToStaticMarkup(<QueryClientProvider client={new QueryClient()}><GrokBotWorkspace /></QueryClientProvider>); }
beforeEach(() => {
  vi.clearAllMocks();
  state.summary = { id: "fixture", sessionKey: "account:fixture", title: "Fixture", cwd: "", path: "/fixture/thread.blob", status: "unknown", formatVersion: "近期缓存", archived: false, canRename: false, canArchive: false, canDelete: false, messageCount: 0 };
});
test("uncached threads remain readable list items with no mutation controls", () => {
  state.summary.path = "";
  const markup = render();
  expect(markup).toContain("未缓存");
  expect(state.rename).toBeUndefined();
  expect(state.actions.map(action => action.id)).toEqual(["copy-reply", "copy-id", "copy-path", "reveal"]);
  expect(state.actions.find(action => action.id === "copy-path")?.disabled).toBe(true);
});
test("copy path uses the selected native path directly", () => {
  render();
  state.actions.find(action => action.id === "copy-path")!.run();
  expect(state.copy).toHaveBeenCalledWith("/fixture/thread.blob", "已复制路径");
  expect(state.read).not.toHaveBeenCalled();
});
test("an uncached pending request keeps its actionable status", () => {
  state.summary.path = "";
  state.summary.status = "reply_needed";
  expect(render()).toContain("需要回复");
});
