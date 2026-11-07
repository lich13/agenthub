import { visibleMarkdown } from "./visibleMarkdown";
import type {
  MessageBlock,
  PendingElicitation,
} from "../../types";

export type MessageScrollSnapshot = {
  scrollTop: number;
  clientHeight: number;
  scrollHeight: number;
};

export type ConversationMessagePresentation = {
  kind: "user" | "assistant";
  rowClassName: string;
  bodyClassName: string;
};

export function shouldAutoFollowMessageStream(snapshot: MessageScrollSnapshot, threshold = 96): boolean {
  return snapshot.scrollHeight - snapshot.scrollTop - snapshot.clientHeight <= threshold;
}

export function isRunningToolBlock(block: MessageBlock): boolean {
  if (!isToolBlock(block)) return false;
  const status = block.status?.trim();
  return Boolean(status && ["pending", "running", "in_progress", "inProgress", "active"].includes(status));
}

export function latestAssistantCopyText(blocks: MessageBlock[]): string | null {
  const latest = [...blocks].reverse().find((block) =>
    block.role === "assistant" && shouldRenderConversationMessage(block)
  );
  const text = latest ? messageBlockText(latest).trim() : "";
  return text || null;
}

export function threadResumeCommand(threadId?: string | null): string | null {
  const id = threadId?.trim();
  return id ? `codex resume ${id}` : null;
}

export function isPlanBlock(block: MessageBlock): boolean {
  if (block.role === "user") return false;
  const kind = normalizedBlockKind(block);
  const displayKind = normalizedDisplayKind(block);
  return displayKind === "plan" || kind.includes("plan") || Boolean(block.text?.includes("<proposed_plan>"));
}

export function isApprovalBlock(block: MessageBlock): boolean {
  const kind = normalizedBlockKind(block);
  const displayKind = normalizedDisplayKind(block);
  return displayKind === "approval" || kind.includes("requestapproval") || kind.includes("approval") || kind.includes("permissions/request");
}

export function isQuestionBlock(block: MessageBlock): boolean {
  const kind = normalizedBlockKind(block);
  const displayKind = normalizedDisplayKind(block);
  return displayKind === "question"
    || (kind === "request_user_input" || kind === "requestuserinput")
    || kind.includes("request_user_input")
    || kind.includes("requestuserinput")
    || (block.questions?.length ?? 0) > 0;
}

export function isQuestionResultBlock(block: MessageBlock): boolean {
  const kind = normalizedBlockKind(block);
  const displayKind = normalizedDisplayKind(block);
  return displayKind === "question_result"
    || kind === "request_user_input_result"
    || (block.answers?.length ?? 0) > 0;
}

export function isToolBlock(block: MessageBlock): boolean {
  const kind = normalizedBlockKind(block);
  return block.role === "tool"
    || kind.includes("tool")
    || kind.includes("command")
    || kind.includes("function_call")
    || kind.includes("web_search");
}

export function shouldRenderConversationMessage(block: MessageBlock): boolean {
  const role = block.role || "";
  const kind = normalizedBlockKind(block);
  if (!["assistant", "user"].includes(role)) return false;
  if ((block.questions?.length ?? 0) > 0) return false;
  if (kind === "request_user_input" || kind === "requestuserinput") return false;
  if (isToolBlock(block) || isPlanBlock(block) || isApprovalBlock(block)) return false;
  if (block.role === "user") {
    if (/^<(?:user_shell_command|turn_aborted)>|^========= memory_summary begins =========/i.test((block.text ?? "").trimStart())) return false;
  } else if (isInternalContextText(block.text)) return false;
  return !["reasoning", "agent_reasoning", "session_meta"].includes(kind);
}

export function shouldRenderConversationBlock(block: MessageBlock): boolean {
  if (isApprovalBlock(block)) return false;
  if (isPlanBlock(block) || isQuestionBlock(block) || isQuestionResultBlock(block)) return true;
  return isToolBlock(block) || shouldRenderConversationMessage(block);
}

export function isResolvedActionBlock(block: MessageBlock): boolean {
  if (block.resolved === true) return true;
  const status = (block.plan_status ?? block.status ?? "").toLowerCase();
  return ["completed", "complete", "succeeded", "success", "done", "approved", "declined", "rejected", "cancelled", "canceled", "failed"].includes(status);
}

