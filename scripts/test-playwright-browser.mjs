import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "node:test";
import { assertManagedExecutable, launchOptionsForProject, verifyProjectBrowser } from "../webui/playwright-browser-guard.mjs";

function fixture(t) {
  const root = mkdtempSync(join(tmpdir(), "agenthub-browser-"));
  t.after(() => rmSync(root, { recursive: true }));
  const file = relative => {
    const path = join(root, relative);
    mkdirSync(dirname(path), { recursive: true });
    writeFileSync(path, "synthetic executable");
    return path;
  };
  return { root, file, expected: file("ms-playwright/chromium-1/Chrome for Testing.app/browser") };
}

test("bundled headed Chromium, headless shell and WebKit pass by real path", t => {
  const f = fixture(t);
  assert.match(assertManagedExecutable(f.expected, f.expected), /^chromium-1/);
  const shell = f.file("ms-playwright/chromium_headless_shell-1/browser");
  assert.match(assertManagedExecutable(shell, f.expected), /^chromium_headless_shell-1/);
  const webkit = f.file("ms-playwright/webkit-1/pw_run.sh");
  assert.match(assertManagedExecutable(webkit, webkit), /^webkit-1/);
});

test("system executable and a cache prefix lookalike are rejected", t => {
  const f = fixture(t);
  for (const path of ["Applications/Google Chrome.app/browser", "ms-playwright-other/chromium-1/browser"]) {
    assert.throws(() => assertManagedExecutable(f.file(path), f.expected), /escaped/);
  }
});

test("symlinks cannot redirect a managed executable outside its cache", t => {
  const f = fixture(t);
  const external = f.file("Applications/browser");
  const link = join(dirname(f.expected), "redirected");
  symlinkSync(external, link);
  assert.throws(() => assertManagedExecutable(link, f.expected), /escaped/);
  assert.throws(() => assertManagedExecutable(link, link), /escaped/);
});

test("project options retain headed and scrollbar settings without channel or executable overrides", () => {
  assert.deepEqual(launchOptionsForProject({ browserName: "chromium", headless: false,
    launchOptions: { ignoreDefaultArgs: ["--hide-scrollbars"] } }, {}),
  { headless: false, ignoreDefaultArgs: ["--hide-scrollbars"], host: "127.0.0.1" });
  assert.equal(launchOptionsForProject({ browserName: "webkit" }, {}).headless, true);
  for (const override of [
    { channel: "chromium" }, { channel: "chrome" }, { channel: "msedge" },
    { launchOptions: { channel: "chrome-beta" } },
    { launchOptions: { executablePath: "/fixture/system-browser" } },
    { connectOptions: { wsEndpoint: "ws://127.0.0.1:1234" } }
  ]) assert.throws(() => launchOptionsForProject({ browserName: "chromium", ...override }, {}));
  assert.throws(() => launchOptionsForProject({ browserName: "chromium" }, { PW_TEST_CONNECT_WS_ENDPOINT: "ws://127.0.0.1:1234" }));
});

test("the actual launched executable is checked and the probe always closes", async t => {
  const f = fixture(t);
  for (const actual of [f.expected, f.file("Applications/browser")]) {
    let closes = 0;
    const browser = {
      executablePath: () => f.expected,
      launchServer: async options => {
        assert.equal(options.headless, false);
        return { process: () => ({ spawnfile: actual }), close: async () => { closes++; } };
      }
    };
    const result = verifyProjectBrowser({ use: { browserName: "chromium", headless: false } }, { chromium: browser }, {});
    if (actual === f.expected) await result;
    else await assert.rejects(result, /escaped/);
    assert.equal(closes, 1);
  }
});

test("a redirected installation fails before launching any browser", async t => {
  const f = fixture(t);
  const link = join(dirname(f.expected), "redirected");
  symlinkSync(f.file("Applications/browser"), link);
  await assert.rejects(verifyProjectBrowser({ use: { browserName: "chromium" } }, {
    chromium: { executablePath: () => link, launchServer: () => assert.fail("must not launch") }
  }, {}), /escaped/);
});

test("repository browser entrypoints cannot select installed brand channels or executable paths", () => {
  const root = fileURLToPath(new URL("../", import.meta.url));
  const files = execFileSync("git", ["ls-files", "-z"], { cwd: root, encoding: "utf8" }).split("\0")
    .filter(path => /\.(?:[cm]?[jt]sx?|json|ya?ml|md|sh)$/.test(path))
    .filter(path => !/(?:^|\/)(?:test-[^/]+|[^/]+\.test\.[^/]+)$/.test(path));
  for (const path of files) {
    const source = readFileSync(join(root, path), "utf8");
    assert.doesNotMatch(source, /\bchannel\s*:\s*["'](?:chromium|chrome(?:-beta|-dev|-canary)?|msedge(?:-beta|-dev|-canary)?)["']/, path);
    assert.doesNotMatch(source, /\bexecutablePath\s*:\s*["']/, path);
    assert.doesNotMatch(source, /\/Applications\/Google Chrome\.app/, path);
  }
  const config = readFileSync(join(root, "webui/playwright.config.ts"), "utf8");
  assert.match(config, /globalSetup:\s*["']\.\/playwright\.global-setup\.mjs["']/);
});
