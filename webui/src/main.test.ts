import { describe, expect, test } from "vitest";
import indexHtml from "../index.html?raw";
import bootGuardSource from "../public/agenthub-boot.js?raw";
import { THEME_STORAGE_KEY } from "./lib/theme";
import mainSource from "./main.tsx?raw";

describe("AgentHub WebUI bootstrap", () => {
  test("installs a visible boot error guard before the module bundle runs", () => {
    const guardIndex = indexHtml.indexOf("window.__AGENTHUB_BOOT__");
    const guardScriptIndex = indexHtml.indexOf("agenthub-boot.js");
    const moduleIndex = indexHtml.indexOf('type="module"');

    expect(guardScriptIndex).toBeGreaterThanOrEqual(0);
    expect(moduleIndex).toBeGreaterThan(guardScriptIndex);
    expect(bootGuardSource).toContain(`localStorage.getItem(${JSON.stringify(THEME_STORAGE_KEY)})`);
    expect(bootGuardSource).toContain('document.documentElement.setAttribute("data-theme", storedTheme)');
    expect(bootGuardSource).toContain('document.documentElement.removeAttribute("data-theme")');
    expect(bootGuardSource).toContain("window.__AGENTHUB_BOOT__");
    expect(bootGuardSource).toContain("AgentHub 界面载入失败");
    expect(bootGuardSource).toContain("window.addEventListener(\"error\"");
    expect(bootGuardSource).toContain("window.addEventListener(\"unhandledrejection\"");
    expect(guardIndex).toBe(-1);
  });

  test("marks React mount completion so the boot guard does not leave a blank WebView", () => {
    expect(mainSource).toContain("window.__AGENTHUB_BOOT__");
    expect(mainSource).toContain(".mounted = true");
  });
});
