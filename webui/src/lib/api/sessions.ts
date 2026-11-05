import { callCommand } from "./transport";
export type SessionProvider = "codex" | "grok" | "grokbot" | "claude_code" | "antigravity" | "dsh";
export type SessionOperation = "archive" | "restore" | "delete";
export type SessionBatchRequest = { provider: SessionProvider; operation: SessionOperation; sessionKeys: string[]; includeDescendants?: boolean };
export type SessionBatchItem = { sessionKey: string; id: string; title: string; paths: string[]; bytes: number; allowed: boolean; reason: string | null; fingerprint: string | null; descendants?: string[]; warnings?: string[] };
export type SessionBatchPreview = { provider: SessionProvider; operation: SessionOperation; items: SessionBatchItem[] };
export type SessionBatchExecuteRequest = { provider: SessionProvider; operation: SessionOperation; items: Array<{ sessionKey: string; fingerprint?: string; descendants?: string[] }>; confirmed: boolean; includeDescendants?: boolean };
export type SessionBatchResult = { items: Array<{ sessionKey: string; status: "succeeded" | "blocked" | "failed"; message: string | null; deletedIds?: string[]; residuals?: string[] }> };
export function previewSessionBatch(request: SessionBatchRequest): Promise<SessionBatchPreview> { return callCommand("sessions.bulkPreview", { request }); }
export function executeSessionBatch(request: SessionBatchExecuteRequest): Promise<SessionBatchResult> { return callCommand("sessions.bulkExecute", { request }); }

export type SessionAttachmentRequest = { rootThreadId?: string; provider: SessionProvider; sessionKey: string; messageId: string; attachmentId: string };
export type SessionAttachmentResponse = { mimeType: string; base64: string };
export function readSessionAttachment(request: SessionAttachmentRequest): Promise<SessionAttachmentResponse> { return callCommand("sessions.attachmentRead", { request }); }
