import { isInstructionFileActivity, visibleMarkdown } from "./visibleMarkdown";
import type { MessageBlock, GrokHistoryEvent, ClaudeHistoryEvent, ProviderHistoryEvent } from "../../types";
import { isToolBlock, isHistoryCollapsedBlock, isPlanBlock, isQuestionBlock, isQuestionResultBlock, toolBlockDetailText } from "./conversationViewModel";

export type ExecutionStatus = "running" | "failed" | "completed" | "unknown";
export type ExecutionCommand = {
  id: string;
  sourceIds: string[];
  title: string;
  preview?: string;
  status: ExecutionStatus;
  instructionFile?: boolean;
  sections: Array<{ label: string; text: string }>;
};
export type ExecutionGroup = {
  id: string;
  mergedIds?: string[];
  commands: ExecutionCommand[];
  kind: "command" | "tool";
  provider: "Codex" | "Grok" | "Claude Code" | "Antigravity" | "DeepSeek Harness" | "Grok Bot";
  running: boolean;
  recordedOnly?: boolean;
  failedCount: number;
  turnId?: string | null;
};
export type ExecutionRenderItem<T> =
  | { kind: "item"; item: T; key: string; sourceIds: string[] }
  | { kind: "group"; group: ExecutionGroup };

const RUNNING = new Set(["pending", "running", "in_progress", "inprogress", "active", "generating"]);
const FAILED = new Set(["failed", "error", "cancelled", "canceled", "rejected"]);
const COMMAND_NAMES = new Set([
  "exec", "exec_command", "run_command", "run_terminal_command", "sleep", "js", "javascript",
  "bash", "bash_execution", "bashexecution", "shell", "terminal", "python", "node", "execute", "command_execution", "commandexecution"
]);

export function executionStatus(value?: string | null): ExecutionStatus {
  const status = value?.trim().toLowerCase() ?? "";
  if (RUNNING.has(status)) return "running";
  if (FAILED.has(status)) return "failed";
  return "completed";
}

// Inspect tool names/titles, never arbitrary output (which may mention another tool).
export function isCommandText(value?: string | null): boolean {
  const name = value?.trim().toLowerCase().split(/[\s`(]/, 1)[0].split(/\.|__|\/|:/).pop() ?? "";
  return COMMAND_NAMES.has(name);
}

function commandTitle(...values: Array<string | null | undefined>): string {
  for (const value of values) {
    const normalized = value?.trim().toLowerCase().split(/[\s`(]/, 1)[0].split(/\.|__|\/|:/).pop() ?? "";
    if (!COMMAND_NAMES.has(normalized)) continue;
    if (normalized === "bashexecution" || normalized === "bash_execution") return "bash";
    if (normalized === "commandexecution" || normalized === "command_execution") return "execute";
    if (normalized === "run_terminal_command" || normalized === "run_command") return "exec";
    return normalized;
  }
  return values.find(value => value?.trim())?.trim().split(/\s+/, 1)[0] || "命令";
}

// Prefer native identities; content fingerprints are only a fallback for old events.
export function activityFingerprint(value: string): string {
  let hash = 2166136261;
  for (let index = 0; index < value.length; index++) hash = Math.imul(hash ^ value.charCodeAt(index), 16777619);
  return (hash >>> 0).toString(36);
}

function activityPreview(...sources: Array<string | null | undefined>): string | undefined {
  for (const source of sources) {
    if (!source?.trim()) continue;
    let text = source;
    if (source.trimStart().startsWith("{")) {
      try {
        const value: unknown = JSON.parse(source);
        if (value && typeof value === "object") {
          const fields = value as Record<string, unknown>;
          text = ["cmd", "command", "title", "path", "file_path", "query", "code"].map(key => fields[key]).find(value => typeof value === "string") as string || source;
        }
      } catch { /* Native commands and titles may already be plain text. */ }
    }
    const cleaned = visibleMarkdown(text).replace(/\s+/g, " ").trim();
    if (cleaned) return cleaned.length > 180 ? `${cleaned.slice(0, 180)}…` : cleaned;
  }
  return undefined;
}

type Activity<T> = {
  item: T;
  key: string;
  sourceIds: string[];
  callId?: string | null;
  turnId?: string | null;
  tool: boolean;
  command: boolean;
  instructionFile: boolean;
  title: string;
  preview?: string;
  phase: "call" | "result" | "update";
  status?: string | null;
  sections: ExecutionCommand["sections"];
};

