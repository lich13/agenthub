import { expect, test, type Page } from "@playwright/test";
import * as demo from "../src/lib/api/demo";
import { mockApi, mockCommand, observeCommands } from "./fixtures";

const baseUrl = "https://api.example.com/agenthub/";
const sharedId = "same-machine-thread";
const machinePicker = (page: Page) => page.getByRole("combobox", { name: "当前机器" });
const settings = async (page: Page) => {
  await page.locator(".side-nav").getByRole("button", { name: "设置", exact: true }).click();
  await page.getByRole("tab", { name: "远程连接", exact: true }).click();
};

async function installMachines(page: Page) {
  const calls = await mockApi(page, { connection: { configured: true, baseUrl } });
  let machine: "local" | "remote" = "local";
  const titles = { local: "Local thread", remote: "Remote thread" };
  const summary = () => ({ ...demo.demoThreads(false, "")[0], id: sharedId, title: titles[machine], status: "Recent" as const });
  const detail = () => ({ ...demo.demoThreadDetail(sharedId), summary: summary(), blocks: [
    { id: "same-message", role: "assistant", kind: "message", text: `${machine} machine content`, questions: [] }
  ] });
  await mockCommand(page, "remote.select", async (args, next) => {
    const view = await next(); machine = args.request.target; return view;
  });
  await mockCommand(page, "threads.list", args => args?.archived ? [] : [summary()]);
  await mockCommand(page, "threads.detail", detail);
  await mockCommand(page, "threads.rename", args => { titles[machine] = args.name; return { ok: true }; });
  return { calls, detail, machine: () => machine };
}

async function selectThread(page: Page, title: string) {
  await page.locator(".thread-item").filter({ hasText: title }).click();
  await expect(page.locator(".conversation-title")).toHaveText(title);
}

test("switching machines isolates list, detail, selection and title overrides for the same thread ID", async ({ page }) => {
  const { calls } = await installMachines(page);
  const requests: string[] = [];
  page.on("request", request => { if (request.url().includes("/api/rpc/")) requests.push(request.url()); });
  await page.goto("/");
  await selectThread(page, "Local thread");
  await page.getByLabel("任务操作", { exact: true }).click();
  await page.getByRole("button", { name: "改名", exact: true }).click();
  await page.getByLabel("任务名称", { exact: true }).fill("Local renamed");
  await page.getByTitle("保存名称").click();
  await expect(page.locator(".conversation-title")).toHaveText("Local renamed");
  await machinePicker(page).selectOption("remote");
  await expect(page.locator(".thread-item")).toContainText("Remote thread");
  await expect(page.locator(".conversation-title")).toHaveCount(0);
  await selectThread(page, "Remote thread");
  await expect(page.locator(".message-stream")).toContainText("remote machine content");
  await expect(page.locator(".message-stream")).not.toContainText("local machine content");
  await machinePicker(page).selectOption("local");
  await selectThread(page, "Local renamed");
  await expect(page.locator(".message-stream")).toContainText("local machine content");
  expect(calls.filter(command => command === "remote.get")).toHaveLength(1);
  expect(calls.filter(command => command === "remote.select")).toHaveLength(2);
  expect(requests).toEqual([]);
});

test("a late local detail cannot replace the selected remote thread with the same ID", async ({ page }) => {
  const state = await installMachines(page);
  let release!: () => void;
  const gate = new Promise<void>(resolve => { release = resolve; });
  let started = false;
  let finished = false;
  await mockCommand(page, "threads.detail", async () => {
    const result = state.detail();
    if (state.machine() === "local") { started = true; await gate; finished = true; }
    return result;
  });
  await page.goto("/");
  await page.locator(".thread-item").click();
  await expect.poll(() => started).toBe(true);
  await machinePicker(page).selectOption("remote");
  await selectThread(page, "Remote thread");
  await expect(page.locator(".message-stream")).toContainText("remote machine content");
  release();
  await expect.poll(() => finished).toBe(true);
  await expect(page.locator(".conversation-title")).toHaveText("Remote thread");
  await expect(page.locator(".message-stream")).not.toContainText("local machine content");
});

