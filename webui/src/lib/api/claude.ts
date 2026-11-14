import type { ClaudeDeletePreview, ClaudeDeleteRequest, ClaudeDeleteResult, ClaudeOwnershipDiagnostics, ClaudeSessionDetail, ClaudeSessionSummary } from "../../types";
import { connectionSnapshot, invokeNative } from "../runtime";
import { callCommand } from "./transport";

export function listClaudeSessions(query?: { q?: string; limit?: number }): Promise<ClaudeSessionSummary[]> { return callCommand("claude.list", query); }
export function getClaudeSession(sessionKey: string, before?: string, agentId?: string): Promise<ClaudeSessionDetail> { return callCommand("claude.detail", { sessionKey, limit: 160, before, ...(agentId ? { agentId } : {}) }); }
export function renameClaudeSession(sessionKey: string, title: string): Promise<ClaudeSessionSummary> { return callCommand("claude.rename", { sessionKey, title }); }
export function previewClaudeSessionDelete(sessionKey: string): Promise<ClaudeDeletePreview> { return callCommand("claude.deletePreview", { sessionKey }); }
export async function getClaudeOwnershipDiagnostics(sessionKey: string): Promise<ClaudeOwnershipDiagnostics> {
  const connection = connectionSnapshot();
  if (connection.target !== "local" || connection.changing) throw new Error("进程归属诊断仅支持本机会话");
  const result = await invokeNative<ClaudeOwnershipDiagnostics>("claude.ownershipDiagnostics", { sessionKey });
  if (connectionSnapshot().revision !== connection.revision) throw new Error("连接已变化，请重新诊断");
  return result;
}
export function deleteClaudeSession(request: ClaudeDeleteRequest): Promise<ClaudeDeleteResult> { return callCommand("claude.deleteExecute", { request }); }
