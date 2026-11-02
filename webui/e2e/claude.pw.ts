import { expect, test } from "@playwright/test";
import { mockApi, mockCommand } from "./fixtures";
import type { ClaudeHistoryEvent, ClaudeSessionSummary } from "../src/types";

const summary: ClaudeSessionSummary = {
  id: "native-claude", sessionKey: "claude:fixture", title: "Claude fixture", cwd: "/isolated/workspace",
  path: "/isolated/projects/native.jsonl", status: "recent", formatVersion: "2.1.284", messageCount: 3,
  canRename: true, canDelete: true, updatedAt: "2026-09-29T00:00:00Z"
};
const events: ClaudeHistoryEvent[] = [
  { id: "u1", kind: "user_message", text: "# Literal request\n  Keep spaces" },
  { id: "agents", kind: "user_message", text: "# AGENTS.md instructions\n\n<INSTRUCTIONS>\n# Example guidelines\nRead carefully.\n</INSTRUCTIONS>" },
  { id: "a1", kind: "assistant_message", text: "Before tools" },
  { id: "read", kind: "tool_call", role: "Read", callId: "read", status: "completed", detail: '{"file_path":"AGENTS.md"}', result: "Instructions" },
  { id: "bash", kind: "tool_call", role: "Bash", callId: "bash", status: "completed", detail: '{"command":"printf example"}', result: "example" },
  { id: "plan", kind: "plan", text: "# Example plan\n\n- Preserve native files" },
  { id: "a2", kind: "assistant_message", text: "After tools<oai-mem-citation>internal marker</oai-mem-citation>" }
];

async function openClaude(page: import("@playwright/test").Page, mobile = false) {
  await page.goto("/");
  await page.locator(mobile ? ".mobile-tabs" : ".side-nav").getByRole("button", { name: "Claude Code", exact: true }).click();
  if (mobile) await page.locator(".provider-session").click();
}

for (const mobile of [false, true]) {
  test(`Claude native timeline, controls and disclosure ${mobile ? "mobile" : "desktop"}`, async ({ page }) => {
    await page.setViewportSize(mobile ? { width: 390, height: 844 } : { width: 1280, height: 900 });
    await page.emulateMedia({ colorScheme: mobile ? "light" : "dark", reducedMotion: "reduce" });
    await mockApi(page);
    await mockCommand(page, "claude.list", () => [summary]);
    await mockCommand(page, "claude.detail", () => ({ summary, events, hasMore: false, totalEvents: events.length }));
    await page.addInitScript(() => {
      Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText: async (text: string) => { (window as any).copied = text; } } });
    });
    let saved: any;
    await mockCommand(page, "plans.save", args => { saved = args; return { path: "/isolated/Downloads/Example plan.md" }; });
    await openClaude(page, mobile);
    await expect(page.locator(".user-message-bubble").filter({ hasText: "# Literal request" })).toBeVisible();
    await expect(page.locator(".provider-events")).not.toContainText("internal marker");
    const group = page.locator(".execution-group");
    await expect(group).not.toHaveAttribute("open", "");
    await expect(group.locator(":scope > summary")).toContainText("已读取文件、运行命令");
    await group.locator(":scope > summary").focus(); await page.keyboard.press("Enter");
    await expect(group.locator(".execution-command")).toHaveCount(2);
    const command = group.locator(".execution-command").filter({ hasText: "printf example" });
    await command.locator("summary").click(); await expect(command).toHaveAttribute("open", "");
    await page.emulateMedia({ colorScheme: mobile ? "dark" : "light" });
    if (mobile) await page.getByTitle("返回任务列表").click();
    await page.getByTitle("刷新任务").click();
    if (mobile) await page.locator(".provider-session").click();
    await expect(command).toHaveAttribute("open", "");
    await page.getByRole("button", { name: "复制计划", exact: true }).click();
    await expect.poll(() => page.evaluate(() => (window as any).copied)).toBe(events[5].text);
    await page.getByRole("button", { name: "下载计划 Markdown" }).click();
    await expect.poll(() => JSON.stringify(saved)).toContain("Example plan.md");
    await expect.poll(() => page.evaluate(() => document.documentElement.scrollHeight - innerHeight)).toBeLessThanOrEqual(1);
  });
}

test("Claude rename, blocked records and missing native data", async ({ page }) => {
  await mockApi(page);
  let title = summary.title;
  let empty = false;
  await mockCommand(page, "claude.list", () => empty ? [] : [{ ...summary, title }, { ...summary, sessionKey: "claude:blocked", title: "Unknown format", canRename: false, canDelete: false, renameBlockReason: "格式未确认", deleteBlockReason: "格式未确认" }]);
  await mockCommand(page, "claude.detail", () => ({ summary: { ...summary, title }, events: [], hasMore: false, totalEvents: 0 }));
  await mockCommand(page, "claude.rename", args => { title = args.title; return { ...summary, title }; });
  await openClaude(page);
  await page.locator(".provider-session").filter({ hasText: summary.title }).dblclick();
  const input = page.getByRole("textbox", { name: "线程名称" });
  await input.fill("Claude renamed"); await input.press("Enter");
  await expect(page.locator(".conversation-title")).toHaveText("Claude renamed");
  await page.locator(".provider-session").filter({ hasText: "Unknown format" }).dblclick();
  await expect(input).toHaveCount(0);
  empty = true; await page.getByTitle("刷新任务").click();
  await expect(page.getByText("未发现 Claude Code 会话")).toBeVisible();
});

