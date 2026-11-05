import { expect, test, type Page } from "@playwright/test";
import { mockApi, mockCommand, observeCommands } from "./fixtures";
import { demoJob, demoUpdateStatus } from "../src/lib/api/demo";

const picker = (page: Page) => page.getByRole("combobox", { name: "当前机器" });
const settings = (page: Page) => page.locator(".side-nav, .mobile-tabs").getByRole("button", { name: "设置", exact: true }).filter({ visible: true }).click();
const status = { ...demoUpdateStatus("macos-tauri"), current_version: "1.2.2", latest_version: "v1.2.2", update_available: false };

test("one update panel follows the current machine and only checks its selected endpoint", async ({ page }) => {
  await mockApi(page, { connection: { configured: true, sshHost: "fixture-host" } });
  const remoteCalls: string[] = [];
  observeCommands(page, (name, args) => { if (name === "remote.invoke") remoteCalls.push(args.request.command); });
  await mockCommand(page, "updates.status", () => status);
  await mockCommand(page, "jobs.detail", args => ({ ...demoJob(args.id), status: "succeeded" }));
  await page.goto("/");
  await settings(page);
  const panel = page.locator(".update-panel");
  await expect(panel).toHaveCount(1);
  await expect(panel).toContainText("本机 App 更新");
  await expect(panel.getByRole("button", { name: /更新至/ })).toHaveCount(0);
  await expect(panel).not.toContainText("已是最新版本");
  await panel.getByRole("button", { name: "检查更新", exact: true }).click();
  await expect(panel.getByRole("status")).toHaveText("已是最新版本");
  expect(remoteCalls).not.toContain("updates.check");
  await picker(page).selectOption("remote");
  await settings(page);
  await expect(panel).toHaveCount(1);
  await expect(panel).toContainText("远程服务更新");
  await expect(panel).not.toContainText("本机 App 更新");
  await expect(panel).not.toContainText("已是最新版本");
  await expect(panel.getByRole("button", { name: "清理更新备份" })).toBeVisible();
  await panel.getByRole("button", { name: "检查更新", exact: true }).focus();
  await page.keyboard.press("Enter");
  await expect(panel.getByRole("status")).toHaveText("已是最新版本");
  expect(remoteCalls.filter(command => command === "updates.check")).toHaveLength(1);
});

test("update errors, new releases and jobs show accurate inline state and block switching", async ({ page }) => {
  await page.setViewportSize({ width: 1000, height: 820 });
  await page.emulateMedia({ colorScheme: "dark", reducedMotion: "reduce" });
  await mockApi(page, { connection: { configured: true, sshHost: "fixture-host" } });
  let current = { ...status, latest_version: null as string | null, update_available: null as boolean | null };
  let fail = true;
  let finish = false;
  const outputReads: boolean[] = [];
  await mockCommand(page, "updates.status", () => current);
  await mockCommand(page, "updates.check", () => {
    if (fail) throw new Error("检查更新失败：fixture offline");
    current = { ...status, latest_version: "v1.2.4", update_available: true };
    return { job_id: "update-fixture", status: current };
  });
  await mockCommand(page, "jobs.detail", args => {
    outputReads.push(args.includeOutput !== false);
    return { ...demoJob(args.id), output: args.includeOutput === false ? "" : "fixture complete", status: finish ? "succeeded" : "running" };
  });
  await page.goto("/");
  await settings(page);
  const panel = page.locator(".update-panel");
  await panel.getByRole("button", { name: "检查更新" }).click();
  await expect(panel.getByRole("alert")).toContainText("fixture offline");
  await expect(panel).not.toContainText("已是最新版本");
  fail = false;
  await panel.getByRole("button", { name: "检查更新" }).click();
  await expect(panel.getByRole("status")).toContainText("正在检查更新");
  await expect(picker(page)).toBeEnabled();
  await expect(panel.getByRole("button", { name: "更新至 1.2.4" })).toBeDisabled();
  expect(outputReads.length).toBeGreaterThan(0);
  expect(outputReads.every(include => !include)).toBe(true);
  finish = true;
  await expect(panel.getByRole("button", { name: "更新至 1.2.4" })).toBeEnabled();
  expect(outputReads.every(include => !include)).toBe(true);
  await expect(picker(page)).toBeEnabled();
  await page.setViewportSize({ width: 680, height: 820 });
  await expect(panel.getByRole("button", { name: "更新至 1.2.4" })).toBeVisible();
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
});

test("a late local update read cannot leak a local version into the remote panel", async ({ page }) => {
  await mockApi(page, { connection: { configured: true, sshHost: "fixture-host" } });
  let remote = false;
  let started = false;
  let release!: () => void;
  const gate = new Promise<void>(resolve => { release = resolve; });
  await mockCommand(page, "remote.select", async (args, next) => { remote = args.request.target === "remote"; return next(); });
  await mockCommand(page, "updates.status", async () => {
    if (remote) return { ...status, current_version: "1.2.0", latest_version: "v1.2.2", update_available: true };
    started = true;
    await gate;
    return { ...status, current_version: "1.1.0" };
  });
  await page.goto("/");
  await settings(page);
  await expect.poll(() => started).toBe(true);
  await picker(page).selectOption("remote");
  await settings(page);
  await expect(page.locator(".update-panel")).toContainText("1.2.0");
  release();
  await expect(page.locator(".update-panel")).not.toContainText("1.1.0");
  await expect(page.getByRole("button", { name: "更新至 1.2.2" })).toBeEnabled();
});
