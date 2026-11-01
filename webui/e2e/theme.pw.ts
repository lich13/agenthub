import { expect, test } from "@playwright/test";
import { assertContrast, assertNoOverflow, mockApi } from "./fixtures";

for (const colorScheme of ["light", "dark"] as const) {
  test.describe(colorScheme, () => {
    test.use({ colorScheme, viewport: { width: 1280, height: 820 } });
    test("conversation colors resolve and jobs remain readable", async ({ page }) => {
      await mockApi(page);
      await page.goto("/");
      await page.locator(".thread-item").first().click();
      await expect(page.locator(".conversation-title")).toBeVisible();
      expect(await page.locator(".conversation-shell").evaluate((el) => getComputedStyle(el).getPropertyValue("--text-primary").trim())).not.toBe("");
      await assertContrast(page, ".conversation-title, .chat-meta span, .tool-title");
      await page.locator(".side-nav").getByRole("button", { name: "设置", exact: true }).click();
      await page.locator(".execution-history > summary").click();
      await expect(page.locator(".job-item").first()).toBeVisible();
      await assertContrast(page, ".job-item summary, .tone-success, .tone-warning, .tone-danger");
      await assertNoOverflow(page);
    });
  });
}
