import { Children, isValidElement, type ComponentProps, type MouseEvent, type ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { expect, test, vi } from "vitest";
import type { ClaudeHistoryEvent } from "../../types";
import { groupClaudeProcesses } from "../../lib/domain/claudeProcessGroups";
import { DisclosureRevealContext, DisclosureScope } from "../common/ActivityDetails";
import { renderClaudeEvents } from "./ClaudeWorkspace";

const observed = vi.hoisted(() => ({ elements: [] as ReactElement<ComponentProps<"details">>[], machineScope: "local" }));
vi.mock("../../lib/query/connection", async importOriginal => ({ ...await importOriginal<typeof import("../../lib/query/connection")>(), machineScope: () => observed.machineScope }));
vi.mock("../common/ActivityDetails", async importOriginal => {
  const actual = await importOriginal<typeof import("../common/ActivityDetails")>();
  return { ...actual, ActivityDetails: (props: ComponentProps<typeof actual.ActivityDetails>) => {
    const element = actual.ActivityDetails(props);
    observed.elements.push(element);
    return element;
  } };
});

const event = (id: string, kind: string, fields: Partial<ClaudeHistoryEvent> = {}): ClaudeHistoryEvent => ({ id, kind, turnId: "native-turn", ...fields });
const fixture = () => [
  event("native-turn", "user_message", { text: "Visible request" }),
  event("think", "thinking", { text: "Private process fixture" }),
  event("explain", "assistant_message", { text: "Temporary explanation", assistantPhase: "intermediate" }),
  event("tool", "tool_call", { callId: "call", role: "Read", status: "completed", detail: "Synthetic tool input" }),
  event("result", "tool_result", { callId: "call", status: "completed", result: "Synthetic tool result" }),
  event("final", "assistant_message", { text: "Visible final answer", assistantPhase: "final" })
];
function render(events: ClaudeHistoryEvent[], session: string, reveal: string | null = null) {
  observed.elements = [];
  const html = renderToStaticMarkup(<DisclosureScope.Provider value={`claude:${session}`}><DisclosureRevealContext.Provider value={reveal}>
    {renderClaudeEvents(groupClaudeProcesses(events), "Fixture title")}
  </DisclosureRevealContext.Provider></DisclosureScope.Provider>);
  return { html, details: [...observed.elements] };
}
function toggle(details: ReactElement<ComponentProps<"details">>) {
  const summary = Children.toArray(details.props.children).find(child => isValidElement(child) && child.type === "summary");
  if (!isValidElement<ComponentProps<"summary">>(summary)) throw new Error("Summary missing");
  summary.props.onClick!({ target: { closest: () => null }, preventDefault: vi.fn() } as unknown as MouseEvent<HTMLElement>);
}

test("the process defaults closed with no mounted body while final answer and copy action stay visible", () => {
  const { html, details } = render(fixture(), "default");
  expect(details).toHaveLength(2);
  expect(details[0].props.open).toBe(false);
  expect(html).toContain("已思考");
  expect(html).toContain("1 项工具");
  expect(html).not.toContain("Private process fixture");
  expect(html).toContain("Temporary explanation");
  expect(html).not.toContain("Synthetic tool result");
  expect(html).toContain("Visible request");
  expect(html).toContain("Visible final answer");
  expect(html).toContain('aria-label="复制回复"');
  expect(html.match(/user-message-bubble/g)).toHaveLength(1);
});

test.each(["think", "explain", "tool", "result"])("search for %s reveals only the matching process segment", position => {
  const { html, details } = render(fixture(), `search-${position}`, position);
  expect(html).toContain("Temporary explanation");
  if (position === "tool" || position === "result") {
    expect(details[0].props.open).toBe(false);
    expect(details.filter(detail => detail.props.open)).toHaveLength(3);
    expect(html).toContain("Synthetic tool result");
    expect(html).toContain('data-timeline-aliases="tool result"');
  } else if (position === "think") {
    expect(details[0].props.open).toBe(true);
    expect(html).toContain("Private process fixture");
    expect(html).not.toContain("Synthetic tool result");
  } else {
    expect(details.every(detail => !detail.props.open)).toBe(true);
    expect(html).not.toContain("Private process fixture");
  }
});

test("search for a user or final answer does not expand the process", () => {
  for (const position of ["native-turn", "final"]) {
    const { html, details } = render(fixture(), `outside-${position}`, position);
    expect(details[0].props.open).toBe(false);
    expect(html).not.toContain("Private process fixture");
    expect(html).toContain("Visible final answer");
  }
});

test("segment choices survive pagination and remain independent within a turn and across machines", () => {
  observed.machineScope = "local";
  const events = fixture().filter(item => item.id !== "explain");
  const initial = render(events.slice(2), "stable");
  toggle(initial.details[0]);
  const refreshed = render(events, "stable");
  expect(refreshed.details[0].props.open).toBe(true);
  expect(refreshed.html).toContain("Private process fixture");
  expect(render(events, "other-session").details[0].props.open).toBe(false);
  observed.machineScope = "remote:fixture";
  expect(render(events, "stable").details[0].props.open).toBe(false);
  observed.machineScope = "local";
  const separate = render(fixture(), "segments");
  toggle(separate.details[0]);
  const opened = render(fixture(), "segments");
  expect(opened.html).toContain("Private process fixture");
  expect(opened.html).not.toContain("Synthetic tool result");
  expect(opened.details[1].props.open).toBe(false);
});

test("plan body and actions remain visible while waiting tools are folded", () => {
  const events = [
    event("plan", "plan", { text: "# Fixture plan\nPlan body", callId: "plan" }),
    event("failure", "system", { role: "api_error", status: "failed", text: "Synthetic failure" }),
    event("ask", "tool_call", { role: "AskUserQuestion", status: "in_progress", callId: "ask" }),
    event("permission", "permission_request", { status: "in_progress", text: "Approve fixture", detail: "Fixture authorization" })
  ];
  const closed = render(events, "waiting");
  for (const label of ["1 项失败", "待回复", "待授权", "展开查看"]) expect(closed.html).toContain(label);
  expect(closed.html).not.toContain("thread-running-indicator");
  expect(closed.html).toContain("Plan body");
  const opened = render(events, "waiting", "plan");
  expect(opened.html).toContain("Plan body");
  expect(opened.html).toContain('aria-label="复制计划"');
  expect(opened.html).toContain('aria-label="下载计划 Markdown"');
});

test("a running tool has a visible indicator without mounting its output", () => {
  const { html, details } = render([event("tool", "tool_call", { role: "Read", callId: "call", status: "in_progress", detail: "Unfinished input" })], "running");
  expect(details[0].props.open).toBe(false);
  expect(html).toContain("thread-running-indicator");
  expect(html).not.toContain("Unfinished input");
});

test("automatic preamble and summaries fold without bubbles and keep search and pagination aliases", () => {
  const preamble = [
    event("snapshot", "file-history-snapshot", { turnId: null, text: "文件历史快照", detail: "Snapshot fixture" }),
    event("settings", "atis-latch", { turnId: null, text: "会话设置", detail: "Settings fixture" })
  ];
  const later = [event("request", "user_message", { text: "/plan\nActual request" }), event("compact", "compaction", { text: "Compaction fixture" })];
  const closed = render([...preamble, ...later], "metadata");
  expect(closed.html).toContain("系统活动");
  expect(closed.html).toContain("压缩摘要");
  expect(closed.html).not.toContain("Snapshot fixture");
  expect(closed.html).not.toContain("Compaction fixture");
  expect(closed.html.match(/user-message-bubble/g)).toHaveLength(1);
  const partial = render([preamble[1], ...later], "metadata-paging");
  toggle(partial.details[0]);
  expect(render([...preamble, ...later], "metadata-paging").details[0].props.open).toBe(true);
  const found = render([...preamble, ...later], "metadata-search", "snapshot");
  expect(found.html).toContain("Snapshot fixture");
  expect(found.html).not.toContain("Compaction fixture");
});

test("interleaved commands, atis-latch, native_context and trailing turn_duration/cost-state after API error fold cleanly", () => {
  const events = [
    event("u1", "user_message", { text: "Inspect pipeline" }),
    event("t1", "thinking", { text: "Hidden step 1" }),
    event("c1", "tool_call", { callId: "b1", role: "Bash", status: "completed", detail: "secret-cmd-1" }),
    event("l1", "atis-latch", { text: "会话设置", detail: "latch-detail" }),
    event("r1", "tool_result", { callId: "b1", status: "completed", result: "secret-out-1" }),
    event("n1", "system", { role: "native_context", text: "上下文", detail: "ctx-detail" }),
    event("c2", "tool_call", { callId: "b2", role: "Bash", status: "completed", detail: "secret-cmd-2" }),
    event("r2", "tool_result", { callId: "b2", status: "completed", result: "secret-out-2" }),
    event("err", "assistant_message", { text: "API Error: 502 Upstream request failed (https://example.invalid)." }),
    event("dur", "system", { role: "turn_duration", detail: "12s" }),
    event("cost", "cost-state", { text: "用量状态", detail: "tokens" }),
  ];
  const closed = render(events, "d3678-regression");
  expect(closed.html).toContain("已思考");
  expect(closed.html).toContain("2 项工具");
  expect(closed.html).toContain("API Error: 502 Upstream request failed");
  expect(closed.html).not.toContain("Hidden step 1");
  expect(closed.html).not.toContain("secret-cmd-1");
  expect(closed.html).not.toContain("secret-out-1");
  expect(closed.html).not.toContain("secret-cmd-2");
  expect(closed.html).not.toContain("ctx-detail");
});


test("recorded child tools without a result stay unknown without running indicators", () => {
  const events = [event("recorded-tool", "tool_call", { role: "Bash", status: "running", detail: "printf fixture" })];
  const html = renderToStaticMarkup(<DisclosureScope.Provider value="child-recorded-only"><DisclosureRevealContext.Provider value="recorded-tool">
    {renderClaudeEvents(groupClaudeProcesses(events, true), "Fixture child")}
  </DisclosureRevealContext.Provider></DisclosureScope.Provider>);
  expect(html).toContain("未见结果");
  expect(html).not.toMatch(/运行中|正在执行|已完成|running-indicator|is-running/);
  expect(groupClaudeProcesses(events).some(entry => entry.kind === "process" && entry.process.running)).toBe(true);
});