export function isActionablePlanBlock(block: MessageBlock, current: MessageBlock | null | undefined): boolean {
  return Boolean(current && isPlanBlock(block) && !isResolvedActionBlock(block) && sameActionBlock(block, current));
}

export function isActionableQuestionBlock(block: MessageBlock, current: PendingElicitation | MessageBlock | null | undefined): boolean {
  if (!current || !isQuestionBlock(block) || isResolvedActionBlock(block)) return false;
  const currentTurnId = current.turn_id ?? null;
  if (currentTurnId && block.turn_id !== currentTurnId) return false;
  const currentItemId = "id" in current ? current.item_id ?? current.call_id ?? current.id : current.item_id ?? null;
  if (!currentItemId) return Boolean(currentTurnId);
  return [block.item_id, block.call_id, block.id].some((value) => value === currentItemId);
}

function sameActionBlock(left: MessageBlock, right: MessageBlock): boolean {
  const leftIds = [left.id, left.item_id, left.call_id].filter(Boolean);
  const rightIds = [right.id, right.item_id, right.call_id].filter(Boolean);
  const sameId = leftIds.some((leftId) => rightIds.includes(leftId));
  if (!sameId) return false;
  if (left.turn_id && right.turn_id) return left.turn_id === right.turn_id;
  return true;
}

function normalizedBlockKind(block: Pick<MessageBlock, "kind">): string {
  return block.kind.toLowerCase();
}

function normalizedDisplayKind(block: Pick<MessageBlock, "display_kind">): string {
  return block.display_kind?.toLowerCase() ?? "";
}

export function conversationMessagePresentation(block: Pick<MessageBlock, "role">): ConversationMessagePresentation {
  const kind = block.role === "user" ? "user" : "assistant";
  return {
    kind,
    rowClassName: `chat-row ${kind}`,
    bodyClassName: kind === "user" ? "chat-bubble" : "assistant-message-body"
  };
}

export function visibleConversationBlocksForHistory(
  blocks: MessageBlock[],
  showAllHistory: boolean,
  currentPlan?: MessageBlock | null,
  currentQuestion?: PendingElicitation | MessageBlock | null,
  revealedBlockId?: string | null
): MessageBlock[] {
  const renderable = blocks.filter(shouldRenderConversationBlock);
  if (showAllHistory) return renderable;
  return compactConversationBlocks(renderable, 60, 3, currentPlan, currentQuestion, revealedBlockId);
}

export function compactConversationBlocks(
  blocks: MessageBlock[],
  maxChatMessages = 60,
  maxActionBlocks = 3,
  currentPlan?: MessageBlock | null,
  currentQuestion?: PendingElicitation | MessageBlock | null,
  revealedBlockId?: string | null
): MessageBlock[] {
  if (blocks.some((block) => historyCollapseKind(block) === "action")) {
    return blocks;
  }

  const actionIndexes = blocks
    .map((block, index) => ({ block, index }))
    .filter(({ block }) => {
      if (block.id === revealedBlockId) return false;
      if (isHistoryCollapsedBlock(block)) return false;
      if (isActionablePlanBlock(block, currentPlan)) return false;
      if (isActionableQuestionBlock(block, currentQuestion)) return false;
      return isPlanBlock(block) || isQuestionBlock(block) || isQuestionResultBlock(block);
    })
    .map(({ index }) => index);
  const actionCompacted = compactIndexedBlocks(
    blocks,
    actionIndexes,
    maxActionBlocks,
    "action-history-collapsed",
    "action_history_collapsed",
    "action_history",
    "条历史计划/问题已折叠"
  );

  if (actionCompacted.some((block) => historyCollapseKind(block) === "chat")) {
    return actionCompacted;
  }

  const chatIndexes = actionCompacted
    .map((block, index) => ({ block, index }))
    .filter(({ block }) => block.id !== revealedBlockId && shouldRenderConversationMessage(block))
    .map(({ index }) => index);
  return compactIndexedBlocks(
    actionCompacted,
    chatIndexes,
    maxChatMessages,
    "chat-history-collapsed",
    "chat_history_collapsed",
    "chat_history",
    "条历史对话已折叠"
  );
}

