import { readFileSync } from "node:fs";
import { expect, test } from "@playwright/test";
import { assertNoOverflow, mockApi, mockCommand } from "./fixtures";

test("Finder Command-click batch archive confirms explicit keys, retains failed selections, and requires another preview", async ({ page }) => {
  await mockApi(page);
  let previews = 0;
  const executed: any[] = [];
  await mockCommand(page, "sessions.bulkPreview", args => {
    previews++;
    const request = args.request;
    return { ...request, items: request.sessionKeys.map((key: string) => ({ sessionKey: key, id: key, title: "Selected fixture", paths: [], bytes: 0, allowed: true, reason: null, fingerprint: `${previews}:${key}` })) };
  });
  await mockCommand(page, "sessions.bulkExecute", args => {
    const request = args.request;
    executed.push(request);
    return { items: request.items.map((item: any, index: number) => ({ sessionKey: item.sessionKey, status: index === 0 ? "succeeded" : "blocked", message: index === 0 ? null : "Fixture changed; preview again" })) };
  });
  await page.goto("/");
  await expect(page.getByRole("button", { name: "多选线程", exact: true })).toHaveCount(0);
  await expect(page.locator(".session-checkbox")).toHaveCount(0);

  const rows = page.locator(".thread-item");
  await rows.nth(0).click({ modifiers: ["Meta"] });
  await rows.nth(1).click({ modifiers: ["Meta"] });
  await expect(rows.nth(0)).toHaveClass(/\bmulti-selected\b/);
  await expect(rows.nth(1)).toHaveClass(/\bmulti-selected\b/);
  await expect(page.getByText("已选 2", { exact: true })).toBeVisible();

  await page.getByRole("button", { name: "归档所选线程", exact: true }).click();
  const dialog = page.getByRole("dialog");
  await expect(dialog).toBeVisible();
  await page.keyboard.press("Meta+a");
  await expect(page.getByText("已选 2", { exact: true })).toBeVisible();
  expect(executed).toHaveLength(0);
  await dialog.getByRole("button", { name: "取消", exact: true }).click();
  await expect(page.getByRole("button", { name: "归档所选线程", exact: true })).toBeFocused();
  await page.getByRole("button", { name: "归档所选线程", exact: true }).click();
  await dialog.getByRole("button", { name: "确认归档 2 项", exact: true }).click();
  await expect(page.getByText("已完成 1 项，未完成 1 项", { exact: true })).toBeVisible();
  await expect(page.getByText("已选 1", { exact: true })).toBeVisible();
  expect(executed).toHaveLength(1);
  expect(executed[0].provider).toBe("codex");
  expect(executed[0].items).toHaveLength(2);
  expect(executed[0].items.every((item: any) => typeof item.sessionKey === "string" && !item.path)).toBe(true);
  await page.getByRole("button", { name: "归档所选线程", exact: true }).click();
  await expect(dialog.getByRole("button", { name: "确认归档 1 项", exact: true })).toBeEnabled();
  expect(previews).toBe(3);
});

test("canceling an in-flight preview restores filtering and Shift-range + Escape follow Finder semantics", async ({ page }) => {
  await mockApi(page);
  let release!: () => void;
  let started = false;
  const gate = new Promise<void>(resolve => { release = resolve; });
  await mockCommand(page, "sessions.bulkPreview", async args => {
    started = true;
    await gate;
    const request = args.request;
    return { ...request, items: request.sessionKeys.map((key: string) => ({ sessionKey: key, id: key, title: "Old preview", paths: [], bytes: 0, allowed: true, fingerprint: key })) };
  });
  await page.goto("/");
  const rows = page.locator(".thread-item");
  await rows.first().click({ modifiers: ["Meta"] });
  await page.getByRole("button", { name: "归档所选线程", exact: true }).click();
  await expect.poll(() => started).toBe(true);
  const search = page.locator(".search-box input").first();
  await expect(search).toBeDisabled();
  await page.getByRole("button", { name: "取消多选", exact: true }).click();
  await expect(search).toBeEnabled();
  await search.fill("no matching fixture");
  release();
  await expect(page.getByRole("dialog")).toHaveCount(0);
  await expect(page.locator(".finder-selection-bar")).toHaveCount(0);

  await page.locator(".search-box input").first().fill("");
  await rows.nth(0).click({ modifiers: ["Meta"] });
  await rows.nth(2).click({ modifiers: ["Shift"] });
  await expect(page.getByText("已选 3", { exact: true })).toBeVisible();
  await rows.nth(2).focus();
  await page.keyboard.press("Escape");
  await expect(page.locator(".finder-selection-bar")).toHaveCount(0);
});

