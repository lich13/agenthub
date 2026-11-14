import { expect, test, type Locator, type Page } from "@playwright/test";
import { mockApi, mockCommand } from "./fixtures";
import type { ClaudeHistoryEvent, ClaudeSessionSummary, GrokHistoryEvent, GrokSessionSummary, HistoryMetadata, MessageBlock, SubagentActivity, ThreadDetail, ThreadSummary } from "../src/types";

const pageMessages = 24;
const totalMessages = pageMessages * 3;
const instructions = "Follow the isolated fixture rules with **formatting** and `example` code.\n\n".repeat(64);
const output = "Tool output payload for the isolated performance fixture.\n".repeat(512);
const summaries: ThreadSummary[] = ["a", "b"].map(key => ({
  id: `performance-${key}`, title: `Performance fixture ${key.toUpperCase()}`,
  status: "Recent", message_count: totalMessages, cwd: "/fixture/workspace"
}));

type Provider = "codex" | "claude" | "grok" | "subagent";
type Request = { before: string | null; id: string };

function historyBlockId(id: string, blockIndex: number) {
  const messageIndex = Math.floor(blockIndex / 2);
  return `${id}-${blockIndex % 2 === 0 ? "reply" : "tool"}-${messageIndex}`;
}

function grokEventId(eventIndex: number) {
  return `grok:${eventIndex}`;
}

function blockIndexFromId(id: string, candidate: string, messageCount: number): number {
  const prefix = `${id}-`;
  if (!candidate.startsWith(prefix)) return -1;
  const match = /^(reply|tool)-(\d+)$/.exec(candidate.slice(prefix.length));
  if (!match) return -1;
  const messageIndex = Number(match[2]);
  if (!Number.isInteger(messageIndex) || messageIndex < 0 || messageIndex >= messageCount) return -1;
  return messageIndex * 2 + (match[1] === "tool" ? 1 : 0);
}

function pageEnd(id: string, before: string | null | undefined, provider: Provider, messageCount: number): number {
  if (!before) return messageCount;
  if (provider === "codex" || provider === "subagent") {
    if (before.startsWith("a:")) {
      const anchor = blockIndexFromId(id, before.slice(2), messageCount);
      if (anchor < 0) throw new Error("Invalid stable fixture block anchor");
      return Math.min(messageCount, Math.floor(anchor / 2) + 1);
    }
    const [kind, rawIndex, ...idParts] = before.split(":");
    const blockIndex = Number(rawIndex);
    if (kind !== "b" || !Number.isInteger(blockIndex) || blockIndex <= 0 || blockIndex % 2 !== 0 || blockIndex > messageCount * 2 || idParts.join(":") !== historyBlockId(id, blockIndex)) {
      throw new Error("Invalid stable fixture history cursor");
    }
    return blockIndex / 2;
  }
  const anchor = blockIndexFromId(id, before, messageCount);
  if (anchor < 0) throw new Error("Invalid native fixture event cursor");
  return Math.ceil(anchor / 2);
}

function message(id: string, text: string): MessageBlock {
  return { id, kind: "message", role: "assistant", text, questions: [] };
}

function historyBlocks(id: string, start: number, end: number, revision: number, messageCount: number): MessageBlock[] {
  return Array.from({ length: end - start }, (_, offset) => {
    const index = start + offset;
    return [
      message(historyBlockId(id, index * 2), `History fixture ${index}.\n\n## AGENTS.md\n${instructions}\n## Reply\n${"Visible history fixture. ".repeat(24)}${index === messageCount - 1 ? `\n\nLatest poll update ${revision}.` : ""}`),
      {
        id: historyBlockId(id, index * 2 + 1), call_id: `${id}-call-${index}`, kind: "function_call_output", role: "tool",
        tool_name: "exec_command", input: `printf fixture-${index}`, text: `Tool output fixture ${index}.\n${output}`,
        status: "completed", questions: []
      }
    ];
  }).flat();
}

