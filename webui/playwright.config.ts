import { defineConfig } from "@playwright/test";
import { join } from "node:path";
import { tmpdir } from "node:os";

export default defineConfig({
  testDir: "./e2e",
  testMatch: "**/*.pw.ts",
  timeout: 30000,
  fullyParallel: true,
  workers: 2,
  reporter: "list",
  globalSetup: "./playwright.global-setup.mjs",
  outputDir: process.env.AGENTHUB_TEST_OUTPUT ?? join(tmpdir(), `codex-${process.env.CODEX_THREAD_ID ?? "agenthub-ci"}`, "playwright"),
  use: { baseURL: "http://127.0.0.1:5197", screenshot: "only-on-failure", trace: "retain-on-failure" },
  projects: [
    // Use the browsers installed by Playwright in both headed and headless runs.
    { name: "chromium", use: { browserName: "chromium", launchOptions: { ignoreDefaultArgs: ["--hide-scrollbars"] } } },
    { name: "webkit", use: { browserName: "webkit" } }
  ],
  webServer: {
    command: process.env.AGENTHUB_BROWSER_PREVIEW === "1"
      ? "pnpm exec vite preview --host 127.0.0.1 --port 5197 --strictPort"
      : "VITE_USE_REAL_API=1 pnpm dev --host 127.0.0.1 --port 5197 --strictPort",
    url: "http://127.0.0.1:5197",
    reuseExistingServer: false
  }
});
