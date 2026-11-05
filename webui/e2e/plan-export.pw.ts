import { expect, test } from "@playwright/test";
import { mockApi, mockCommand, observeCommands } from "./fixtures";
import * as demo from "../src/lib/api/demo";

test("Codex Plan copies and saves cleaned Markdown with its title through the native command", async ({ page }) => {
  await mockApi(page);
  const saves: unknown[] = [];
  const downloads: string[] = [];
  page.on("download", download => downloads.push(download.suggestedFilename()));
  await mockCommand(page, "plans.save", args => { saves.push(args); return { filename: args.request.filename }; });
  const markdown = "# Release: Notes/Review?\n\n- **Preserve** formatting\n- [Guide](https://example.com/guide)";
  await page.addInitScript(() => {
    Object.defineProperty(navigator, "clipboard", { value: { writeText: async (value: string) => { document.documentElement.dataset.copied = value; } } });
  });
  await mockCommand(page, "threads.detail", args => {
    const detail = demo.demoThreadDetail("019e95a0-demo");
    detail.blocks = [{ id: "plan", role: "assistant", kind: "plan", text: `<proposed_plan>\n<citation_entries># Internal title</citation_entries>\n${markdown}\n<rollout_ids>internal-id</rollout_ids>\n</proposed_plan>`, questions: [] }];
    return detail;
  });
  await page.goto("/");
  await page.locator(".thread-item").filter({ hasText: "Plan Mode 修复" }).click();
  const plan = page.locator(".plan-cell");
  await expect(plan).toContainText("Preserve");
  await plan.getByRole("button", { name: "复制计划" }).click();
  await expect(page.locator("html")).toHaveAttribute("data-copied", markdown);
  await expect(plan.getByRole("button", { name: "已复制计划" })).toBeVisible();
  await plan.getByRole("button", { name: "下载计划 Markdown" }).click();
  await expect(plan.getByRole("status")).toHaveText("已保存到下载文件夹");
  expect(saves).toEqual([{ request: { filename: "Release Notes Review.md", markdown } }]);
  expect(downloads).toEqual([]);
});

test("a remote Grok Plan saves on the local machine and reports clipboard failure nearby", async ({ page }) => {
  await mockApi(page, { connection: { target: "remote", revision: 3, configured: true, sshHost: "fixture-host" } });
  const saves: unknown[] = [];
  const remoteCommands: string[] = [];
  observeCommands(page, (command, args) => { if (command === "remote.invoke") remoteCommands.push(args.request.command); });
  await mockCommand(page, "plans.save", args => { saves.push(args); return { filename: args.request.filename }; });
  await page.addInitScript(() => {
    Object.defineProperty(navigator, "clipboard", { value: { writeText: async () => { throw new Error("denied"); } } });
  });
  const markdown = "# Grok Plan\n\nRun **one** check.";
  await mockCommand(page, "grok.detail", args => ({
    summary: { id: "grok-fixture", title: "Grok fixture", cwd: "/isolated/workspace", status: "recent" },
    events: [{ kind: "plan", text: `${markdown}\n<oai-mem-citation>internal-id</oai-mem-citation>` }, { kind: "agent_message_chunk", text: "Done" }]
  }));
  await page.goto("/");
  await page.locator(".side-nav").getByRole("button", { name: "Grok Build", exact: true }).click();
  const plan = page.locator(".provider-event.plan");
  await plan.getByRole("button", { name: "复制计划" }).click();
  await expect(plan.getByRole("status")).toContainText("复制失败");
  await plan.getByRole("button", { name: "下载计划 Markdown" }).click();
  await expect(plan.getByRole("status")).toHaveText("已保存到下载文件夹");
  expect(saves).toEqual([{ request: { filename: "Grok Plan.md", markdown } }]);
  expect(remoteCommands).toContain("grok.detail");
  expect(remoteCommands).not.toContain("plans.save");
  await expect(page.getByRole("combobox", { name: "当前机器" })).toHaveValue("remote");
});

test("a failed Plan save shows inline feedback and can be retried", async ({ page }) => {
  await mockApi(page);
  let attempts = 0;
  await mockCommand(page, "plans.save", args => {
    if (++attempts === 1) throw new Error("Downloads is not writable");
    return { filename: args.request.filename };
  });
  await page.goto("/");
  await page.locator(".thread-item").filter({ hasText: "Plan Mode 修复" }).click();
  const plan = page.locator(".plan-cell");
  const save = plan.getByRole("button", { name: "下载计划 Markdown" });
  await save.click();
  await expect(plan.getByRole("status")).toHaveText("保存失败，请重试。");
  await expect(save).toBeEnabled();
  await save.click();
  await expect(plan.getByRole("status")).toHaveText("已保存到下载文件夹");
  expect(attempts).toBe(2);
});

test("a pending native Plan save prevents duplicate clicks and waits before reporting success", async ({ page }) => {
  await mockApi(page);
  let finish!: () => void;
  const pending = new Promise<void>(resolve => { finish = resolve; });
  let attempts = 0;
  await mockCommand(page, "plans.save", async args => {
    attempts++;
    await pending;
    return { filename: args.request.filename };
  });
  await page.goto("/");
  await page.locator(".thread-item").filter({ hasText: "Plan Mode 修复" }).click();
  const plan = page.locator(".plan-cell");
  const save = plan.getByRole("button", { name: "下载计划 Markdown" });
  try {
    await save.dblclick();
    await expect.poll(() => attempts).toBe(1);
    await expect(save).toBeDisabled();
    await expect(plan.getByRole("status")).toHaveText("");
  } finally { finish(); }
  await expect(plan.getByRole("status")).toHaveText("已保存到下载文件夹");
  await expect(save).toBeEnabled();
  expect(attempts).toBe(1);
});

test("an empty Grok Plan has no copy or download action", async ({ page }) => {
  await mockApi(page);
  await mockCommand(page, "grok.detail", args => ({
    summary: { id: "grok-fixture", title: "Grok fixture", cwd: "/isolated/workspace", status: "recent" },
    events: [{ kind: "plan", text: "<citation_entries>internal-only</citation_entries>" }]
  }));
  await page.goto("/");
  await page.locator(".side-nav").getByRole("button", { name: "Grok Build", exact: true }).click();
  await expect(page.locator(".provider-event.plan button")).toHaveCount(0);
});
