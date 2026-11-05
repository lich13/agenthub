import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { ComponentProps, ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, test, vi } from "vitest";

import {
  AntigravityWorkspace,
  DshWorkspace,
  providerSessionLabel,
  renderProviderEvents,
} from "./ArchivableProviderWorkspace";
import { groupProviderEvents } from "../../lib/domain/executionGroups";
import type { ProviderHistoryEvent } from "../../types";
import { DisclosureRevealContext, DisclosureScope } from "../common/ActivityDetails";

const observed = vi.hoisted(() => ({
  elements: [] as ReactElement<ComponentProps<"details">>[],
}));

test("Grok Bot pending interactions keep their reply status without a running tool", () => {
  const events: ProviderHistoryEvent[] = [{ id: "question", kind: "interaction", status: "reply_needed", text: "Fixture question", detail: "Choose the next step" }];
  const html = renderToStaticMarkup(<>{renderProviderEvents(groupProviderEvents(events, "Grok Bot"), "Fixture", "Grok Bot")}</>);
  expect(html).toContain("需要回复");
  expect(html).toContain("Fixture question");
  expect(html).not.toContain("运行中");
  expect(html).not.toContain("已完成");
});

vi.mock("../common/ActivityDetails", async importOriginal => {
  const actual = await importOriginal<typeof import("../common/ActivityDetails")>();
  return {
    ...actual,
    ActivityDetails: (props: ComponentProps<typeof actual.ActivityDetails>) => {
      const element = actual.ActivityDetails(props);
      observed.elements.push(element);
      return element;
    },
  };
});

function sampleEvents(): ProviderHistoryEvent[] {
  return [
    {
      id: "turn-user",
      kind: "user_message",
      text: "Inspect the workspace build pipeline",
    },
    {
      id: "turn-think",
      kind: "thinking",
      text: "Hidden reasoning trace fixture",
    },
    {
      id: "turn-cmd-call",
      kind: "tool_call",
      callId: "cmd-1",
      role: "run_command",
      status: "completed",
      detail: "cargo test --locked",
    },
    {
      id: "turn-cmd-out",
      kind: "tool_result",
      callId: "cmd-1",
      status: "completed",
      result: "test result: ok.",
    },
    {
      id: "turn-web-call",
      kind: "tool_call",
      callId: "web-1",
      role: "search_web",
      status: "completed",
      detail: "tauri v2 updater docs",
    },
    {
      id: "turn-web-out",
      kind: "tool_result",
      callId: "web-1",
      status: "completed",
      result: "Found official guide.",
    },
    {
      id: "turn-final",
      kind: "assistant_message",
      text: "## Summary\n\nAll checks passed.",
    },
  ];
}

function renderEvents(
  providerName: "Antigravity" | "DeepSeek Harness",
  reveal: string | null = null,
) {
  observed.elements = [];
  const grouped = groupProviderEvents(sampleEvents(), providerName);
  const html = renderToStaticMarkup(
    <DisclosureScope.Provider value={`${providerName}:fixture`}>
      <DisclosureRevealContext.Provider value={reveal}>
        {renderProviderEvents(grouped, "Fixture Session", providerName)}
      </DisclosureRevealContext.Provider>
    </DisclosureScope.Provider>,
  );
  return { html, details: [...observed.elements] };
}

describe("ArchivableProviderWorkspace", () => {
  test.each(["Antigravity", "DeepSeek Harness"] as const)(
    "%s renders user message, collapsed thinking, grouped execution disclosures, and copyable final markdown",
    providerName => {
      const { html, details } = renderEvents(providerName);
      expect(details).toHaveLength(1);
      expect(details[0]?.props.open).toBe(false);
      expect(html).toContain("process-group");
      expect(html).toContain("Inspect the workspace build pipeline");
      expect(html).toContain("已思考");
      expect(html).not.toContain("Hidden reasoning trace fixture");
      expect(html).not.toContain("test result: ok.");
      expect(html).toContain("All checks passed.");
      expect(html).toContain('aria-label="复制回复"');
    },
  );

  test("search reveal opens thinking or grouped tool disclosures at the target position", () => {
    const thinkRevealed = renderEvents("Antigravity", "turn-think");
    expect(thinkRevealed.html).toContain("Hidden reasoning trace fixture");
    expect(thinkRevealed.html).not.toContain("test result: ok.");

    const toolRevealed = renderEvents("DeepSeek Harness", "turn-cmd-out");
    expect(toolRevealed.html).toContain("cargo test --locked");
    expect(toolRevealed.html).toContain("test result: ok.");
  });

  test("providerSessionLabel trims titles and falls back cleanly for untitled threads", () => {
    expect(providerSessionLabel({ id: "ag-1", title: "  Thread Alpha  " })).toBe("Thread Alpha");
    expect(providerSessionLabel({ id: "ag-2", title: "   " })).toBe("ag-2");
  });

  test("older remote servers without antigravity_sessions or dsh_sessions render a graceful upgrade notice", () => {
    vi.stubGlobal("document", { visibilityState: "visible" });
    try {
      const client = new QueryClient({
        defaultOptions: { queries: { retry: false } },
      });
      const unsupportedCapabilities = {
        runtimeKind: "desktop" as const,
        hostSurface: "linux_server_api" as const,
        codexStatePaths: true,
        updatePrune: true,
        threadCleanup: true,
        threadArchiveActions: true,
        updateServiceLabels: true,
        antigravitySessions: false,
        dshSessions: false,
      };

      const agHtml = renderToStaticMarkup(
        <QueryClientProvider client={client}>
          <AntigravityWorkspace capabilities={unsupportedCapabilities} />
        </QueryClientProvider>,
      );
      expect(agHtml).toContain("当前连接的远程服务版本较旧，尚未支持 Antigravity 会话列表");
      expect(agHtml).toContain("升级远程服务后即可浏览 Antigravity 会话");

      const dshHtml = renderToStaticMarkup(
        <QueryClientProvider client={client}>
          <DshWorkspace capabilities={unsupportedCapabilities} />
        </QueryClientProvider>,
      );
      expect(dshHtml).toContain("当前连接的远程服务版本较旧，尚未支持 DeepSeek Harness 会话列表");
      expect(dshHtml).toContain("升级远程服务后即可浏览 DeepSeek Harness 会话");
    } finally {
      vi.unstubAllGlobals();
    }
  });
});
