import { expect, test } from "@playwright/test";
import { assertContrast, assertNoOverflow, mockApi, mockCommand, observeCommands } from "./fixtures";

test("Probe saves Bark and restricted recovery independently and retains drafts on failure", async ({ page }) => {
  await mockApi(page);
  const saved: Record<string, any>[] = [];
  observeCommands(page, (command, args) => { if (command === "probe.settings.save") saved.push(args.settings); });
  await page.goto("/");
  await page.locator(".side-nav").getByRole("button", { name: "Probe", exact: true }).click();
  await page.getByLabel("启用 Bark", { exact: true }).uncheck();
  await page.getByRole("button", { name: "保存设置", exact: true }).click();
  await expect.poll(() => saved.length).toBe(1);
  expect(saved[0].probe.notifications.enabled).toBe(false);
  await expect(page.locator(".form-success").first()).toHaveText("设置已保存");
  await expect(page.getByLabel("启用 Bark", { exact: true })).not.toBeChecked();
  await expect(page.getByRole("button", { name: "保存设置", exact: true })).toBeEnabled();
  await page.getByLabel("启用 Bark", { exact: true }).check();
  await page.getByRole("button", { name: "保存设置", exact: true }).click();
  await expect.poll(() => saved.length).toBe(2);
  expect(saved[1].probe.notifications.enabled).toBe(true);
  await expect(page.getByRole("button", { name: "保存设置", exact: true })).toBeEnabled();
  await mockCommand(page, "probe.settings.save", args => Promise.reject(new Error("Fixture save failed")));
  await page.getByLabel("主机标签", { exact: true }).fill("retained-draft");
  await page.getByRole("button", { name: "保存设置", exact: true }).click();
  await expect(page.locator(".form-error").first()).toContainText("Fixture save failed");
  expect(saved).toHaveLength(3);
  expect(saved[2].codex.host_label).toBe("retained-draft");
  await expect(page.getByLabel("主机标签", { exact: true })).toHaveValue("retained-draft");
  await assertContrast(page, ".form-error");
});

test("Probe rejects duplicate save events before pending state renders", async ({ page }) => {
  await mockApi(page);
  let saves = 0;
  let releaseSave!: () => void;
  const responseGate = new Promise<void>(resolve => { releaseSave = resolve; });
  await mockCommand(page, "probe.settings.save", async (args, next) => {
    saves++;
    await responseGate;
    return next();
  });
  await page.goto("/");
  await page.locator(".side-nav").getByRole("button", { name: "Probe", exact: true }).click();
  const save = page.getByRole("button", { name: "保存设置", exact: true });
  await expect(save).toBeEnabled();
  // Dispatch both events in one turn, before React Query's pending notification.
  await save.evaluate(button => { (button as HTMLButtonElement).click(); (button as HTMLButtonElement).click(); });
  await expect.poll(() => saves).toBe(1);
  await expect(save).toBeDisabled();
  await expect(page.getByLabel("主机标签", { exact: true })).toBeDisabled();
  releaseSave();
  await expect(page.locator(".form-success").first()).toHaveText("设置已保存");
  await expect(save).toBeEnabled();
  expect(saves).toBe(1);
});

test("unified cross-platform cleanup requires preview, explicit confirmation, and visible failure", async ({ page }) => {
  await mockApi(page);
  const executed: Array<{
    confirmed: boolean;
    scopes?: string[];
    targets?: Array<{ scope: string; id: string; fingerprint?: string | null }>;
  }> = [];
  await mockCommand(page, "cleanup.unifiedExecute", args => {
    executed.push(args.request);
    return Promise.reject(new Error("Fixture count changed; run a new preview"));
  });
  await page.goto("/");
  await page.locator(".side-nav").getByRole("button", { name: "设置", exact: true }).click();
  await page.getByRole("tab", { name: "更新与维护", exact: true }).click();
  const cleanup = page.locator(".panel").filter({ has: page.getByText("全平台线程清理", { exact: true }) });
  await expect(cleanup.getByRole("button", { name: "一键清理所选平台", exact: true })).toBeDisabled();
  await cleanup.getByRole("button", { name: "全平台扫描预览", exact: true }).click();
  await expect(cleanup.getByRole("button", { name: "一键清理所选平台", exact: true })).toBeEnabled();
  await cleanup.getByLabel("选择 Antigravity 已归档线程", { exact: true }).uncheck();
  await cleanup.getByRole("button", { name: "一键清理所选平台", exact: true }).click();
  expect(executed).toHaveLength(0);
  await cleanup.getByRole("button", { name: "取消", exact: true }).click();
  expect(executed).toHaveLength(0);
  await cleanup.getByRole("button", { name: "一键清理所选平台", exact: true }).click();
  await cleanup.getByRole("button", { name: "确认清理所选", exact: true }).click();
  await expect(cleanup.locator(".form-error")).toContainText("Fixture count changed");
  expect(executed).toHaveLength(1);
  expect(executed[0].confirmed).toBe(true);
  expect(executed[0].scopes).not.toContain("antigravity_archived");
  expect(executed[0].scopes?.length).toBeGreaterThan(0);
  expect(executed[0].targets?.length).toBeGreaterThan(0);
  expect(executed[0].targets?.every((t) => t.scope !== "antigravity_archived" && Boolean(t.fingerprint))).toBe(true);
  await assertContrast(page, ".form-error, .status-chip, .danger-button");
});