test("Claude paging retains the visible anchor and command expansion", async ({ page }) => {
  await mockApi(page);
  const history: ClaudeHistoryEvent[] = Array.from({ length: 80 }, (_, i) => i % 2 === 0
    ? { id: `text-${i}`, kind: "assistant_message", text: `Claude history ${i}\n\n${"Readable history fixture. ".repeat(8)}` }
    : { id: `tool-${i}`, kind: "tool_call", role: "Bash", status: "completed", detail: JSON.stringify({ command: `printf example-${i}` }), result: `ok ${"tool output. ".repeat(8)}` });
  const requests: Array<{ before?: string }> = [];
  let release!: () => void;
  const gate = new Promise<void>(resolve => { release = resolve; });
  await mockCommand(page, "claude.list", () => [summary]);
  await mockCommand(page, "claude.detail", async args => {
    requests.push({ before: args.before });
    if (args.before) await gate;
    return { summary, events: args.before ? history.slice(0, 40) : history.slice(40), hasMore: !args.before, beforeCursor: args.before ? null : "text-40", totalEvents: 80 };
  });
  await openClaude(page);
  const stream = page.locator(".provider-events");
  await expect.poll(() => stream.evaluate(node => node.scrollHeight > node.clientHeight)).toBe(true);
  await expect.poll(() => stream.evaluate(node => node.scrollTop >= node.scrollHeight - node.clientHeight - 1)).toBe(true);
  const initial = await stream.evaluate(node => ({ top: node.scrollTop, height: node.scrollHeight, client: node.clientHeight }));
  expect(initial.height).toBeGreaterThan(initial.client);
  expect(initial.top).toBeGreaterThanOrEqual(initial.height - initial.client - 1);
  await expect(page.getByText("Claude history 0", { exact: true })).toHaveCount(0);
  const anchor = page.getByText("Claude history 42", { exact: true });
  await stream.evaluate(node => {
    node.scrollTop = Math.max(201, node.scrollHeight - node.clientHeight);
    node.dispatchEvent(new Event("scroll", { bubbles: true }));
    node.scrollTop = 200;
    node.dispatchEvent(new Event("scroll", { bubbles: true }));
  });
  await expect.poll(() => requests.filter(request => request.before).length).toBe(1);
  await expect(stream.locator(".history-loading")).toContainText("正在加载较早消息");
  await expect(anchor).toBeVisible();
  const top = await anchor.evaluate(node => node.getBoundingClientRect().top);
  release();
  await expect(stream.locator(".execution-group")).toHaveCount(40);
  await expect(stream.locator(".history-loading")).toHaveCount(0);
  expect(Math.abs(await anchor.evaluate(node => node.getBoundingClientRect().top) - top)).toBeLessThanOrEqual(2);
  expect(requests.filter(request => request.before)).toEqual([{ before: "text-40" }]);
  await expect(page.getByText("Claude history 0", { exact: true })).toBeVisible();
  await page.getByTitle("刷新任务").click();
  await expect(stream.locator(".execution-group")).toHaveCount(40);
  await expect(page.getByText("Claude history 0", { exact: true })).toBeVisible();
});

test("Claude attachments and disclosure cache stay isolated between machines", async ({ page }) => {
  await mockApi(page, { connection: { configured: true, baseUrl: "https://api.example.com/agenthub/" } });
  let machine = "local";
  const reads: string[] = [];
  await mockCommand(page, "remote.select", async (args, next) => { const result = await next(); machine = args.request.target; return result; });
  await mockCommand(page, "claude.list", () => [{ ...summary, title: `${machine} Claude` }]);
  await mockCommand(page, "claude.detail", () => ({ summary: { ...summary, title: `${machine} Claude` }, hasMore: false, totalEvents: 2, events: [
    { id: "same-message", kind: "user_message", userMessage: { id: "same-message", text: `${machine} image`, attachments: [{ id: "same-image", name: "sample.png", kind: "image" }] } },
    { id: "same-call", kind: "tool_call", role: "Bash", status: "completed", detail: '{"command":"pwd"}', result: machine }
  ] }));
  await mockCommand(page, "sessions.attachmentRead", args => {
    expect(args.request.provider).toBe("claude_code"); expect(args.request.sessionKey).toBe(summary.sessionKey); expect(args.request).not.toHaveProperty("path");
    reads.push(machine); return { mimeType: "image/png", base64: "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aN1cAAAAASUVORK5CYII=" };
  });
  await openClaude(page);
  const image = page.locator(".attachment-thumbnail img");
  await expect.poll(() => image.evaluate(el => (el as HTMLImageElement).naturalWidth)).toBe(1);
  const button = page.getByRole("button", { name: "预览图片：sample.png" });
  await button.focus(); await page.keyboard.press("Enter"); await expect(page.getByRole("dialog")).toBeVisible();
  await page.keyboard.press("Escape"); await expect(button).toBeFocused();
  await page.locator(".execution-group > summary").click();
  await page.getByRole("combobox", { name: "当前机器" }).selectOption("remote");
  await expect(page.locator(".conversation-title")).toHaveText("remote Claude");
  await expect(page.locator(".user-message-bubble")).toHaveText("remote image");
  await expect(page.locator(".execution-group")).not.toHaveAttribute("open", "");
  await expect.poll(() => reads).toEqual(["local", "remote"]);
  await page.getByRole("combobox", { name: "当前机器" }).selectOption("local");
  await expect(page.locator(".execution-group")).toHaveAttribute("open", "");
  await expect(page.locator(".conversation-title")).toHaveText("local Claude");
});