function grokEvents(id: string, start: number, end: number, revision: number, messageCount: number): GrokHistoryEvent[] {
  return Array.from({ length: end - start }, (_, offset) => {
    const index = start + offset;
    const callId = `${id}-call-${index}`;
    return [
      {
        id: grokEventId(index * 3), kind: "agent_message_chunk",
        text: `History fixture ${index}.${index === messageCount - 1 ? `\n\nLatest poll update ${revision}.` : ""}`
      },
      { id: grokEventId(index * 3 + 1), callId, kind: "tool_call", method: "exec_command", detail: `printf fixture-${index}`, status: "completed" },
      {
        id: grokEventId(index * 3 + 2), callId, kind: "tool_result", method: "exec_command", status: "completed",
        text: `Tool output fixture ${index}.\n${output}`
      }
    ];
  }).flat();
}

function detail(summary: ThreadSummary, blocks: MessageBlock[], start = 0, history?: HistoryMetadata, messageCount = totalMessages): ThreadDetail {
  return {
    summary: { ...summary, message_count: messageCount }, blocks, history, messages: [], raw_event_count: messageCount * 2, total_blocks: messageCount * 2,
    has_more_blocks: start > 0, before_cursor: start > 0 ? `b:${start * 2}` : null
  };
}

async function installHistory(page: Page, provider: Provider) {
  await mockApi(page);
  const requests: Request[] = [];
  let revision = 0;
  let messageCount = totalMessages;
  const changedEventId = (id: string) => provider === "grok" ? grokEventId((messageCount - 1) * 3) : historyBlockId(id, messageCount * 2 - 2);
  const historyFor = (id: string): HistoryMetadata => ({
    epoch: `performance:${id}`, revision: `revision:${revision}`,
    changes: Array.from({ length: revision + 1 }, (_, index) => ({
      revision: `revision:${index}`, changedIds: index ? [changedEventId(id)] : []
    }))
  });
  const pageFor = (id: string, before?: string | null) => {
    requests.push({ id, before: before ?? null });
    if (provider === "grok") {
      const events = grokEvents(id, 0, messageCount, revision, messageCount);
      const end = before ? events.findIndex(event => event.id === before) : events.length;
      if (end < 0) throw new Error("Invalid native Grok fixture event cursor");
      const start = Math.max(0, end - pageMessages * 3);
      return { start, end, blocks: [] as MessageBlock[], events: events.slice(start, end), history: historyFor(id), hasMore: start > 0, beforeCursor: start > 0 ? events[start].id : null };
    }
    const end = pageEnd(id, before, provider, messageCount);
    const start = Math.max(0, end - pageMessages);
    return {
      start, end, blocks: historyBlocks(id, start, end, revision, messageCount), events: [] as GrokHistoryEvent[], history: historyFor(id),
      hasMore: start > 0, beforeCursor: start > 0 ? historyBlockId(id, start * 2) : null
    };
  };
  await mockCommand(page, "threads.list", () => summaries.map(summary => ({ ...summary, message_count: messageCount })));
  if (provider === "codex") {
    await mockCommand(page, "threads.detail", args => {
      const summary = summaries.find(row => row.id === args.id)!;
      const window = pageFor(summary.id, args.options?.before);
      return detail(summary, window.blocks, window.start, window.history, messageCount);
    });
    await mockCommand(page, "threads.blocks", args => {
      const window = pageFor(args.id, args.options?.before);
      return {
        threadId: args.id, blocks: window.blocks, history: window.history, totalBlocks: messageCount * 2,
        hasMoreBlocks: window.start > 0, beforeCursor: window.start > 0 ? `b:${window.start * 2}` : null
      };
    });
  } else if (provider === "claude") {
    const summary: ClaudeSessionSummary = {
      id: "performance-claude", sessionKey: "claude:performance", title: "Claude performance fixture",
      cwd: "/fixture/workspace", path: "/fixture/session.jsonl", status: "recent", formatVersion: "2.1.284",
      messageCount, canRename: true, canDelete: true
    };
    await mockCommand(page, "claude.list", () => [{ ...summary, messageCount }]);
    await mockCommand(page, "claude.detail", args => {
      const window = pageFor(summary.id, args.before);
      const events: ClaudeHistoryEvent[] = window.blocks.map(block => block.role === "tool"
        ? { id: block.id, callId: block.call_id, kind: "tool_call", role: "Bash", detail: block.input, result: block.text, status: "completed" }
        : { id: block.id, kind: "assistant_message", text: block.text });
      return { summary: { ...summary, messageCount }, events, history: window.history, totalEvents: messageCount * 2, hasMore: window.start > 0, beforeCursor: window.start > 0 ? historyBlockId(summary.id, window.start * 2) : null };
    });
  } else if (provider === "grok") {
    const summary: GrokSessionSummary = {
      id: "performance-grok", title: "Grok performance fixture", cwd: "/fixture/workspace",
      path: "/fixture/session.jsonl", messageCount, status: "recent"
    };
    await mockCommand(page, "grok.list", () => [{ ...summary, messageCount }]);
    await mockCommand(page, "grok.detail", args => {
      const window = pageFor(summary.id, args.before);
      return {
        summary: { ...summary, messageCount }, events: window.events,
        history: window.history, totalEvents: messageCount * 3, hasMore: window.hasMore,
        beforeCursor: window.beforeCursor
      };
    });
  } else {
    const agent: SubagentActivity = { agentId: "performance-child", name: "Performance child fixture", available: true, status: "running" };
    const root = detail(summaries[0], [
      message("root-message", "Parent fixture remains independently scrollable."),
      { id: "child-activity", role: "tool", kind: "function_call", tool_name: "spawn_agent", questions: [], subagent: agent }
    ], 0, historyFor(summaries[0].id), messageCount);
    await mockCommand(page, "threads.detail", () => root);
    await mockCommand(page, "threads.subagentDetail", args => {
      const window = pageFor(agent.agentId!, args.request.before);
      return {
        rootThreadId: summaries[0].id, parentThreadId: summaries[0].id, agent,
        detail: detail({ ...summaries[0], id: agent.agentId!, title: agent.name }, window.blocks, window.start, window.history, messageCount)
      };
    });
  }
  const stream = page.locator(provider === "claude" || provider === "grok" ? ".provider-events" : provider === "subagent" ? ".subagent-stream" : ".conversation-main .message-stream");
  return {
    requests, stream,
    updateLatest: () => { revision += 1; },
    appendLatest: () => { messageCount += 1; revision += 1; },
    open: async () => {
      await page.goto("/");
      if (provider === "claude" || provider === "grok") await page.locator(".side-nav").getByRole("button", { name: provider === "claude" ? "Claude Code" : "Grok Build", exact: true }).click();
      else {
        await page.locator(".thread-item").filter({ hasText: summaries[0].title }).click();
        if (provider === "subagent") await page.getByRole("button", { name: /Performance child fixture/ }).click();
      }
      await expect(stream.locator("details.execution-group")).toHaveCount(pageMessages);
      await expect.poll(() => stream.evaluate(node => node.scrollTop >= node.scrollHeight - node.clientHeight - 1)).toBe(true);
    }
  };
}