test("an in-flight rename disables machine switching until the native write finishes", async ({ page }) => {
  await installMachines(page);
  let release!: () => void;
  const gate = new Promise<void>(resolve => { release = resolve; });
  let started = false;
  await mockCommand(page, "threads.rename", async () => { started = true; await gate; return { ok: true }; });
  await page.goto("/");
  await selectThread(page, "Local thread");
  await page.getByLabel("任务操作", { exact: true }).click();
  await page.getByRole("button", { name: "改名", exact: true }).click();
  await page.getByLabel("任务名称", { exact: true }).fill("Pending title");
  await page.getByTitle("保存名称").click();
  await expect.poll(() => started).toBe(true);
  await expect(machinePicker(page)).toBeDisabled();
  release();
  await expect(machinePicker(page)).toBeEnabled();
  await machinePicker(page).selectOption("remote");
  await expect(page.locator(".thread-item")).toContainText("Remote thread");
});

test("a failed remote connection stays selected and never displays local rows", async ({ page }) => {
  const { calls } = await installMachines(page);
  await mockCommand(page, "remote.invoke", () => { throw new Error("远程连接失败：fixture offline"); });
  await page.goto("/");
  await expect(page.locator(".thread-item")).toContainText("Local thread");
  const before = calls.filter(command => command === "threads.list").length;
  await machinePicker(page).selectOption("remote");
  await expect(page.locator(".form-error").first()).toContainText("fixture offline");
  await expect(machinePicker(page)).toHaveValue("remote");
  await expect(page.locator(".thread-item")).toHaveCount(0);
  expect(calls.filter(command => command === "threads.list")).toHaveLength(before);
});

test("saving the API Key clears its field and never stores it in browser state", async ({ page }) => {
  const calls = await mockApi(page);
  const credentials: Record<string, unknown>[] = [];
  observeCommands(page, (command, args) => { if (command === "remote.save") credentials.push(args.request); });
  await page.goto("/");
  await machinePicker(page).selectOption("remote");
  await expect(page.getByRole("tab", { name: "远程连接" })).toHaveAttribute("aria-selected", "true");
  const key = "fixture-private-api-key";
  await page.getByLabel("HTTPS 地址", { exact: true }).fill(baseUrl);
  await page.getByLabel("管理员 API Key", { exact: true }).fill(key);
  await page.getByRole("button", { name: "验证并保存", exact: true }).click();
  await expect(machinePicker(page)).toHaveValue("remote");
  await settings(page);
  await expect(page.getByLabel("管理员 API Key", { exact: true })).toHaveValue("");
  await expect(page.getByLabel("HTTPS 地址", { exact: true })).toHaveValue(baseUrl);
  expect(credentials).toEqual([{ revision: 0, baseUrl, apiKey: key }]);
  expect(await page.evaluate(() => JSON.stringify({ local: { ...localStorage }, session: { ...sessionStorage } }))).not.toContain(key);
  await page.reload();
  await settings(page);
  await expect(page.getByLabel("管理员 API Key", { exact: true })).toHaveValue("");
  expect(calls.filter(command => command === "remote.get")).toHaveLength(2);
});

test("remote Markdown file paths copy the original path and never offer Finder reveal", async ({ page }) => {
  const calls = await mockApi(page, { connection: { target: "remote", revision: 1, configured: true, baseUrl } });
  await page.addInitScript(() => {
    Object.defineProperty(navigator, "clipboard", { value: { writeText: async (text: string) => { document.documentElement.dataset.copied = text; } } });
  });
  await mockCommand(page, "grok.detail", () => ({ summary: { cwd: "/srv/project" }, events: [
    { kind: "agent_message_chunk", text: "[remote source](/srv/project/src/main.rs:42)" }
  ] }));
  await page.goto("/");
  await page.locator(".side-nav").getByRole("button", { name: "Grok Build", exact: true }).click();
  await page.getByRole("button", { name: "remote source" }).click();
  await expect(page.locator("html")).toHaveAttribute("data-copied", "/srv/project/src/main.rs");
  await expect(page.getByRole("button", { name: /Finder/ })).toHaveCount(0);
  expect(calls).not.toContain("plugin:opener|reveal_item_in_dir");
});
