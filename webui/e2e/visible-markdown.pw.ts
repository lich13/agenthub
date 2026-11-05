import { expect, test } from "@playwright/test";
import { mockApi, mockCommand } from "./fixtures";
import * as demo from "../src/lib/api/demo";

const metadata = "<oai-mem-citation><citation_entries>internal-entry</citation_entries><rollout_ids>internal-id</rollout_ids></oai-mem-citation>";

for (const provider of ["codex", "grok"]) {
  test(`${provider} hides metadata and copies the visible Markdown`, async ({ page }) => {
    await mockApi(page);
    const markdown = "**Visible reply**\n\nRead MEMORY.md.\n\n```xml\n<rollout_ids>literal example</rollout_ids>\n```";
    await page.addInitScript(() => {
      Object.defineProperty(navigator, "clipboard", { value: { writeText: async (value: string) => { document.documentElement.dataset.copied = value; } } });
    });
    await mockCommand(page, `${provider === "codex" ? "threads" : provider}.detail`, args => {
      if (provider === "codex") {
        const detail = demo.demoThreadDetail("019e95a0-demo");
        detail.blocks = [{ id: "reply", role: "assistant", kind: "message", text: `${markdown}\n${metadata}`, questions: [] }];
        return detail;
      }
      return { summary: {}, events: [{ kind: provider === "grok" ? "agent_message_chunk" : "assistant_message", text: `${markdown}\n${metadata}` }] };
    });
    await page.goto("/");
    if (provider === "codex") await page.locator(".thread-item").filter({ hasText: "Plan Mode 修复" }).click();
    else await page.locator(".side-nav").getByRole("button", { name: "Grok Build", exact: true }).click();
    const reply = page.locator(".markdown-content");
    await expect(reply).toContainText("Visible reply");
    await expect(reply).not.toContainText("internal-entry");
    await expect(reply).not.toContainText("internal-id");
    await expect(reply).toContainText("literal example");
    const copy = page.getByRole("button", { name: "复制回复", exact: true });
    await copy.focus();
    await page.keyboard.press("Enter");
    await expect(page.locator("html")).toHaveAttribute("data-copied", markdown);
    await expect(page.getByTitle("复制代码")).toBeVisible();
  });
}

test("instruction choices survive streaming, theme and mobile list transitions", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.emulateMedia({ colorScheme: "dark", reducedMotion: "reduce" });
  await mockApi(page);
  let text = `Visible.\n\n## /workspace/AGENTS.md\n${"Instruction line.\n".repeat(120)}\n## Result\nDone.${metadata}`;
  await mockCommand(page, "grok.detail", args => ({ summary: {}, events: [{ kind: "agent_message_chunk", text }] }));
  await page.goto("/");
  await page.locator(".mobile-tabs").getByRole("button", { name: "Grok Build", exact: true }).click();
  await page.locator(".provider-session").click();
  const file = page.locator("details.instruction-file");
  await expect(file).not.toHaveAttribute("open");
  const summary = file.locator(":scope > summary");
  await expect(summary).toContainText("AGENTS.md");
  await expect(summary).not.toContainText("/workspace");
  await summary.focus();
  await page.keyboard.press("Enter");
  await expect(file).toHaveAttribute("open", "");
  text += "\nAppended result.";
  await expect(page.getByText("Appended result.", { exact: false })).toBeVisible({ timeout: 3500 });
  await expect(file).toHaveAttribute("open", "");
  await page.getByTitle("返回任务列表").click();
  await page.locator(".provider-session").click();
  await expect(file).toHaveAttribute("open", "");
  await page.emulateMedia({ colorScheme: "light" });
  await expect(file).toHaveAttribute("open", "");
  await summary.click();
  text += "\nOne more result.";
  await expect(page.getByText("One more result.", { exact: false })).toBeVisible({ timeout: 3500 });
  await expect(file).not.toHaveAttribute("open");
  await expect.poll(() => page.evaluate(() => document.documentElement.scrollHeight - innerHeight)).toBeLessThanOrEqual(1);
});

test("running and failed AGENTS tools stay folded inside mixed Grok activity", async ({ page }) => {
  await mockApi(page);
  const events = [
    { kind: "tool_call", callId: "agents", text: "Read /workspace/AGENTS.md", status: "in_progress", detail: "File contents" },
    { kind: "tool_call", callId: "ordinary", text: "Execute pwd", status: "failed", detail: "Command failure" }
  ];
  await mockCommand(page, "grok.detail", args => ({ summary: {}, events }));
  await page.goto("/");
  await page.locator(".side-nav").getByRole("button", { name: "Grok Build", exact: true }).click();
  const group = page.locator("details.execution-group");
  const commands = group.locator("details.execution-command");
  await expect(group).toHaveAttribute("open", "");
  await expect(commands.nth(0)).not.toHaveAttribute("open");
  await expect(commands.nth(0).locator("summary")).toContainText("AGENTS.md");
  await expect(commands.nth(0).locator("summary .thread-running-indicator")).toBeVisible();
  await expect(commands.nth(1)).toHaveAttribute("open", "");
  await commands.nth(0).locator("summary").click();
  events[0].status = "failed";
  events[0].detail = "Updated file contents";
  await expect(commands.nth(0)).toContainText("Updated file contents", { timeout: 3500 });
  await expect(commands.nth(0)).toHaveAttribute("open", "");
});

for (const provider of ["codex", "grok"]) {
  test(`${provider} remote file Markdown copies the source path without navigation`, async ({ page }) => {
    await mockApi(page, { connection: { target: "remote", revision: 1, sshHost: "fixture-host", configured: true } });
    await page.addInitScript(() => {
      Object.defineProperty(navigator, "clipboard", { value: { writeText: async (value: string) => { document.documentElement.dataset.copiedPath = value; } } });
    });
    const markdown = "[文件](file:///isolated/workspace/%E6%96%87%E4%BB%B6%20A.md#L12-C4) and [relative](docs/notes.md)";
    if (provider === "codex") {
      await mockCommand(page, "threads.detail", args => {
        const detail = demo.demoThreadDetail("019e95a0-demo");
        detail.summary.cwd = "/isolated/workspace";
        detail.blocks = [{ id: "path-reply", role: "assistant", kind: "message", text: markdown, questions: [] }];
        return detail;
      });
    } else {
      await mockCommand(page, `${provider}.detail`, args => ({
        summary: provider === "grok" ? { cwd: "/isolated/workspace" } : { cwd: "/isolated/workspace" },
        events: [{ kind: provider === "grok" ? "agent_message_chunk" : "assistant_message", text: markdown }]
      }));
    }
    await page.goto("/");
    if (provider === "codex") await page.locator(".thread-item").filter({ hasText: "Plan Mode 修复" }).click();
    else await page.locator(".side-nav").getByRole("button", { name: "Grok Build", exact: true }).click();
    const first = page.locator(".file-path-label").first();
    await expect(first).toBeVisible();
    await first.click();
    await expect(page.locator("html")).toHaveAttribute("data-copied-path", "/isolated/workspace/文件 A.md");
    await expect(page).toHaveURL(/\/$/);
    await page.getByRole("button", { name: "复制文件路径", exact: true }).last().click();
    await expect(page.locator("html")).toHaveAttribute("data-copied-path", "/isolated/workspace/docs/notes.md");
  });
}