for (const provider of ["grok"] as const) {
  test(`${provider} batch deletion uses distinct keys and reports failed locations on mobile`, async ({ page }) => {
    await mockApi(page);
    await page.setViewportSize({ width: 390, height: 844 });
    const keys = ["grok-one", "grok-two"];
    await mockCommand(page, `${provider}.list`, () => keys.map((key, index) => ({ id: key, sessionKey: key, title: `Fixture ${index}`, cwd: "/isolated/workspace", path: `/isolated/sessions/${key}`, handoffPath: `/isolated/sessions/${key}/chat_history.jsonl`, status: "recent", messageCount: 1, canRename: true, canDelete: index === 0 })));
    await mockCommand(page, "sessions.bulkPreview", () => ({ provider, operation: "delete", items: keys.map((key, index) => ({ sessionKey: key, id: key, title: `Fixture ${index}`, paths: [`/isolated/sessions/${key}`], bytes: 64, allowed: index === 0, reason: index === 0 ? null : "无法定位线程目录", fingerprint: index === 0 ? "fixture" : null })) }));
    let executed: any;
    await mockCommand(page, "sessions.bulkExecute", args => {
      executed = args.request;
      return { items: [{ sessionKey: keys[0], status: "succeeded" }] };
    });
    await page.goto("/");
    await page.locator(".mobile-tabs").getByRole("button", { name: "Grok Build", exact: true }).click();
    await page.locator(".provider-session").first().click({ modifiers: ["Meta"] });
    await page.locator(".finder-selection-bar").getByRole("button", { name: "全选", exact: true }).click();
    await expect(page.getByText("已选 2", { exact: true })).toBeVisible();
    await page.getByRole("button", { name: "删除所选线程", exact: true }).click();
    await expect(page.getByRole("dialog")).toContainText("无法定位线程目录");
    await assertNoOverflow(page);
    await page.getByRole("button", { name: "确认删除 1 项", exact: true }).click();
    await expect(page.getByText("已选 1", { exact: true })).toBeVisible();
    expect(executed.items).toEqual([{ sessionKey: keys[0] }]);
  });
}

test("Antigravity 20-step alternating thinking and command wall collapses into one ChatGPT-App process disclosure and TimelineRail navigates turns", async ({ page }) => {
  await mockApi(page);
  const events: any[] = [
    { id: "ag-u1", kind: "user_message", text: "First prompt: inspect repository structure", turnId: "t1" },
    { id: "ag-a1", kind: "assistant_message", text: "## Step 1\n\nRepository structure verified.\n\n" + "Detail paragraph.\n\n".repeat(16), turnId: "t1" },
    { id: "ag-u2", kind: "user_message", text: "Second prompt: run multi-step verification", turnId: "t2" },
  ];
  for (let i = 0; i < 10; i++) {
    events.push({ id: `ag-think-${i}`, kind: "thinking", text: `Intermediate reasoning ${i}`, turnId: "t2" });
    events.push({ id: `ag-call-${i}`, kind: "tool_call", callId: `c-${i}`, role: "run_command", detail: `printf check-${i}`, status: "completed", turnId: "t2" });
    events.push({ id: `ag-out-${i}`, kind: "tool_result", callId: `c-${i}`, role: "run_command", result: `ok-${i}`, status: "completed", turnId: "t2" });
  }
  events.push({ id: "ag-a2", kind: "assistant_message", text: "## Final\n\nAll 10 verification rounds succeeded.", turnId: "t2" });

  await mockCommand(page, "antigravity.detail", () => ({
    summary: {
      id: "ag-fixture",
      sessionKey: "ag:ag-fixture",
      title: "Antigravity fixture",
      cwd: "/isolated/antigravity-workspace",
      path: "/isolated/antigravity/brain/ag-fixture/.system_generated/logs/transcript_full.jsonl",
      archived: false,
      updatedAt: "2026-10-01T00:00:00Z",
      messageCount: 4,
      lastMessage: "All 10 verification rounds succeeded.",
      status: "recent",
      formatVersion: "transcript-v1",
      storageSize: { status: "ready", bytes: 4096 },
      canRename: true,
      canArchive: true,
      canDelete: true,
    },
    events,
    totalEvents: events.length,
    hasMore: false,
    timeline: [
      { id: "ag-u1", title: "First prompt: inspect repository structure" },
      { id: "ag-u2", title: "Second prompt: run multi-step verification" },
    ],
  }));

  await page.setViewportSize({ width: 1280, height: 820 });
  await page.goto("/");
  await page.locator(".side-nav").getByRole("button", { name: "Antigravity", exact: true }).click();
  await page.locator(".provider-session").first().click();

  const stream = page.locator(".provider-events");
  const processGroups = stream.locator("details.process-group");
  await expect(processGroups).toHaveCount(1);
  await expect(processGroups.first()).not.toHaveAttribute("open", "");
  await expect(processGroups.first().locator(":scope > summary")).toContainText("已思考");
  await expect(processGroups.first().locator(":scope > summary")).toContainText("10 项操作");

  await processGroups.first().locator(":scope > summary").click();
  await expect(processGroups.first()).toHaveAttribute("open", "");
  await expect(processGroups.first().locator(".process-thought-step")).toHaveCount(10);
  await expect(processGroups.first().locator(".execution-command")).toHaveCount(10);

  const railItems = page.locator(".timeline-rail-item");
  await expect(railItems).toHaveCount(2);
  await railItems.first().click();
  await expect(railItems.first()).toHaveClass(/\bactive\b/);
  await assertNoOverflow(page);
});