function compactIndexedBlocks(
  blocks: MessageBlock[],
  indexes: number[],
  maxVisible: number,
  id: string,
  kind: string,
  toolName: string,
  label: string
): MessageBlock[] {
  if (indexes.length <= maxVisible) return blocks;
  const keep = new Set(indexes.slice(-maxVisible));
  const hide = new Set(indexes.slice(0, -maxVisible));
  const hidden = indexes.length - keep.size;
  const collapsed: MessageBlock = {
    id,
    role: "tool",
    kind,
    status: "completed",
    text: `${hidden} ${label}`,
    summary: `${hidden} ${label}`,
    tool_name: toolName,
    truncated: false,
    questions: []
  };
  const compacted: MessageBlock[] = [];
  let segment: MessageBlock | null = null;
  let segmentCount = 0;
  let inserted = false;
  for (const [index, block] of blocks.entries()) {
    if (hide.has(index) && !keep.has(index)) {
      if (!segment) {
        segment = {
          ...collapsed,
          id: inserted ? `${id}:${block.id}` : id,
          payload: { collapsedIds: [] as string[] }
        };
        compacted.push(segment);
        inserted = true;
        segmentCount = 0;
      }
      (segment.payload as { collapsedIds: string[] }).collapsedIds.push(block.id);
      segmentCount += 1;
      segment.text = segment.summary = `${segmentCount} ${label}`;
      continue;
    }
    segment = null;
    compacted.push(block);
  }
  return compacted;
}

export function isHistoryCollapsedBlock(block: MessageBlock): boolean {
  return historyCollapseKind(block) !== null;
}

export function historyCollapseKind(block: MessageBlock): "chat" | "tool" | "action" | null {
  const kind = block.kind.toLowerCase();
  if (kind === "chat_history_collapsed") return "chat";
  if (kind === "tool_history_collapsed") return "tool";
  if (kind === "action_history_collapsed") return "action";
  return null;
}

function isInternalContextText(value?: string | null): boolean {
  const text = value?.trimStart().toLowerCase();
  if (!text) return false;
  return [
    "<environment_context>",
    "<permissions instructions>",
    "<app-context>",
    "<collaboration_mode>",
    "<skills_instructions>",
    "<plugins_instructions>",
    "<subagent_notification>",
    "<subagent_context>",
    "<codex_internal_context",
    "<goal_context>",
    "<additional_context>",
    "<user_instructions>",
    "<turn_aborted>",
    "<user_shell_command>",
    "<legacy_unified_exec_process_limit_warning>",
    "<legacy_apply_patch_exec_command_warning>",
    "<legacy_model_mismatch_warning>",
    "========= memory_summary begins ========="
  ].some((prefix) => text.startsWith(prefix));
}

export function toolBlockTitle(block: MessageBlock): string {
  if (isHistoryCollapsedBlock(block)) {
    return firstDisplayLine(block.summary)
      ?? firstDisplayLine(block.text)
      ?? block.tool_name?.trim()
      ?? block.kind
      ?? "tool";
  }
  return block.tool_name?.trim() || block.kind || "tool";
}

export function toolBlockStatus(block: MessageBlock): string {
  return block.status?.trim() || block.call_id || "completed";
}

export function toolBlockSummary(block: MessageBlock): string | null {
  return firstDisplayLine(block.summary)
    ?? firstDisplayLine(block.text)
    ?? firstDisplayLine(block.input)
    ?? null;
}

export function toolBlockDetailText(block: MessageBlock): string {
  const sections: string[] = [];
  const input = block.input?.trim();
  const output = (block.text?.trim() || formatPayload(block.payload).trim());
  if (input) sections.push(`Input\n${input}`);
  if (output) sections.push(`${input ? "Output\n" : ""}${output}`);
  if (block.truncated) sections.push("[output truncated]");
  return sections.join("\n\n") || "No output";
}

export function messageBlockText(block: MessageBlock): string {
  return visibleMarkdown(block.text?.trim() || formatPayload(block.payload) || "");
}

function firstDisplayLine(value?: string | null): string | null {
  const line = value?.split(/\r?\n/).map((item) => item.trim()).find(Boolean);
  return line || null;
}

export function blockKindLabel(kind: string): string {
  if (kind.includes("agentMessage")) return "assistant";
  if (kind.includes("userMessage")) return "user";
  if (kind.includes("function")) return "tool";
  return kind;
}

export function roleLabel(role: string): string {
  if (role === "assistant") return "Codex";
  if (role === "user") return "User";
  if (role === "tool") return "Tool";
  return role || "System";
}

export function formatPayload(payload: unknown): string {
  if (!payload) return "";
  if (typeof payload === "string") return payload;
  return JSON.stringify(payload, null, 2);
}

export function formatTime(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleString();
}