function historyId(provider: Provider) {
  if (provider === "claude") return "performance-claude";
  if (provider === "grok") return "performance-grok";
  if (provider === "subagent") return "performance-child";
  return summaries[0].id;
}

function expectedOlderCursors(provider: Provider): string[] {
  const id = historyId(provider);
  const starts = [totalMessages - pageMessages, totalMessages - pageMessages * 2];
  return starts.map(start => provider === "codex" || provider === "subagent"
    ? `b:${start * 2}:${historyBlockId(id, start * 2)}`
    : provider === "grok" ? grokEventId(start * 3) : historyBlockId(id, start * 2));
}

async function loadRemainingPages(stream: Locator) {
  for (const count of [pageMessages * 2, totalMessages]) {
    await stream.evaluate(node => {
      node.scrollTop = Math.max(201, node.scrollHeight - node.clientHeight);
      node.dispatchEvent(new Event("scroll", { bubbles: true }));
      node.scrollTop = 120;
      node.dispatchEvent(new Event("scroll", { bubbles: true }));
    });
    await expect(stream.locator("details.execution-group")).toHaveCount(count);
    await expect(stream.locator(".history-loading")).toHaveCount(0);
  }
}

for (const provider of ["codex", "claude", "grok", "subagent"] as const) {
  test(`${provider} polling refreshes only the latest page after loading three long history pages`, async ({ page }) => {
    await page.clock.install();
    const fixture = await installHistory(page, provider);
    await fixture.open();
    expect(fixture.requests.filter(request => request.before)).toEqual([]);
    await loadRemainingPages(fixture.stream);
    expect(fixture.requests.filter(request => request.before).map(request => request.before)).toEqual(expectedOlderCursors(provider));
    expect(fixture.requests.filter(request => request.before)).toHaveLength(2);
    if (provider === "codex") {
      await fixture.stream.getByRole("button", { name: "显示全部历史", exact: true }).first().click();
      await expect(fixture.stream.getByText("History fixture 0.", { exact: true })).toHaveCount(1);
    }
    const group = fixture.stream.locator("details.execution-group").first();
    await group.locator(":scope > summary").click();
    const command = group.locator("details.execution-command").first();
    await command.locator(":scope > summary").click();
    await expect(command.locator(".execution-section").last()).toContainText("Tool output fixture 0.");
    for (let revision = 1; revision <= 2; revision++) {
      const latestReads = fixture.requests.filter(request => !request.before).length;
      fixture.updateLatest();
      await page.clock.fastForward(6000);
      await expect.poll(() => fixture.requests.filter(request => !request.before).length).toBeGreaterThan(latestReads);
      await expect(fixture.stream.getByText(`Latest poll update ${revision}.`, { exact: true })).toHaveCount(1);
      expect(fixture.requests.filter(request => request.before).map(request => request.before)).toEqual(expectedOlderCursors(provider));
      expect(fixture.requests.filter(request => request.before)).toHaveLength(2);
      await expect(fixture.stream.locator("details.execution-group")).toHaveCount(totalMessages);
      await expect(fixture.stream.getByText("History fixture 0.", { exact: true })).toHaveCount(1);
      await expect(command).toHaveAttribute("open", "");
    }
  });
}

