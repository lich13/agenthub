import type { ClaudeHistoryEvent } from "../../types";
import { executionStatus, groupClaudeEvents, type ExecutionRenderItem } from "./executionGroups";

export type ClaudeProcessGroup = {
  id: string;
  stateKey: string;
  turnId: string;
  sourceIds: string[];
  entries: ClaudeEventRenderItem[];
  segmentCount: number;
  toolCount: number;
  failedCount: number;
  running: boolean;
  completed: boolean;
  needsReply: boolean;
  needsPermission: boolean;
  hasPermission: boolean;
};
export type ClaudeMetadataGroup = { id: string; sourceIds: string[]; label: string; entries: ExecutionRenderItem<ClaudeHistoryEvent>[] };
export type ClaudeEventRenderItem = ExecutionRenderItem<ClaudeHistoryEvent> | { kind: "metadata"; group: ClaudeMetadataGroup };
export type ClaudeRenderItem = ClaudeEventRenderItem | { kind: "process"; process: ClaudeProcessGroup };

const METADATA_KINDS = new Set(["file-history-snapshot", "file-history-delta", "queue-operation", "cost-state", "atis-latch", "attachment", "compaction", "branch_summary"]);
const AUTOMATIC_ROLES = new Set(["native_context", "local_command", "task_notification", "turn_duration", "away_summary", "stop_hook_summary"]);
function isAutomatic(event: ClaudeHistoryEvent) {
  return METADATA_KINDS.has(event.kind) || (event.kind === "system" && AUTOMATIC_ROLES.has(event.role ?? ""));
}
function foldAutomatic(entries: ExecutionRenderItem<ClaudeHistoryEvent>[]): ClaudeEventRenderItem[] {
  const result: ClaudeEventRenderItem[] = [];
  let pending: Extract<ExecutionRenderItem<ClaudeHistoryEvent>, { kind: "item" }>[] = [];
  const flush = () => {
    if (!pending.length) return;
    const sourceIds = pending.flatMap(entry => entry.sourceIds);
    const label = pending.every(entry => ["compaction", "branch_summary"].includes(entry.item.kind)) ? "压缩摘要"
      : pending.every(entry => entry.item.role === "native_context") ? "上下文" : "系统活动";
    result.push({ kind: "metadata", group: { id: `claude-metadata:${sourceIds[0]}`, label, sourceIds, entries: pending } });
    pending = [];
  };
  for (const entry of entries) {
    if (entry.kind === "item" && isAutomatic(entry.item)) {
      pending.push(entry);
      continue;
    }
    if (
      entry.kind === "group" &&
      pending.length > 0 &&
      pending.every(item => !["compaction", "branch_summary"].includes(item.item.kind))
    ) {
      const prev = result[result.length - 1];
      if (
        prev?.kind === "group" &&
        (!prev.group.turnId || !entry.group.turnId || prev.group.turnId === entry.group.turnId)
      ) {
        const commands = [...prev.group.commands, ...entry.group.commands];
        const mergedIds = [
          ...(prev.group.mergedIds ?? []),
          entry.group.id,
          ...(entry.group.mergedIds ?? [])
        ];
        result[result.length - 1] = {
          kind: "group",
          group: {
            ...prev.group,
            mergedIds,
            commands,
            kind: entry.group.kind === "tool" ? "tool" : prev.group.kind,
            turnId: prev.group.turnId || entry.group.turnId,
            running: commands.some(command => command.status === "running"),
            failedCount: commands.filter(command => command.status === "failed").length
          }
        };
        continue;
      }
    }
    flush();
    result.push(entry);
  }
  flush();
  return result;
}

const PROCESS_KINDS = new Set(["thinking", "tool_call", "tool_result", "tool_reference", "failure", "permission_request", "permission_response", "file-history-snapshot", "file-history-delta", "queue-operation", "cost-state", "atis-latch", "attachment"]);
function isProcess(event: ClaudeHistoryEvent) {
  return PROCESS_KINDS.has(event.kind)
    || (event.kind === "system" && (AUTOMATIC_ROLES.has(event.role ?? "") || event.role === "api_error"));
}

function groupTurn(events: ClaudeHistoryEvent[], turnId: string, ended: boolean): ClaudeRenderItem[] {
  // Pair tools over the whole turn before splitting at visible prose or plans.
  const entries = groupClaudeEvents(events);
  const questionIds = new Set(events.filter(event => event.role === "AskUserQuestion").map(event => event.id));
  const output: ClaudeRenderItem[] = [];
  const finalIndex = entries.reduce((last, entry, index) => entry.kind === "item" && entry.item.assistantPhase === "final" ? index : last, -1);
  let pending: ExecutionRenderItem<ClaudeHistoryEvent>[] = [];
  const flush = (completed: boolean) => {
    if (!pending.length) return;
    const segment = pending;
    pending = [];
    if (segment.every(entry => entry.kind === "item" && isAutomatic(entry.item))) {
      output.push(...foldAutomatic(segment));
      return;
    }
    const commands = segment.flatMap(entry => entry.kind === "group" ? entry.group.commands : []);
    const processEvents = segment.flatMap(entry => entry.kind === "item" ? [entry.item] : []);
    const sourceIds = segment.flatMap(entry => entry.kind === "item" ? entry.sourceIds : entry.group.commands.flatMap(command => command.sourceIds));
    const closed = ended || completed || processEvents.some(event => event.kind === "failure");
    const pendingEvent = (event: ClaudeHistoryEvent) => !closed && executionStatus(event.status) === "running";
    const needsReply = !closed && commands.some(command => command.status === "running" && command.sourceIds.some(id => questionIds.has(id)));
    const needsPermission = processEvents.some(event => event.kind === "permission_request" && pendingEvent(event));
    const stateKey = `claude-process:${turnId}:${sourceIds[0]}`;
    output.push({ kind: "process", process: {
      id: `claude-process[${turnId}][${sourceIds[0]}]`, stateKey, turnId, sourceIds, entries: foldAutomatic(segment),
      segmentCount: processEvents.length, toolCount: commands.length,
      failedCount: commands.filter(command => command.status === "failed").length
        + processEvents.filter(event => executionStatus(event.status) === "failed").length,
      completed, needsReply, needsPermission,
      running: !closed && !needsReply && !needsPermission && (commands.some(command => command.status === "running") || processEvents.some(pendingEvent)),
      hasPermission: processEvents.some(event => event.kind === "permission_request")
    } });
  };
  for (let index = 0; index < entries.length; index++) {
    const entry = entries[index];
    if (entry.kind === "group" || isProcess(entry.item)) {
      pending.push(entry);
    } else {
      flush(finalIndex >= index);
      output.push(...foldAutomatic([entry]));
    }
  }
  flush(false);
  return output;
}

/** Display-only grouping. Native event IDs and inner tool/result aliases survive. */
export function groupClaudeProcesses(events: ClaudeHistoryEvent[]): ClaudeRenderItem[] {
  const output: ClaudeRenderItem[] = [];
  let turn: string | null | undefined;
  let pending: ClaudeHistoryEvent[] = [];
  const flush = (ended: boolean) => {
    if (pending.length) output.push(...(turn ? groupTurn(pending, turn, ended) : foldAutomatic(groupClaudeEvents(pending))));
    pending = [];
  };
  for (const event of events) {
    if (event.turnId !== turn) { flush(true); turn = event.turnId; }
    if (["user_message", "compaction", "branch_summary"].includes(event.kind)) {
      flush(true);
      output.push(...foldAutomatic(groupClaudeEvents([event])));
    } else pending.push(event);
  }
  flush(false);
  return output;
}
