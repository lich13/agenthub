import { expect, test } from "vitest";
import type { ClaudeHistoryEvent } from "../../types";
import { groupClaudeProcesses, type ClaudeRenderItem } from "./claudeProcessGroups";

const event = (id: string, kind: string, fields: Partial<ClaudeHistoryEvent> = {}): ClaudeHistoryEvent => ({ id, kind, turnId: "turn-1", ...fields });
function process(entries: ClaudeRenderItem[]) {
  const found = entries.find(entry => entry.kind === "process");
  if (!found || found.kind !== "process") throw new Error("Process missing");
  return found.process;
}

test("one native turn keeps intermediate replies visible between paired tool segments", () => {
  const result = groupClaudeProcesses([
    event("user", "user_message"), event("think", "thinking"),
    event("explain", "assistant_message", { assistantPhase: "intermediate" }),
    event("call", "tool_call", { callId: "c1", role: "Read", status: "in_progress", detail: "fixture" }),
    event("result", "tool_result", { callId: "c1", status: "completed", result: "output" }),
    event("final", "assistant_message", { assistantPhase: "final", text: "Final answer" }),
    event("duration", "system", { role: "turn_duration" })
  ]);
  expect(result.map(entry => entry.kind)).toEqual(["item", "process", "item", "process", "item", "metadata"]);
  const segments = result.filter(entry => entry.kind === "process");
  expect(segments[0].process.sourceIds).toEqual(["think"]);
  const group = segments[1].process;
  expect(group.sourceIds).toEqual(["call", "result"]);
  expect(group.stateKey).not.toBe(segments[0].process.stateKey);
  expect(result[2]).toMatchObject({ kind: "item", item: { id: "explain" } });
  const tools = group.entries[0];
  if (tools.kind !== "group") throw new Error("Tool group missing");
  expect(tools.group.commands).toHaveLength(1);
  expect(tools.group.commands[0].sourceIds).toEqual(["call", "result"]);
  expect(tools.group.commands[0].status).toBe("completed");
  expect(group).toMatchObject({ segmentCount: 0, toolCount: 1, completed: true, running: false });
});

test("unknown-phase prose and legacy events remain visible at their original positions", () => {
  const unknown = groupClaudeProcesses([
    event("think", "thinking"), event("unknown", "assistant_message", { text: "Unclassified text" }),
    event("tool", "tool_call", { callId: "call", role: "Read" })
  ]);
  expect(unknown.map(entry => entry.kind)).toEqual(["process", "item", "process"]);
  expect(unknown[1]).toMatchObject({ kind: "item", item: { id: "unknown" } });
  const legacy = groupClaudeProcesses([event("old-think", "thinking", { turnId: null }), event("old-reply", "assistant_message", { turnId: null })]);
  expect(legacy.map(entry => entry.kind)).toEqual(["item", "item"]);
  const trailing = groupClaudeProcesses([event("think", "thinking"), event("unknown", "assistant_message")]);
  expect(trailing.map(entry => entry.kind)).toEqual(["process", "item"]);
  expect(process(trailing).completed).toBe(false);
});

test("running comes from native evidence and ends at a new turn or explicit context boundary", () => {
  const call = event("call", "tool_call", { callId: "c1", status: "in_progress" });
  const active = groupClaudeProcesses([call, event("unknown", "assistant_message")]);
  expect(process(active).running).toBe(true);
  expect(process(groupClaudeProcesses([event("final", "assistant_message", { assistantPhase: "final" }), call])).running).toBe(true);
  for (const boundary of [event("next", "user_message", { turnId: "turn-2" }), event("compact", "compaction")]) {
    const result = groupClaudeProcesses([call, boundary, event("later", "thinking", { turnId: boundary.turnId })]);
    const groups = result.filter(entry => entry.kind === "process");
    expect(groups).toHaveLength(2);
    expect(process(result).running).toBe(false);
    expect(groups[0]).not.toEqual(groups[1]);
  }
  expect(process(groupClaudeProcesses([event("think", "thinking")])).running).toBe(false);
});