test("Grok polling appends a new event without refetching the loaded older pages", async ({ page }) => {
  await page.clock.install();
  const fixture = await installHistory(page, "grok");
  await fixture.open();
  await loadRemainingPages(fixture.stream);
  const oldPageCursors = expectedOlderCursors("grok");
  const latestReads = fixture.requests.filter(request => !request.before).length;
  fixture.appendLatest();
  await page.clock.fastForward(6000);
  await expect.poll(() => fixture.requests.filter(request => !request.before).length).toBeGreaterThan(latestReads);
  await expect(fixture.stream.locator("details.execution-group")).toHaveCount(totalMessages + 1);
  expect(fixture.requests.filter(request => request.before).map(request => request.before)).toEqual(oldPageCursors);
  expect(fixture.requests.filter(request => request.before)).toHaveLength(2);
  const appendedMessageId = `grok:${grokEventId(totalMessages * 3)}:0`;
  const appendedMessage = fixture.stream.locator(`[data-timeline-id="${appendedMessageId}"]`);
  await expect(appendedMessage).toHaveCount(1);
  await expect(appendedMessage).toContainText(`History fixture ${totalMessages}.`);
  const appendedGroup = fixture.stream.locator("details.execution-group").last();
  await expect(appendedGroup).toHaveCount(1);
  const messageIndex = await appendedMessage.evaluate(element => Array.from(element.parentElement!.children).indexOf(element));
  const groupIndex = await appendedGroup.evaluate(element => Array.from(element.parentElement!.children).indexOf(element));
  expect(messageIndex).toBeLessThan(groupIndex);
  await appendedGroup.locator(":scope > summary").click();
  const command = appendedGroup.locator("details.execution-command").first();
  await expect(command.locator(".command-preview")).toContainText(`printf fixture-${totalMessages}`);
  await command.locator(":scope > summary").click();
  await expect(command.locator(".execution-section").last()).toContainText(`Tool output fixture ${totalMessages}.`);
});

