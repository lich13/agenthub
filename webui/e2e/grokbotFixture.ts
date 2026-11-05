import type { ProviderSessionDetail, ProviderSessionSummary } from "../src/types";

export const grokbotRows: ProviderSessionSummary[] = [
  { id: "bot-fixture", sessionKey: "account:bot-fixture", title: "整理项目与确认下一步", cwd: "", path: "/isolated/grokbot/thread.kblob", archived: false, updatedAt: "2026-10-01T00:00:00Z", messageCount: 4, status: "reply_needed", formatVersion: "近期缓存", storageSize: { status: "complete", scope: "file", bytes: 4096 }, canRename: false, canArchive: false, canDelete: false },
  { id: "uncached-fixture", sessionKey: "account:uncached-fixture", title: "尚未缓存在本机的任务", cwd: "", path: "", archived: false, updatedAt: "2026-09-30T00:00:00Z", messageCount: 0, status: "unknown", formatVersion: "未缓存", canRename: false, canArchive: false, canDelete: false },
];

export function grokbotDetail(sessionKey: string): ProviderSessionDetail {
  const summary = grokbotRows.find(row => row.sessionKey === sessionKey)!;
  const events = summary.path ? [
    { id: "bot-user", kind: "user_message", text: "请整理这个项目，先确认需要调整的部分。" },
    { id: "bot-tool", kind: "tool_call", callId: "bot-read", status: "completed", text: "读取项目文件", detail: "Read /workspace/fixture/README.md", result: "项目结构与使用方式" },
    { id: "bot-answer", kind: "assistant_message", text: "已整理项目结构。\n\n现有列表、阅读区和工具记录保持统一的间距。可以先核对需要修改的内容，再继续处理。\n\n- 查看项目文件与现有组件\n- 确认待处理的问题\n- 核对修改后的结果" },
    { id: "bot-question", kind: "interaction", status: "reply_needed", text: "确认修改范围", detail: "先处理界面还是通知？" },
  ] : [];
  return { summary, events, totalEvents: events.length, hasMore: false };
}