test("native log maintenance is retired on every settings surface", async ({ page }) => {
  const calls = await mockApi(page);
  await page.goto("/");
  await page.locator(".side-nav").getByRole("button", { name: "设置", exact: true }).click();
  await page.getByRole("tab", { name: "更新与维护", exact: true }).click();
  await expect(page.getByText("Codex 日志库维护", { exact: true })).toHaveCount(0);
  expect(calls.some(name => name.startsWith("probe.logsDb"))).toBe(false);
});

test("copy commands use the selected task and long load errors stay readable", async ({ page }) => {
  await mockApi(page);
  await page.addInitScript(() => {
    Object.defineProperty(navigator, "clipboard", { value: { writeText: async (text: string) => { document.documentElement.dataset.copied = text; } } });
  });
  await page.goto("/");
  await page.locator(".thread-item").first().click();
  await page.locator(".chat-row.assistant .copy-reply").last().click();
  await expect(page.locator("html")).toHaveAttribute("data-copied", /状态正常/);
  await expect(page.getByLabel("已复制回复", { exact: true })).toBeVisible();
  const menu = page.getByLabel("任务操作", { exact: true });
  await menu.click();
  await page.getByRole("button", { name: "复制 ID", exact: true }).click();
  const id = await page.locator("html").getAttribute("data-copied");
  expect(id).toBeTruthy();
  await expect(page.getByText("已复制", { exact: true })).toBeVisible();
  await menu.click();
  await page.getByRole("button", { name: "复制恢复命令", exact: true }).click();
  await expect(page.locator("html")).toHaveAttribute("data-copied", `codex resume ${id}`);
  await page.locator(".side-nav").getByRole("button", { name: "Grok Build", exact: true }).click();
  await page.locator(".provider-session").click();
  await page.getByLabel("Grok 任务操作").click();
  await page.getByRole("button", { name: "复制线程 ID", exact: true }).click();
  await expect(page.locator("html")).toHaveAttribute("data-copied", "grok-fixture");
  await expect(page.getByText("已复制线程 ID", { exact: true })).toBeVisible();
  await page.setViewportSize({ width: 320, height: 844 });
  await mockCommand(page, "grok.list", args => Promise.reject(new Error("Fixture load failure: " + "long-path/".repeat(40))));
  await page.locator(".mobile-tabs").getByRole("button", { name: "Codex", exact: true }).click();
  await page.locator(".mobile-tabs").getByRole("button", { name: "Grok Build", exact: true }).click();
  await expect(page.locator(".form-error").first()).toContainText("Fixture load failure");
  await assertContrast(page, ".form-error");
  await assertNoOverflow(page);
});

test("provider copy reports clipboard failures", async ({ page }) => {
  await mockApi(page);
  await page.addInitScript(() => {
    Object.defineProperty(navigator, "clipboard", { value: { writeText: async () => { throw new Error("denied"); } } });
  });
  await page.goto("/");
  await page.locator(".side-nav").getByRole("button", { name: "Grok Build", exact: true }).click();
  await page.locator(".provider-session").click();
  await page.getByLabel("Grok 任务操作").click();
  await page.getByRole("button", { name: "复制线程 ID", exact: true }).click();
  await expect(page.getByRole("status")).toHaveText("复制失败：denied");
});