async function measureToggle(summary: Locator, open: boolean) {
  await summary.scrollIntoViewIfNeeded();
  return summary.evaluate(async (element, expectedOpen) => {
    const started = performance.now();
    (element as HTMLElement).click();
    await new Promise<void>(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve())));
    if ((element.parentElement as HTMLDetailsElement).open !== expectedOpen) throw new Error("Disclosure did not reach its expected state");
    return performance.now() - started;
  }, open);
}

function p95(samples: number[]) {
  return [...samples].sort((left, right) => left - right)[Math.ceil(samples.length * 0.95) - 1];
}

type DiagnosticEntryType = "longtask" | "long-animation-frame";
type DiagnosticSupport = "supported" | "unsupported" | "error";
type DiagnosticEntry = {
  type: DiagnosticEntryType;
  durationMs: number;
  blockingDurationMs?: number;
  scriptCount?: number;
  scriptDurationMs?: number;
  forcedStyleAndLayoutDurationMs?: number;
};
type DiagnosticCaptureState = {
  support: Record<DiagnosticEntryType, DiagnosticSupport>;
  counts: Record<DiagnosticEntryType, number>;
  records: DiagnosticEntry[];
  observers: Array<{ type: DiagnosticEntryType; observer: PerformanceObserver }>;
  collect: (type: DiagnosticEntryType, entries: PerformanceEntry[]) => void;
};
type DiagnosticWindow = Window & { __desktopPerformanceCapture?: DiagnosticCaptureState };
type EntryDiagnostics = {
  observers: Record<DiagnosticEntryType, { status: DiagnosticSupport; count: number | null }>;
  records: { collected: number; retained: number; dropped: number; truncated: boolean; slowest: DiagnosticEntry[] };
};
type FrameBaseline =
  | { status: "measured"; samples: number; p95Ms: number; maxMs: number }
  | { status: "error"; samples: 0; p95Ms: null; maxMs: null };
type SwitchTiming = {
  clickReturnMs: number;
  observerTitleMs: number | null;
  firstRafMs: number | null;
  readyRafMs: number | null;
  readyPostFrameTaskMs: number | null;
  totalRafMs: number;
  waitFrames: number;
};
type SwitchMetric = {
  samples: number;
  unknown: number;
  p95: number | null;
  max: number | null;
  slowest: Array<{ sample: number; value: number }>;
};

function summarizeSwitchMetric(values: Array<number | null>): SwitchMetric {
  const measured: Array<{ sample: number; value: number }> = [];
  values.forEach((value, index) => {
    if (value !== null) measured.push({ sample: index + 1, value });
  });
  if (!measured.length) return { samples: 0, unknown: values.length, p95: null, max: null, slowest: [] };
  return {
    samples: measured.length,
    unknown: values.length - measured.length,
    p95: Number(p95(measured.map(sample => sample.value)).toFixed(1)),
    max: Number(Math.max(...measured.map(sample => sample.value)).toFixed(1)),
    slowest: [...measured]
      .sort((left, right) => right.value - left.value || left.sample - right.sample)
      .slice(0, 5)
      .map(({ sample, value }) => ({ sample, value: Number(value.toFixed(1)) }))
  };
}

function summarizeSwitchMilliseconds(values: Array<number | null>) {
  const summary = summarizeSwitchMetric(values);
  return {
    samples: summary.samples,
    unknown: summary.unknown,
    p95Ms: summary.p95,
    maxMs: summary.max,
    slowest: summary.slowest.map(({ sample, value }) => ({ sample, ms: value }))
  };
}

function summarizeSwitchFrames(values: number[]) {
  const summary = summarizeSwitchMetric(values);
  return {
    samples: summary.samples,
    unknown: summary.unknown,
    p95Frames: summary.p95,
    maxFrames: summary.max,
    slowest: summary.slowest.map(({ sample, value }) => ({ sample, frames: value }))
  };
}

function unavailableEntryDiagnostics(): EntryDiagnostics {
  return {
    observers: {
      longtask: { status: "error", count: null },
      "long-animation-frame": { status: "error", count: null }
    },
    records: { collected: 0, retained: 0, dropped: 0, truncated: false, slowest: [] }
  };
}

