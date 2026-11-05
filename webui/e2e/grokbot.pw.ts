import { expect, test } from "@playwright/test";
import { mockApi } from "./fixtures";

test("Grok Bot keeps cache paths and read-only keyboard menus", async ({ page }) => {
  await mockApi(page);
  await page.addInitScript(() => {
    Object.defineProperty(navigator, "clipboard", { value: { writeText: async (text: string) => { (window as any).__copied = text; } }, configurable: true });
  });
  await page.goto("/");
  await page.locator(".side-nav").getByRole("button", { name: "Grok Bot", exact: true }).click();
  const row = page.locator(".provider-session").first();
  await row.click();
  await expect(page.locator(".provider-events")).toContainText("已整理项目结构");
  await row.press("Shift+F10");
  const menu = page.locator(".context-task-menu");
  await expect(menu.getByRole("menuitem")).toHaveText(["复制 ID", "复制路径", "在 Finder 中显示"]);
  await menu.getByRole("menuitem", { name: "复制路径", exact: true }).click();
  await expect.poll(() => page.evaluate(() => (window as any).__copied)).toBe("/isolated/grokbot/thread.kblob");
  await page.locator(".provider-session").nth(1).click({ button: "right" });
  await expect(menu.getByRole("menuitem", { name: "复制路径", exact: true })).toBeDisabled();
  await expect(menu.getByRole("menuitem", { name: "在 Finder 中显示", exact: true })).toBeDisabled();
  await page.keyboard.press("Escape");
  await expect(menu).toHaveCount(0);
});
