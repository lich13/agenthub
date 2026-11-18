import type { CodexAccountScopeResult, CodexAccountScopeStatus, MessageBlock, ThreadBlockPage, ThreadDetail, ThreadSummary } from "../../types";
import { callCommand } from "./transport";
import { USE_DEMO } from "./shared";
import type { SubagentDetailRequest, SubagentDetailResponse } from "../../types";

export async function getSubagentDetail(request: SubagentDetailRequest): Promise<SubagentDetailResponse> {
  return callCommand<SubagentDetailResponse>("threads.subagentDetail", { request });
}

export async function listThreads(archived = false, q = ""): Promise<ThreadSummary[]> {
  if (USE_DEMO) return (await import("./demo")).demoThreads(archived, q);
  return callCommand<ThreadSummary[]>("threads.list", { archived, q, limit: 120 });
}

export type ThreadDetailOptions = {
  limit?: number;
  before?: string | null;
  full?: boolean;
};

export async function getThread(id: string, options: ThreadDetailOptions = {}): Promise<ThreadDetail> {
  if (USE_DEMO) {
    return (await import("./demo")).demoThreadDetail(id);
  }
  return callCommand<ThreadDetail>("threads.detail", { id, options });
}

export async function getThreadBlocks(id: string, options: Pick<ThreadDetailOptions, "limit" | "before"> = {}): Promise<ThreadBlockPage> {
  if (USE_DEMO) {
    return (await import("./demo")).demoThreadBlockPage(id);
  }
  const page = await callCommand<ThreadBlockPage | {
    history?: ThreadBlockPage["history"]; threadId: string; blocks: MessageBlock[]; totalBlocks: number; hasMoreBlocks: boolean; beforeCursor?: string | null;
  }>("threads.blocks", { id, options });
  if ("thread_id" in page) return page;
  return {
    history: page.history,
    thread_id: page.threadId,
    blocks: page.blocks,
    total_blocks: page.totalBlocks,
    has_more_blocks: page.hasMoreBlocks,
    before_cursor: page.beforeCursor
  };
}

export async function archiveThread(threadId: string) {
  return callCommand("threads.archive", { threadId });
}

export async function restoreThread(threadId: string) {
  return callCommand("threads.restore", { threadId });
}

export async function renameThread(threadId: string, name: string) {
  return callCommand("threads.rename", { threadId, name });
}

export async function getCodexAccountScopeStatus(): Promise<CodexAccountScopeStatus> {
  if (USE_DEMO) return (await import("./demo")).demoCodexAccountScopeStatus();
  return callCommand<CodexAccountScopeStatus>("threads.accountScopeStatus");
}

export async function clearCodexAccountScope(): Promise<CodexAccountScopeResult> {
  if (USE_DEMO) return (await import("./demo")).demoCodexAccountScopeResult();
  return callCommand<CodexAccountScopeResult>("threads.accountScopeClear");
}
