import { Children, isValidElement, type ComponentProps, type MouseEvent, type ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { expect, test, vi } from "vitest";
import { UserMessage, UserMessageScope } from "./UserMessage";
import { CopyReplyButton } from "./CopyReplyButton";
import { DisclosureScope } from "./ActivityDetails";
import { MessageBlockView } from "../chat/MessageStream";

const observedDisclosures = vi.hoisted(() => ({ elements: [] as ReactElement<ComponentProps<"details">>[], machineScope: "local" }));

vi.mock("../../lib/query/connection", async importOriginal => {
  const actual = await importOriginal<typeof import("../../lib/query/connection")>();
  return { ...actual, machineScope: () => observedDisclosures.machineScope };
});

// Observe the actual event handlers without replacing disclosure state behavior.
vi.mock("./ActivityDetails", async importOriginal => {
  const actual = await importOriginal<typeof import("./ActivityDetails")>();
  return {
    ...actual,
    ActivityDetails: (props: ComponentProps<typeof actual.ActivityDetails>) => {
      const element = actual.ActivityDetails(props);
      observedDisclosures.elements.push(element);
      return element;
    }
  };
});

const adjacentInstructions = "# AGENTS.md instructions\n\n<INSTRUCTIONS>\n# Rules\nKeep checks.\n# Notes\n保留格式🙂\n</INSTRUCTIONS>";

test("user content is literal text with line breaks and no Markdown headings", () => {
  const text = "# Heading\n  - list\n```js\nconsole.log(1)\n```";
  const html = renderToStaticMarkup(<UserMessage text={text} />);
  expect(html).toContain(text);
  expect(html).not.toMatch(/<(h1|h2|ul|pre)\b/);
  expect(html).not.toContain("chat-meta");
});

test("image-only messages have an attachment area and no empty bubble", () => {
  const html = renderToStaticMarkup(<UserMessage message={{ id: "m", text: "", attachments: [{ id: "i", name: "sample.png", kind: "image", reason: "原附件已不存在或无法读取" }] }} />);
  expect(html).toContain("消息附件");
  expect(html).toContain("sample.png");
  expect(html).not.toContain("user-message-bubble");
});

test("internal page wrappers are removed from user messages and wrapper-only bubbles", () => {
  const page = '<external_codex_apps_open_page>{"page_id":null}</external_codex_apps_open_page>';
  const mixed = renderToStaticMarkup(<MessageBlockView block={{ id: "mixed-page-fixture", role: "user", kind: "message", questions: [], text: `Continue ${page} please` }} />);
  expect(mixed).toContain("user-message-bubble");
  expect(mixed).toContain("Continue  please");
  expect(mixed).toContain('data-timeline-id="mixed-page-fixture"');
  expect(mixed).not.toContain("external_codex_apps_open_page");
  expect(mixed).not.toContain("page_id");

  const internalOnly = renderToStaticMarkup(<MessageBlockView block={{ id: "internal-page-fixture", role: "user", kind: "message", questions: [], text: page }} />);
  expect(internalOnly).toBe("");
  expect(internalOnly).not.toContain("user-message-bubble");
  expect(internalOnly).not.toContain("data-timeline-id");

  expect(renderToStaticMarkup(<CopyReplyButton text={page} />)).toBe("");
});

test("Codex user messages with proposed-plan examples remain user text", () => {
  const html = renderToStaticMarkup(<MessageBlockView block={{ id: "m", role: "user", kind: "message", questions: [], text: "Example: <proposed_plan># text</proposed_plan>" }} />);
  expect(html).toContain("user-message-bubble");
  expect(html).not.toContain("plan-cell");
});

test("normalized body wins over legacy envelope and attachments stay outside the bubble", () => {
  const html = renderToStaticMarkup(<MessageBlockView block={{ id: "m", role: "user", kind: "message", questions: [], text: "legacy envelope", user_message: { id: "m", text: "Request only", attachments: [{ id: "i", name: "sample.png", kind: "image", reason: "Missing" }] } }} />);
  expect(html).not.toContain("legacy envelope");
  expect(html.indexOf("user-attachments")).toBeLessThan(html.indexOf("user-message-bubble"));
  expect(html).toContain("Request only");
});

test("legacy Codex context defaults to one collapsed disclosure and keeps the request visible", () => {
  const text = "# AGENTS.md instructions\n<INSTRUCTIONS>\n# Rules\nGuidance\n</INSTRUCTIONS>\n\nRequest";
  const html = renderToStaticMarkup(<MessageBlockView block={{ id: "legacy", role: "user", kind: "message", questions: [], text }} />);
  expect(html.match(/<details\b/g)).toHaveLength(1);
  expect(html).toContain("上下文");
  expect(html).not.toMatch(/<details[^>]*\bopen=/);
  expect(html).not.toContain("AGENTS.md");
  expect(html).not.toContain("Guidance");
  expect(html).not.toMatch(/\d+ 行 · \d+ 字节/);
  expect(html).toContain("Request");
});

test("adjacent legacy context blocks share one collapsed disclosure without metadata summaries", () => {
  const text = adjacentInstructions + "<environment_context>fixture</environment_context>\n\nVisible request";
  const html = renderToStaticMarkup(<MessageBlockView block={{ id: "adjacent-instruction-fixture", role: "user", kind: "message", questions: [], text }} />);
  expect(html.match(/<details\b/g)).toHaveLength(1);
  expect(html.match(/上下文/g)).toHaveLength(1);
  expect(html).not.toMatch(/<details[^>]*\bopen(?:=|\s|>)/);
  expect(html).not.toContain("AGENTS.md");
  expect(html).not.toContain("Keep checks.");
  expect(html).not.toContain("保留格式🙂");
  expect(html).not.toContain("environment_context");
  expect(html).not.toMatch(/\d+ 行 · \d+ 字节/);
  expect(html).toContain("Visible request");
});

test("structured context renders after attachments and before the real request in one disclosure", () => {
  const html = renderToStaticMarkup(<MessageBlockView block={{
    id: "context-wire-fixture", role: "user", kind: "message", questions: [],
    user_message: {
      id: "native-context-fixture", text: "Actual request body", attachments: [{ id: "attachment", name: "fixture.txt", kind: "file" }],
      contextBlocks: [
        { kind: "instructions", text: "# AGENTS.md instructions\nSynthetic policy fixture." },
        { kind: "environment", text: "Synthetic environment fixture." },
        { kind: "runtime", text: "Synthetic runtime fixture." },
        { kind: "files", text: "Synthetic files fixture." }
      ]
    }
  }} />);
  const attachmentIndex = html.indexOf("user-attachments");
  const disclosureIndex = html.indexOf("<details");
  const requestIndex = html.indexOf("user-message-bubble");
  expect(attachmentIndex).toBeGreaterThanOrEqual(0);
  expect(disclosureIndex).toBeGreaterThan(attachmentIndex);
  expect(requestIndex).toBeGreaterThan(disclosureIndex);
  expect(html.match(/<details\b/g)).toHaveLength(1);
  expect(html).toContain("上下文");
  expect(html).toContain("Actual request body");
  expect(html).not.toMatch(/<details[^>]*\bopen(?:=|\s|>)/);
  expect(html).not.toContain("AGENTS.md");
  expect(html).not.toContain("Synthetic policy fixture.");
  expect(html).not.toContain("Synthetic environment fixture.");
  expect(html).not.toContain("Synthetic runtime fixture.");
  expect(html).not.toContain("Synthetic files fixture.");
  expect(html).not.toMatch(/\d+ 行 · \d+ 字节/);
});

test("context-only messages have no request bubble", () => {
  const html = renderToStaticMarkup(<UserMessage message={{
    id: "context-only-fixture", text: "", attachments: [],
    contextBlocks: [{ kind: "environment", text: "Synthetic context-only fixture." }]
  }} />);
  expect(html).toContain("上下文");
  expect(html).not.toContain("user-message-bubble");
  expect(html).not.toContain("Synthetic context-only fixture.");
});

test("context disclosure stays lazy and scopes its saved choice to machine, thread, and native message", () => {
  const renderMessage = (request: string, activityId: string, contextText: string, messageId = "stable-context-fixture", threadId = "context-refresh-thread") => {
    observedDisclosures.elements = [];
    const html = renderToStaticMarkup(<DisclosureScope.Provider value={threadId}>
      <UserMessage message={{ id: messageId, text: request, attachments: [], contextBlocks: [{ kind: "instructions", text: contextText }] }} activityId={activityId} />
    </DisclosureScope.Provider>);
    return { html, disclosures: [...observedDisclosures.elements] };
  };
  observedDisclosures.machineScope = "local";
  const initial = renderMessage("First request", "initial-activity-fixture", "Initial context fixture.");
  expect(initial.html).not.toMatch(/<details[^>]*\bopen(?:=|\s|>)/);
  expect(initial.html).not.toContain("Initial context fixture.");
  expect(initial.html).toContain("First request");
  expect(initial.disclosures).toHaveLength(1);

  const summary = Children.toArray(initial.disclosures[0].props.children)
    .find(child => isValidElement(child) && child.type === "summary");
  if (!isValidElement<ComponentProps<"summary">>(summary)) throw new Error("Context summary is missing");
  expect(summary.props.onClick).toBeTypeOf("function");
  const preventDefault = vi.fn();
  summary.props.onClick!({ target: { closest: () => null }, preventDefault } as unknown as MouseEvent<HTMLElement>);
  expect(preventDefault).toHaveBeenCalledOnce();

  const refreshed = renderMessage("Updated request", "refreshed-activity-fixture", "Updated context fixture.");
  expect(refreshed.html).toMatch(/<details[^>]*\bopen(?:=|\s|>)/);
  expect(refreshed.html).toContain("上下文");
  expect(refreshed.html).toContain("Updated context fixture.");
  expect(refreshed.html).toContain("Updated request");
  expect(refreshed.html).not.toContain("Initial context fixture.");
  expect(refreshed.html).not.toContain("First request");

  observedDisclosures.machineScope = "remote:https://example.invalid";
  const otherMachine = renderMessage("Remote request", "remote-activity-fixture", "Remote context fixture.");
  expect(otherMachine.html).not.toMatch(/<details[^>]*\bopen(?:=|\s|>)/);
  expect(otherMachine.html).not.toContain("Remote context fixture.");

  observedDisclosures.machineScope = "local";
  const sameMachine = renderMessage("Local request after switch", "local-activity-fixture", "Local context fixture.");
  expect(sameMachine.html).toMatch(/<details[^>]*\bopen(?:=|\s|>)/);
  expect(sameMachine.html).toContain("Local context fixture.");

  const otherMessage = renderMessage("Other request", "other-activity-fixture", "Other context fixture.", "different-context-fixture");
  expect(otherMessage.html).not.toMatch(/<details[^>]*\bopen(?:=|\s|>)/);
  expect(otherMessage.html).not.toContain("Other context fixture.");

  const otherThread = renderMessage("Other thread request", "other-thread-activity-fixture", "Other thread context fixture.", "stable-context-fixture", "different-context-thread");
  expect(otherThread.html).not.toMatch(/<details[^>]*\bopen(?:=|\s|>)/);
  expect(otherThread.html).not.toContain("Other thread context fixture.");
});

test("native question reply renders a question and answer with an answer-only copy action", () => {
  const text = '<send_user_message_question_reply>[{"questionItemId":"example","question":"Please confirm","answer":"Confirmed"}]</send_user_message_question_reply>';
  const html = renderToStaticMarkup(<UserMessage text={text} activityId="reply" />);
  expect(html).toContain("user-question-context");
  expect(html).toContain("Please confirm");
  expect(html).toContain("Confirmed");
  expect(html).toContain('aria-label="复制回答"');
  expect(html).not.toContain("questionItemId");
  expect(html).not.toContain("send_user_message_question_reply");
});


test("child transcript attachments show names without preview or file actions", () => {
  const html = renderToStaticMarkup(<UserMessageScope.Provider value={{ provider: "claude_code", sessionKey: "parent-key", attachments: "names" }}>
    <UserMessage message={{ id: "child-user", text: "", attachments: [{ id: "image", name: "sample.png", kind: "image", path: "/fixture/sample.png" }, { id: "file", name: "notes.txt", kind: "file", path: "/fixture/notes.txt" }] }} />
  </UserMessageScope.Provider>);
  expect(html).toContain("sample.png"); expect(html).toContain("notes.txt");
  expect(html).not.toMatch(/<button|<a |<img|<dialog|attachment-thumbnail|预览|fixture\//);
});