test("Command+A and native Select All use row and selection-bar focus and preserve text fields", async ({ page }) => {
  await mockApi(page);
  await page.goto("/");
  // Use the currently loaded normal list; archived scopes have their own handler.
  const normal = page.locator(".thread-item").filter({ visible: true });
  await normal.first().click({ modifiers: ["Meta"] });
  await expect(normal.first()).toBeFocused();
  await page.keyboard.press("Meta+a");
  const count = await normal.count();
  await expect(page.getByText(`已选 ${count}`, { exact: true })).toBeVisible();
  await page.keyboard.press("Escape");
  await normal.first().click({ modifiers: ["Meta"] });
  await page.getByRole("button", { name: "全选", exact: true }).focus();
  await page.evaluate(readFileSync(new URL("../../src-tauri/src/select_all.js", import.meta.url), "utf8"));
  await expect(page.getByText(`已选 ${count}`, { exact: true })).toBeVisible();
  const search = page.locator(".search-box input").first();
  await search.fill("fixture text");
  await page.evaluate(readFileSync(new URL("../../src-tauri/src/select_all.js", import.meta.url), "utf8"));
  await expect(search).toHaveJSProperty("selectionStart", 0);
  await expect(search).toHaveJSProperty("selectionEnd", 12);
  await expect(page.locator(".finder-selection-bar")).toHaveCount(0);
  await search.fill("");
  await page.getByRole("button", { name: "已归档", exact: true }).click();
  const archived = page.locator(".archived-list .thread-item");
  await archived.first().click({ modifiers: ["Meta"] });
  await page.evaluate(readFileSync(new URL("../../src-tauri/src/select_all.js", import.meta.url), "utf8"));
  await expect(page.locator(".thread-item.multi-selected")).toHaveCount(await archived.count());
  await expect(page.getByRole("button", { name: "恢复所选线程", exact: true })).toBeVisible();
});