test("tool result refresh updates process status without changing the native turn state key", () => {
  const call = event("call", "tool_call", { callId: "c1", status: "in_progress" });
  const initial = process(groupClaudeProcesses([call]));
  const updated = process(groupClaudeProcesses([call, event("result", "tool_result", { callId: "c1", status: "completed", result: "New output" })]));
  expect(initial.running).toBe(true);
  expect(updated.running).toBe(false);
  expect(updated.stateKey).toBe(initial.stateKey);
  const prepended = process(groupClaudeProcesses([event("earlier", "thinking"), call]));
  expect(prepended.sourceIds).toContain(initial.sourceIds[0]);
  expect(prepended.sourceIds).toEqual(["earlier", "call"]);
  const context = process(groupClaudeProcesses([event("context", "compaction"), call]));
  expect(context.stateKey).toBe(initial.stateKey);
  expect(context.id).toBe(initial.id);
});

test("plans stay visible and process headers retain failure, reply and authorization evidence", () => {
  const base = [event("plan", "plan", { callId: "plan-call" }), event("failure", "system", { role: "api_error", status: "failed" })];
  const question = process(groupClaudeProcesses([...base, event("ask", "tool_call", { role: "AskUserQuestion", callId: "q", status: "in_progress" })]));
  expect(question).toMatchObject({ failedCount: 1, needsReply: true, toolCount: 1, running: false });
  const permission = event("permission", "permission_request", { status: "in_progress" });
  expect(process(groupClaudeProcesses([permission]))).toMatchObject({ needsPermission: true, hasPermission: true, running: false });
  expect(process(groupClaudeProcesses([{ ...permission, status: "completed" }]))).toMatchObject({ needsPermission: false, hasPermission: true });
  expect(process(groupClaudeProcesses([permission, event("failure", "failure", { status: "failed" })]))).toMatchObject({ needsPermission: false, failedCount: 1, running: false });
});

test("API error reply followed by trailing turn_duration and cost-state still folds preceding commands and coalesces across atis-latch", () => {
  const result = groupClaudeProcesses([
    event("user", "user_message", { text: "Run CI checks" }),
    event("think-1", "thinking", { text: "Checking build" }),
    event("explain-1", "assistant_message", { assistantPhase: "intermediate", text: "Running first command" }),
    event("bash-1", "tool_call", { callId: "b1", role: "Bash", status: "in_progress", detail: "{\"command\":\"cargo test\"}" }),
    event("latch-1", "atis-latch", { text: "会话设置", detail: "{}" }),
    event("bash-1-out", "tool_result", { callId: "b1", status: "completed", result: "ok" }),
    event("ctx-1", "system", { role: "native_context", detail: "context" }),
    event("bash-2", "tool_call", { callId: "b2", role: "Bash", status: "in_progress", detail: "{\"command\":\"cargo clippy\"}" }),
    event("attach-1", "attachment", { text: "附件记录", detail: "{}" }),
    event("bash-2-out", "tool_result", { callId: "b2", status: "completed", result: "ok" }),
    event("api-err", "assistant_message", { text: "API Error: 502 Upstream request failed (https://example.invalid)." }),
    event("duration", "system", { role: "turn_duration" }),
    event("cost", "cost-state", { text: "用量状态", detail: "{}" }),
  ]);
  expect(result.map(entry => entry.kind)).toEqual(["item", "process", "item", "process", "item", "metadata"]);
  const group = result.filter(entry => entry.kind === "process")[1].process;
  expect(group.toolCount).toBe(2);
  expect(group.completed).toBe(false);
  const execGroups = group.entries.filter(entry => entry.kind === "group");
  expect(execGroups).toHaveLength(1);
  if (execGroups[0].kind === "group") {
    expect(execGroups[0].group.commands).toHaveLength(2);
    expect(execGroups[0].group.mergedIds).toEqual(["group:claude:bash-2:0"]);
    expect(execGroups[0].group.commands.map(cmd => cmd.sourceIds)).toEqual([
      ["bash-1", "bash-1-out"],
      ["bash-2", "bash-2-out"],
    ]);
  }
});

 test("tool results separated by visible prose still update their original call", () => {
  const result = groupClaudeProcesses([
    event("call", "tool_call", { callId: "c", status: "in_progress" }),
    event("reply", "assistant_message", { assistantPhase: "intermediate", text: "Visible reply" }),
    event("result", "tool_result", { callId: "c", status: "completed", result: "Done" }),
    event("plan", "plan", { text: "Visible plan" }),
    event("next-call", "tool_call", { callId: "next", status: "in_progress" }),
  ]);
  expect(result.map(entry => entry.kind)).toEqual(["process", "item", "item", "process"]);
  expect(process(result).sourceIds).toEqual(["call", "result"]);
  expect(process(result).running).toBe(false);
});