async function measureDoubleRafBaseline(page: Page): Promise<FrameBaseline> {
  try {
    const samples = await page.evaluate(async () => {
      const values: number[] = [];
      for (let sample = 0; sample < 20; sample++) {
        const started = performance.now();
        await new Promise<void>(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve())));
        values.push(performance.now() - started);
      }
      return values;
    });
    if (!samples.length) return { status: "error", samples: 0, p95Ms: null, maxMs: null };
    return {
      status: "measured", samples: samples.length,
      p95Ms: Number(p95(samples).toFixed(1)), maxMs: Number(Math.max(...samples).toFixed(1))
    };
  } catch {
    return { status: "error", samples: 0, p95Ms: null, maxMs: null };
  }
}

async function startPerformanceEntryCapture(page: Page): Promise<boolean> {
  try {
    await page.evaluate(() => {
      const diagnosticWindow = window as DiagnosticWindow;
      const entryTypes: DiagnosticEntryType[] = ["longtask", "long-animation-frame"];
      const support: Record<DiagnosticEntryType, DiagnosticSupport> = {
        longtask: "unsupported", "long-animation-frame": "unsupported"
      };
      const counts: Record<DiagnosticEntryType, number> = { longtask: 0, "long-animation-frame": 0 };
      const records: DiagnosticEntry[] = [];
      const observers: DiagnosticCaptureState["observers"] = [];
      const round = (value: number) => Math.round(value * 10) / 10;
      const numeric = (value: unknown) => typeof value === "number" && Number.isFinite(value) ? value : undefined;
      const capture: DiagnosticCaptureState = {
        support, counts, records, observers,
        collect: (type, entries) => {
          for (const entry of entries) {
            counts[type] += 1;
            const duration = numeric(entry.duration);
            if (duration === undefined) continue;
            const record: DiagnosticEntry = { type, durationMs: round(duration) };
            const candidate = entry as PerformanceEntry & {
              blockingDuration?: unknown;
              scripts?: Array<{ duration?: unknown; forcedStyleAndLayoutDuration?: unknown }>;
            };
            const blockingDuration = numeric(candidate.blockingDuration);
            if (blockingDuration !== undefined) record.blockingDurationMs = round(blockingDuration);
            if (Array.isArray(candidate.scripts)) {
              record.scriptCount = candidate.scripts.length;
              const scriptDurations = candidate.scripts.map(script => numeric(script.duration)).filter((value): value is number => value !== undefined);
              const layoutDurations = candidate.scripts.map(script => numeric(script.forcedStyleAndLayoutDuration)).filter((value): value is number => value !== undefined);
              if (scriptDurations.length) record.scriptDurationMs = round(scriptDurations.reduce((sum, value) => sum + value, 0));
              if (layoutDurations.length) record.forcedStyleAndLayoutDurationMs = round(layoutDurations.reduce((sum, value) => sum + value, 0));
            }
            const insertionIndex = records.findIndex(existing => record.durationMs > existing.durationMs);
            if (insertionIndex >= 0) {
              records.splice(insertionIndex, 0, record);
              if (records.length > 100) records.pop();
            } else if (records.length < 100) {
              records.push(record);
            }
          }
        }
      };
      diagnosticWindow.__desktopPerformanceCapture = capture;
      const supportedTypes = typeof PerformanceObserver !== "undefined" && Array.isArray(PerformanceObserver.supportedEntryTypes)
        ? PerformanceObserver.supportedEntryTypes : [];
      for (const type of entryTypes) {
        if (!supportedTypes.includes(type)) continue;
        try {
          const observer = new PerformanceObserver(list => capture.collect(type, list.getEntries()));
          observer.observe({ type, buffered: false });
          observers.push({ type, observer });
          support[type] = "supported";
        } catch {
          support[type] = "error";
        }
      }
    });
    return true;
  } catch {
    return false;
  }
}