for (const provider of ["codex", "antigravity", "dsh"] as const) {
  test(`${provider} active and archived selections stay exclusive through every select-all entry`, async ({ page }) => {
    await mockApi(page);
    if (provider !== "codex") {
      await mockCommand(page, `${provider}.list`, args => {
        const archived = Boolean((args.request ?? args).archived);
        return [1, 2].map(index => ({
          id: `${provider}-${archived}-${index}`, sessionKey: `${provider}-${archived}-${index}`,
          title: `${archived ? "Archived" : "Active"} fixture ${index}`, cwd: "/isolated/workspace", path: "/isolated/session",
          archived, updatedAt: "2026-10-01T00:00:00Z", messageCount: 2, status: "recent",
          canRename: true, canArchive: true, canDelete: true,
        }));
      });
    }
    await page.goto("/");
    if (provider !== "codex") {
      await page.locator(".side-nav").getByRole("button", { name: provider === "antigravity" ? "Antigravity" : "DeepSeek Harness", exact: true }).click();
    }
    const row = provider === "codex" ? ".thread-item" : ".provider-session";
    const active = page.locator(`${row}:not(.archived-list ${row})`);
    const toggle = page.locator(".archived-toggle");
    await active.first().click({ modifiers: ["Meta"] });
    await toggle.focus();
    await page.keyboard.press("Meta+a");
    await expect(page.locator(`${row}.multi-selected`)).toHaveCount(1);
    await page.keyboard.press("Escape");
    await expect(page.locator(`${row}.multi-selected`)).toHaveCount(0);
    await toggle.click();
    const archived = page.locator(`.archived-list ${row}`);
    await expect(archived.first()).toBeVisible();
    // Focus stays on the drawer button after opening it.
    await page.keyboard.press("Meta+a");
    await expect(page.locator(`${row}.multi-selected`)).toHaveCount(await archived.count());
    await expect(page.locator(".finder-selection-bar")).toHaveCount(1);
    await expect(page.getByRole("button", { name: "恢复所选线程", exact: true })).toBeVisible();
    // Move focus without clicking: the keyboard path must clear archived selection.
    await page.locator(row).first().focus();
    await page.keyboard.press("Meta+a");
    await expect(page.locator(`.archived-list ${row}.multi-selected`)).toHaveCount(0);
    await expect(page.locator(".finder-selection-bar")).toHaveCount(1);
    await expect(page.getByRole("button", { name: "归档所选线程", exact: true })).toBeVisible();
    await toggle.focus();
    await page.evaluate(readFileSync(new URL("../../src-tauri/src/select_all.js", import.meta.url), "utf8"));
    await expect(page.locator(".finder-selection-bar")).toHaveCount(1);
    await expect(page.getByRole("button", { name: "恢复所选线程", exact: true })).toBeVisible();
    await page.getByRole("button", { name: "全选", exact: true }).click();
    await expect(page.locator(`${row}.multi-selected`)).toHaveCount(await archived.count());
    await toggle.click();
    await expect(page.locator(".finder-selection-bar")).toHaveCount(0);
    await toggle.click();
    await expect(page.locator(`.archived-list ${row}.multi-selected`)).toHaveCount(0);
    let release!: () => void;
    let started = false;
    const pending = new Promise<void>(resolve => { release = resolve; });
    await mockCommand(page, "sessions.bulkPreview", args => ({ ...args.request, items: args.request.sessionKeys.map((key: string) => ({ sessionKey: key, id: key, title: "Selected fixture", paths: [], bytes: 0, allowed: true, fingerprint: key })) }));
    await mockCommand(page, "sessions.bulkExecute", async args => {
      started = true;
      await pending;
      return { items: args.request.items.map((item: any) => ({ sessionKey: item.sessionKey, status: "succeeded" })) };
    });
    await archived.first().click({ modifiers: ["Meta"] });
    await page.getByRole("button", { name: "恢复所选线程", exact: true }).click();
    await page.getByRole("button", { name: "确认恢复 1 项", exact: true }).click();
    await expect.poll(() => started).toBe(true);
    await toggle.click();
    await toggle.click();
    await expect(archived.first()).toBeDisabled();
    await toggle.focus();
    await page.keyboard.press("Meta+a");
    await expect(page.locator(".finder-selection-bar")).toHaveCount(0);
    await expect(page.locator(".search-box input").first()).toBeDisabled();
    await toggle.click();
    release();
    await expect(page.getByText("已完成 1 项，未完成 0 项", { exact: true })).toBeVisible();
    await active.first().click({ modifiers: ["Meta"] });
    await expect(page.locator(".batch-result-banner")).toHaveCount(0);
    await mockCommand(page, "sessions.bulkPreview", () => { throw new Error("Fixture preview failure"); });
    await page.getByRole("button", { name: "归档所选线程", exact: true }).click();
    await expect(page.getByRole("alert").filter({ hasText: "Fixture preview failure" })).toBeVisible();
    await toggle.click();
    await archived.first().click({ modifiers: ["Meta"] });
    await expect(page.getByRole("alert").filter({ hasText: "Fixture preview failure" })).toHaveCount(0);
  });
}

for (const provider of ["claude", "grok"] as const) {
  test(`${provider} detail writes stay disabled during a batch preview`, async ({ page }) => {
    await mockApi(page);
    const summary = {
      id: `${provider}-batch-fixture`, sessionKey: `${provider}:batch-fixture`, title: "Batch fixture",
      cwd: "/isolated/workspace", path: "/isolated/session.jsonl", handoffPath: "/isolated/session.jsonl",
      status: "recent", messageCount: 1, canRename: true, canDelete: true,
    };
    await mockCommand(page, `${provider}.list`, () => [summary]);
    await mockCommand(page, `${provider}.detail`, () => ({ summary, events: [], totalEvents: 0, hasMore: false }));
    let release!: () => void;
    const gate = new Promise<void>(resolve => { release = resolve; });
    let started = false;
    await mockCommand(page, "sessions.bulkPreview", async args => {
      started = true; await gate;
      return { ...args.request, items: [] };
    });
    await page.goto("/");
    await page.locator(".side-nav").getByRole("button", { name: provider === "claude" ? "Claude Code" : "Grok Build", exact: true }).click();
    const row = page.locator(".provider-session").first();
    await row.click();
    const menuButton = page.getByRole("button", { name: provider === "claude" ? "Claude Code 任务操作" : "Grok 任务操作", exact: true });
    await expect(menuButton).toBeVisible();
    await row.click({ modifiers: ["Meta"] });
    await page.getByRole("button", { name: "删除所选线程", exact: true }).click();
    await expect.poll(() => started).toBe(true);
    await menuButton.click();
    await expect(page.getByRole("button", { name: "改名", exact: true })).toBeDisabled();
    await expect(page.getByRole("button", { name: "删除任务文件", exact: true })).toBeDisabled();
    release();
  });
}
