import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, test } from "vitest";
import agentHookHostCardSource from "./AgentHookHostCard.tsx?raw";
import { DshHostCard } from "./AgentHookHostCard";
import probeWorkspaceSource from "./ProbeWorkspace.tsx?raw";
import copyFeedbackSource from "../../hooks/useCopyFeedback.ts?raw";

describe("ProbeWorkspace & AgentHookHostCard", () => {
  test("removes redundant runtime badge pills, sub-tab switcher, and duplicate hooksHostMatrix renders", () => {
    expect(probeWorkspaceSource).not.toContain("tauri:AgentHub.app");
    expect(probeWorkspaceSource).not.toContain("ProbeViewTab");
    expect(probeWorkspaceSource).not.toContain("probe-subnav");
    expect(probeWorkspaceSource.match(/\{hooksHostMatrix\}/g) ?? []).toHaveLength(1);
  });

  test("AgentHookHostCard uses backend config_path without hardcoded host config paths and DshHostCard has no disabled button", () => {
    expect(agentHookHostCardSource).not.toContain("~/.claude/settings.json");
    expect(agentHookHostCardSource).not.toContain("~/.grok/");
    expect(agentHookHostCardSource).not.toContain("~/.gemini/");
    expect(agentHookHostCardSource).not.toContain("~/.codex/hooks.json");

    const dshMarkup = renderToStaticMarkup(
      <DshHostCard sessionsPath="/custom/sandbox/.dsh/sessions" />,
    );
    expect(dshMarkup).toContain("/custom/sandbox/.dsh/sessions");
    expect(dshMarkup).toContain("原生事件流直连");
    expect(dshMarkup).not.toContain("<button");
    expect(dshMarkup).not.toContain("disabled");
  });

  test("useCopyFeedback centralizes clipboard writes and timer cleanup", () => {
    expect(copyFeedbackSource).toContain("navigator.clipboard.writeText");
    expect(copyFeedbackSource).toContain("clearTimeout");
  });
});