async function stopPerformanceEntryCapture(page: Page, started: boolean): Promise<EntryDiagnostics> {
  if (!started) return unavailableEntryDiagnostics();
  try {
    return await page.evaluate(() => {
      const diagnosticWindow = window as DiagnosticWindow;
      const capture = diagnosticWindow.__desktopPerformanceCapture;
      if (!capture) return {
        observers: {
          longtask: { status: "error", count: null },
          "long-animation-frame": { status: "error", count: null }
        },
        records: { collected: 0, retained: 0, dropped: 0, truncated: false, slowest: [] }
      } satisfies EntryDiagnostics;
      for (const { type, observer } of capture.observers) {
        try {
          capture.collect(type, observer.takeRecords());
        } catch {
          capture.support[type] = "error";
        } finally {
          try {
            observer.disconnect();
          } catch {
            capture.support[type] = "error";
          }
        }
      }
      const countFor = (type: DiagnosticEntryType) => capture.support[type] === "supported" ? capture.counts[type] : null;
      const collected = capture.counts.longtask + capture.counts["long-animation-frame"];
      const result: EntryDiagnostics = {
        observers: {
          longtask: { status: capture.support.longtask, count: countFor("longtask") },
          "long-animation-frame": { status: capture.support["long-animation-frame"], count: countFor("long-animation-frame") }
        },
        records: {
          collected, retained: capture.records.length, dropped: Math.max(0, collected - capture.records.length),
          truncated: collected > capture.records.length, slowest: capture.records.slice(0, 5)
        }
      };
      delete diagnosticWindow.__desktopPerformanceCapture;
      return result;
    });
  } catch {
    return unavailableEntryDiagnostics();
  }
}