function groupActivities<T>(activities: Activity<T>[], provider: ExecutionGroup["provider"], recordedOnly = false): ExecutionRenderItem<T>[] {
  type Entry =
    | { kind: "item"; item: T; key: string; sourceIds: string[] }
    | { kind: "command"; command: ExecutionCommand; isCommand: boolean; newTurn: boolean; turnId?: string | null };
  const entries: Entry[] = [];
  const pending = new Map<string, ExecutionCommand>();
  const occurrences = new Map<string, number>();
  let turn: string | null | undefined;
  for (const activity of activities) {
    const occurrence = occurrences.get(activity.key) ?? 0;
    occurrences.set(activity.key, occurrence + 1);
    const key = `${activity.key}:${occurrence}`;
    const newTurn = Boolean(activity.turnId && turn && activity.turnId !== turn);
    if (newTurn) pending.clear();
    if (activity.turnId) turn = activity.turnId;
    const previous = activity.tool && activity.callId && activity.phase !== "call" ? pending.get(activity.callId) : undefined;
    if (previous) {
      previous.instructionFile ||= activity.instructionFile;
      if (activity.status || activity.phase === "result") previous.status = executionStatus(activity.status);
      for (const section of activity.sections) {
        if (!previous.sections.some(existing => existing.label === section.label && existing.text === section.text)) previous.sections.push(section);
      }
      for (const sourceId of activity.sourceIds) if (!previous.sourceIds.includes(sourceId)) previous.sourceIds.push(sourceId);
      if (activity.phase === "result" && activity.callId) pending.delete(activity.callId);
      continue;
    }
    if (!activity.tool) {
      entries.push({ kind: "item", item: activity.item, key, sourceIds: [...activity.sourceIds] });
      continue;
    }
    const command: ExecutionCommand = {
      id: key,
      sourceIds: [...activity.sourceIds],
      title: activity.title,
      preview: activity.preview,
      instructionFile: activity.instructionFile,
      status: activity.status ? executionStatus(activity.status) : activity.phase === "call" ? "running" : "completed",
      sections: [...activity.sections]
    };
    if (recordedOnly && command.status === "running") command.status = "unknown";
    if (activity.callId) pending.set(activity.callId, command);
    entries.push({ kind: "command", command, isCommand: activity.command, newTurn, turnId: activity.turnId ?? turn });
  }
  const output: ExecutionRenderItem<T>[] = [];
  for (const entry of entries) {
    if (entry.kind === "item") {
      output.push(entry);
      continue;
    }
    const previous = output[output.length - 1];
    const group = previous?.kind === "group" && !entry.newTurn ? previous.group : undefined;
    if (group) {
      group.commands.push(entry.command);
      if (!entry.isCommand) group.kind = "tool";
      if (!group.turnId && entry.turnId) group.turnId = entry.turnId;
    } else output.push({ kind: "group", group: { id: `group:${entry.command.id}`, commands: [entry.command], kind: entry.isCommand ? "command" : "tool", provider, recordedOnly, running: false, failedCount: 0, turnId: entry.turnId } });
  }
  for (const entry of output) {
    if (entry.kind !== "group") continue;
    entry.group.running = entry.group.commands.some(command => command.status === "running");
    entry.group.failedCount = entry.group.commands.filter(command => command.status === "failed").length;
  }
  return output;
}

export function groupCodexCommandBlocks(blocks: MessageBlock[]): ExecutionRenderItem<MessageBlock>[] {
  return groupActivities(blocks.map(block => {
    const result = /output|result/i.test(block.kind);
    const tool = !block.subagent && isToolBlock(block) && !isHistoryCollapsedBlock(block) && !isPlanBlock(block) && !isQuestionBlock(block) && !isQuestionResultBlock(block);
    return {
      item: block,
      key: `codex:${block.turn_id ?? ""}:${block.call_id ?? block.item_id ?? "event"}:${block.id}`,
      sourceIds: [block.id, block.call_id, block.item_id].filter((value): value is string => Boolean(value)),
      callId: block.call_id,
      turnId: block.turn_id,
      tool,
      instructionFile: tool && isInstructionFileActivity(block.tool_name, block.input, block.summary, toolBlockDetailText(block)),
      command: tool && (isCommandText(block.tool_name) || isCommandText(block.kind)),
      title: commandTitle(block.tool_name, block.kind),
      preview: tool ? activityPreview(block.input) : undefined,
      phase: result ? "result" : "call",
      status: block.status,
      sections: tool ? [{ label: result ? "结果" : "命令详情", text: toolBlockDetailText(block) }] : []
    };
  }), "Codex");
}

