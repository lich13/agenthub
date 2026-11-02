import { describe, expect, test } from "vitest";

const productionBusinessSources = import.meta.glob([
  "../../App.tsx",
  "../../components/**/*.ts",
  "../../components/**/*.tsx",
  "../../hooks/**/*.ts",
  "../api/**/*.ts",
  "./**/*.ts",
  "../query/**/*.ts",
  "!../api/**/*.test.*",
  "!../api/**/__tests__/**",
  "!./**/*.test.*",
  "!./**/__tests__/**",
  "!../query/**/*.test.*",
  "!../query/**/__tests__/**"
], {
  eager: true,
  query: "?raw",
  import: "default"
}) as Record<string, string>;

const forbiddenLiteralTokens = [
  "desktopCommand",
  "webCommand",
  "runtimeDispatch",
  "runtimeValue",
  "desktop_api_command",
  "desktopApiRoute",
  "invokeDesktopApi",
  "desktop_api",
  "DesktopApi",
  "desktopBridge",
  "WebRoute",
  "DesktopRoute",
  "getRuntimeKind",
  "currentRuntimeCapabilities",
  "systemCapabilitiesForRuntime",
  "linuxBackupPrune",
  "linuxUpdateLabels"
];

const forbiddenCommandNames = [
  "login",
  "logout",
  "me",
  "publicSettings",
  "desktopApi",
  "uploadFiles",
  "threadEvents"
];

const forbiddenCommandPattern = new RegExp(
  String.raw`\b(?:callCommand|startProbeCommand|runTypedUpdateCommand|runtimeRpc|webJsonRpc|invokeDesktop)(?:<[^>]+>)?\(\s*["'](?:${forbiddenCommandNames.join("|")})["']`
);

const productionLeakPattern = /\/Users\/(?!example(?:\/|\b)|test(?:\/|\b))[^/\s"\']+|\/opt\/agenthub/i;

const productionComponentSources = import.meta.glob([
  "../../App.tsx",
  "../../components/**/*.ts",
  "../../components/**/*.tsx",
  "../../hooks/**/*.ts"
], {
  eager: true,
  query: "?raw",
  import: "default"
}) as Record<string, string>;

describe("production business source scan", () => {
  test("does not reintroduce legacy runtime bridge helpers or command names", () => {
    expect(Object.keys(productionBusinessSources).length).toBeGreaterThan(3);

    for (const [path, source] of Object.entries(productionBusinessSources)) {
      for (const token of forbiddenLiteralTokens) {
        expect(source, `${path} must not contain ${token}`).not.toContain(token);
      }
      expect(source, `${path} must not use legacy non-dot runtime command names`).not.toMatch(forbiddenCommandPattern);
      expect(source, `${path} must not declare route maps in business layers`).not.toMatch(/\bconst\s+ROUTES\b/);
    }
  });

  test("demo and production fixtures do not leak real production endpoints or paths", () => {
    for (const [path, source] of Object.entries(productionBusinessSources)) {
      expect(source, `${path} must not leak production host/path details`).not.toMatch(productionLeakPattern);
    }
  });

  test("components stay behind runtime/api/query/domain boundaries", () => {
    expect(Object.keys(productionComponentSources).length).toBeGreaterThan(1);

    for (const [path, source] of Object.entries(productionComponentSources)) {
      expect(source, `${path} must not directly call runtime transport`).not.toMatch(/\b(runtimeRpc|uploadRuntimeFiles|createRuntimeThreadEventSource|buildRuntimeApiPath)\b/);
      expect(source, `${path} must not directly own query cache`).not.toMatch(/\b(useQueryClient|setQueryData|invalidateQueries|getQueryCache|getQueryData|removeQueries|cancelQueries)\b/);
      expect(source, `${path} must not directly import transport/API`).not.toMatch(/from\s+["'][^"']*(?:lib\/api|api\/transport|lib\/runtime)["']/);
    }
  });

  test("App is only the shell/composition layer for machine selection and readonly task browsing", () => {
    const source = productionComponentSources["../../App.tsx"];
    const chatWorkspace = productionComponentSources["../../components/chat/ChatWorkspace.tsx"];
    const conversation = productionComponentSources["../../components/chat/Conversation.tsx"];
    const composerAttachments = productionComponentSources["../../components/composer/useComposerAttachments.ts"];

    expect(source).toContain("MachineButton");
    expect(source).not.toContain("WebAuthGate");
    expect(source).toContain("ChatWorkspace");
    expect(source).not.toContain("useConversationController");
    expect(source).not.toContain("SlashCommandTextarea");
    expect(source).not.toContain("useComposerAttachments");
    expect(chatWorkspace).toContain("useConversationController");
    expect(conversation).not.toContain("SlashCommandTextarea");
    expect(conversation).not.toContain("useComposerAttachments");
    expect(composerAttachments).toBeUndefined();
    expect(source).not.toMatch(/\bfunction\s+(LoginScreen|ensureTurnstileScript|SlashCommandTextarea|useComposerAttachments)\b/);
    expect(source).not.toContain("useThreadRealtimeSubscription(");
    expect(source).not.toContain("useThreadCacheActions(");
    expect(source).not.toContain("useThreadMessageStoreController(");
  });

  test("runtime globals are read only by runtime transport/context modules", () => {
    for (const [path, source] of Object.entries(productionBusinessSources)) {
      if (path.endsWith("../runtime.ts")) continue;
      expect(source, `${path} must not read runtime globals directly`).not.toMatch(/__TAURI_INTERNALS__|__AGENTHUB_DESKTOP_RUNTIME__/);
    }
  });
});