test("long history expansion, scrolling and cached thread switching record interaction timings", { tag: "@desktop-performance" }, async ({ page }, info) => {
  test.setTimeout(60000);
  await page.emulateMedia({ reducedMotion: "reduce" });
  await page.goto("about:blank");
  const blankFrameBaseline = await measureDoubleRafBaseline(page);
  const fixture = await installHistory(page, "codex");
  await fixture.open();
  await loadRemainingPages(fixture.stream);
  const entryCaptureStarted = await startPerformanceEntryCapture(page);
  const samples: Record<"expand" | "scroll" | "switch", number[]> = { expand: [], scroll: [], switch: [] };
  const switchTimings: SwitchTiming[] = [];
  const group = fixture.stream.locator("details.execution-group").first();
  const groupSummary = group.locator(":scope > summary");
  const instruction = fixture.stream.locator("details.instruction-file").first();
  let entryDiagnostics = unavailableEntryDiagnostics();
  try {
    for (let sample = 0; sample < 10; sample++) {
      samples.expand.push(await measureToggle(groupSummary, true));
      const command = group.locator("details.execution-command").first();
      await expect(command.locator(".execution-section")).toHaveCount(0);
      samples.expand.push(await measureToggle(command.locator(":scope > summary"), true));
      await expect(command.locator(".execution-section").last()).toContainText("Tool output fixture 0.");
      samples.expand.push(await measureToggle(command.locator(":scope > summary"), false));
      await expect(command.locator(".execution-section")).toHaveCount(0);
      samples.expand.push(await measureToggle(groupSummary, false));
      await expect(instruction.locator(".instruction-file-body")).toHaveCount(0);
      samples.expand.push(await measureToggle(instruction.locator(":scope > summary"), true));
      await expect(instruction.locator(".instruction-file-body")).toContainText("Follow the isolated fixture rules");
      samples.expand.push(await measureToggle(instruction.locator(":scope > summary"), false));
    }
    for (let sample = 0; sample < 20; sample++) {
      samples.scroll.push(await fixture.stream.evaluate(async (element, index) => {
        const started = performance.now();
        element.scrollTop = (element.scrollHeight - element.clientHeight) * (index % 2 ? 0.25 : 0.75);
        element.dispatchEvent(new Event("scroll", { bubbles: true }));
        await new Promise<void>(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve())));
        return performance.now() - started;
      }, sample));
    }
    const second = page.locator(".thread-item").filter({ hasText: summaries[1].title });
    await second.click();
    await expect(page.locator(".conversation-title")).toHaveText(summaries[1].title);
    await expect(fixture.stream.locator("details.execution-group")).toHaveCount(pageMessages);
    await loadRemainingPages(fixture.stream);
    for (let sample = 0; sample < 20; sample++) {
      const title = summaries[sample % 2].title;
      const row = page.locator(".thread-item").filter({ hasText: title });
      const timing = await row.evaluate(async (element, expectedTitle) => {
        const titleElement = document.querySelector(".conversation-title");
        if (!titleElement) throw new Error("Conversation title is missing");
        let started = 0;
        let observerTitleMs: number | null = null;
        let firstRafMs: number | null = null;
        let readyRafMs: number | null = null;
        let readyPostFrameTaskMs: number | null = null;
        let readyPostFrameTaskTimer: number | undefined;
        let waitFrames = 0;
        const observer = new MutationObserver(() => {
          if (observerTitleMs === null && titleElement.textContent === expectedTitle) {
            observerTitleMs = performance.now() - started;
            observer.disconnect();
          }
        });
        try {
          observer.observe(titleElement, { childList: true, characterData: true, subtree: true });
          started = performance.now();
          (element as HTMLElement).click();
          const clickReturnMs = performance.now() - started;
          await new Promise<void>((resolve, reject) => {
            const ready = () => requestAnimationFrame(() => {
              waitFrames += 1;
              if (firstRafMs === null) firstRafMs = performance.now() - started;
              if (document.querySelector(".conversation-title")?.textContent === expectedTitle) {
                readyRafMs = performance.now() - started;
                readyPostFrameTaskTimer = window.setTimeout(() => {
                  readyPostFrameTaskMs = performance.now() - started;
                  readyPostFrameTaskTimer = undefined;
                }, 0);
                requestAnimationFrame(() => {
                  waitFrames += 1;
                  resolve();
                });
              } else if (performance.now() - started > 5000) reject(new Error("Thread did not switch to its expected title"));
              else ready();
            });
            ready();
          });
          return {
            clickReturnMs, observerTitleMs, firstRafMs, readyRafMs, readyPostFrameTaskMs,
            totalRafMs: performance.now() - started, waitFrames
          } satisfies SwitchTiming;
        } finally {
          observer.disconnect();
          if (readyPostFrameTaskTimer !== undefined) window.clearTimeout(readyPostFrameTaskTimer);
        }
      }, title);
      samples.switch.push(timing.totalRafMs);
      switchTimings.push(timing);
      await expect(fixture.stream.locator("details.execution-group")).toHaveCount(totalMessages);
    }
  } finally {
    entryDiagnostics = await stopPerformanceEntryCapture(page, entryCaptureStarted);
  }
  const appFrameBaseline = await measureDoubleRafBaseline(page);
  const measurements = Object.fromEntries(Object.entries(samples).map(([name, values]) => [name, { samples: values.length, p95Ms: Number(p95(values).toFixed(1)), maxMs: Number(Math.max(...values).toFixed(1)) }]));
  const switchDiagnostics = {
    clickReturnMs: summarizeSwitchMilliseconds(switchTimings.map(timing => timing.clickReturnMs)),
    observerTitleMs: summarizeSwitchMilliseconds(switchTimings.map(timing => timing.observerTitleMs)),
    firstRafMs: summarizeSwitchMilliseconds(switchTimings.map(timing => timing.firstRafMs)),
    readyRafMs: summarizeSwitchMilliseconds(switchTimings.map(timing => timing.readyRafMs)),
    readyPostFrameTaskMs: summarizeSwitchMilliseconds(switchTimings.map(timing => timing.readyPostFrameTaskMs)),
    totalRafMs: summarizeSwitchMilliseconds(switchTimings.map(timing => timing.totalRafMs)),
    waitFrames: summarizeSwitchFrames(switchTimings.map(timing => timing.waitFrames))
  };
  const result = {
    browser: info.project.name, historyMessages: totalMessages, ...measurements,
    diagnostics: { doubleRafMs: { aboutBlank: blankFrameBaseline, appIdle: appFrameBaseline }, performanceEntries: entryDiagnostics, switch: switchDiagnostics }
  };
  info.annotations.push({ type: "performance", description: JSON.stringify(result) });
  console.log(`Local interaction measurements and frame diagnostics: ${JSON.stringify(result)}`);
});