export function groupGrokCommandEvents(events: GrokHistoryEvent[]): ExecutionRenderItem<GrokHistoryEvent>[] {
  return groupActivities(events.map((event, index) => {
    const command = event.kind.startsWith("tool_") && (isCommandText(event.method) || isCommandText(event.text));
    return {
      item: event,
      key: `grok:${event.id ?? event.userMessage?.id ?? event.callId ?? `${event.timestamp ?? ""}:${event.kind}:${activityFingerprint(event.kind.startsWith("tool_") ? event.text ?? event.detail ?? "" : "")}`}`,
      sourceIds: [event.id, `event:${index}`, event.callId].filter((value): value is string => Boolean(value)),
      callId: event.callId,
      tool: event.kind.startsWith("tool_"),
      instructionFile: isInstructionFileActivity(event.method, event.text, event.detail),
      command,
      title: command ? commandTitle(event.method, event.text) : event.text?.trim() || "工具活动",
      preview: activityPreview(event.kind === "tool_call" ? event.detail : undefined, command && event.text !== commandTitle(event.method, event.text) ? event.text : undefined),
      phase: event.kind === "tool_call" ? "call" : event.kind === "tool_result" ? "result" : "update",
      status: event.status,
      sections: [
        ...(command && event.text && event.text !== commandTitle(event.method, event.text) ? [{ label: "命令", text: event.text }] : []),
        ...(event.detail ? [{ label: command ? "命令详情" : "工具详情", text: event.detail }] : [])
      ]
    };
  }), "Grok");
}

export function groupClaudeEvents(events: ClaudeHistoryEvent[], recordedOnly = false): ExecutionRenderItem<ClaudeHistoryEvent>[] {
  return groupActivities(events.map(event => ({
    item: event,
    key: `claude:${event.id}`,
    sourceIds: [event.id],
    callId: event.callId,
    turnId: event.turnId,
    tool: event.kind === "tool_call" || event.kind === "tool_result",
    instructionFile: isInstructionFileActivity(event.role, event.text, event.detail, event.result),
    command: isCommandText(event.role) || isCommandText(event.text),
    title: event.role || event.text?.trim() || "工具活动",
    preview: activityPreview(event.detail),
    phase: event.kind === "tool_result" ? "result" : "call",
    status: event.status,
    sections: [
      ...(event.detail ? [{ label: "调用参数", text: event.detail }] : []),
      ...(event.result ? [{ label: "结果", text: event.result }] : [])
    ]
  })), "Claude Code", recordedOnly);
}

export function groupProviderEvents(
  events: ProviderHistoryEvent[],
  provider: "Antigravity" | "DeepSeek Harness" | "Grok Bot",
): ExecutionRenderItem<ProviderHistoryEvent>[] {
  const prefix = provider === "Antigravity" ? "antigravity" : provider === "Grok Bot" ? "grokbot" : "dsh";
  return groupActivities(events.map(event => ({
    item: event,
    key: `${prefix}:${event.id}`,
    sourceIds: [event.id, event.callId].filter((value): value is string => Boolean(value)),
    callId: event.callId,
    turnId: event.turnId,
    tool: event.kind === "tool_call" || event.kind === "tool_result",
    instructionFile: isInstructionFileActivity(event.role, event.text, event.detail, event.result),
    command: isCommandText(event.role) || isCommandText(event.text),
    title: isCommandText(event.role)
      ? commandTitle(event.role, event.text)
      : event.role || event.text?.trim() || "工具活动",
    preview: activityPreview(event.detail, event.text && event.text !== event.role ? event.text : undefined),
    phase: event.kind === "tool_result" ? "result" : "call",
    status: event.status,
    sections: [
      ...(event.detail ? [{ label: "调用参数", text: event.detail }] : []),
      ...(event.result ? [{ label: "结果", text: event.result }] : [])
    ]
  })), provider);
}
