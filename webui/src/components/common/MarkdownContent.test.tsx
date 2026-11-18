import { renderToStaticMarkup } from "react-dom/server";
import type { Options } from "react-markdown";
import { describe, expect, test, vi } from "vitest";
import { MarkdownContent } from "./MarkdownContent";
import { MarkdownPathScope } from "./FilePathLink";

type RemarkPlugin = Extract<NonNullable<Options["remarkPlugins"]>[number], (...args: never[]) => unknown>;

const { cacheBudget, parseCounts } = vi.hoisted(() => ({
  cacheBudget: { entries: 8, instructionsBytes: 8 * 1024 * 1024, contextBytes: 8 * 1024 * 1024, markdownBytes: 64 * 1024 },
  parseCounts: new Map<string, number>()
}));

vi.mock("../../lib/domain/resourceBudgets", () => ({ RENDER_CACHE_BUDGET: cacheBudget }));
vi.mock("remark-gfm", async importOriginal => {
  const actual = await importOriginal<typeof import("remark-gfm")>();
  const observeParse: RemarkPlugin = function () {
    actual.default.call(this);
    const parse = this.parser!;
    this.parser = (text, file) => {
      parseCounts.set(text, (parseCounts.get(text) ?? 0) + 1);
      return parse(text, file);
    };
  };
  return { ...actual, default: observeParse };
});

const cacheSemantics = [
  "[Guide](./guide.md)",
  "",
  "| Format | Value |",
  "| --- | --- |",
  "| GFM | retained |",
  "",
  "- [x] Completed task",
  "",
  "~~Removed text~~ and a note[^footnote].",
  "",
  "[^footnote]: Footnote body.",
  "",
  "[Unsafe](javascript:alert%281%29)",
  "",
  "```ts",
  "const fixture = 1;",
  "```"
].join("\n");

function renderScoped(text: string, cwd: string | null) {
  return renderToStaticMarkup(
    <MarkdownPathScope.Provider value={cwd}><MarkdownContent text={text} /></MarkdownPathScope.Provider>
  );
}

function expectCacheSemantics(html: string) {
  expect(html).toContain("<table>");
  expect(html).toContain("GFM");
  expect(html).toContain("type=\"checkbox\"");
  expect(html).toContain("checked");
  expect(html).toContain("<del>Removed text</del>");
  expect(html).toContain("Footnote body.");
  expect(html).toContain("class=\"markdown-code\"");
  expect(html).toContain("const fixture = 1;");
  expect(html).not.toMatch(/href=[\"']javascript:/i);
}

describe("user-visible Markdown", () => {
  test("does not display trailing Codex memory citation metadata", () => {
    const html = renderToStaticMarkup(<MarkdownContent text={'Finished.\n\n<oai-mem-citation>\n<citation_entries>\nMEMORY.md:1-2|note=[internal]\n</citation_entries>\n<rollout_ids>\nprivate-id\n</rollout_ids>\n</oai-mem-citation>'} />);
    expect(html).toContain("Finished.");
    expect(html).not.toContain("private-id");
    expect(html).not.toContain("MEMORY.md:1-2");
  });

  test("keeps literal metadata examples inside code", () => {
    const html = renderToStaticMarkup(<MarkdownContent text={'Example:\n```xml\n<oai-mem-citation>literal</oai-mem-citation>\n```'} />);
    expect(html).toContain("literal");
    expect(html).toContain("&lt;oai-mem-citation&gt;");
  });

  test("removes inline metadata and folds instructions with a path-free summary", () => {
    const html = renderToStaticMarkup(<MarkdownContent foldInstructions text={'Done.<oai-mem-citation>internal-entry</oai-mem-citation>\n\n## /workspace/AGENTS.md\nRun checks.\n## Result\nOrdinary result.'} />);
    expect(html).not.toContain("internal-entry");
    expect(html).toMatch(/<details[^>]*instruction-file[^>]*>/);
    expect(html).not.toMatch(/<details[^>]*open/);
    const summary = html.match(/<summary>(.*?)<\/summary>/)?.[1];
    expect(summary).toContain("AGENTS.md");
    expect(summary).toContain("字节");
    expect(summary).not.toContain("/workspace");
    expect(html).not.toContain("Run checks.");
    expect(html).toContain("Ordinary result.");
  });

  test("does not mount a long instruction body while its disclosure is closed", () => {
    const body = "Instruction payload fixture with **formatting** and `code`.\n\n".repeat(2048);
    const html = renderToStaticMarkup(<MarkdownContent activityId="long-instructions" foldInstructions text={`## AGENTS.md\n${body}\n## Result\nVisible result fixture.`} />);
    expect(html).toMatch(/<details[^>]*instruction-file[^>]*>/);
    expect(html).not.toMatch(/<details[^>]*open/);
    expect(html).toContain("AGENTS.md");
    expect(html).toContain("字节");
    expect(html.includes("instruction-file-body"), "closed instructions should not mount their body").toBe(false);
    expect(html.includes("Instruction payload fixture"), "closed instructions should not render Markdown").toBe(false);
    expect(html).toContain("Visible result fixture.");
  });

  test("renders instruction-like Markdown normally when folding is disabled", () => {
    const html = renderToStaticMarkup(<MarkdownContent text={"## AGENTS.md\nOrdinary instruction example."} />);
    expect(html).not.toContain("<details");
    expect(html).toContain("Ordinary instruction example.");
  });

  test("reuses GFM parsing without leaking path scope or a transformed tree across server renders", () => {
    const text = [
      "[Guide](./guide.md)",
      "",
      "| Format | Value |",
      "| --- | --- |",
      "| GFM | retained |",
      "",
      "- [x] Completed task",
      "",
      "A note[^footnote].",
      "",
      "[^footnote]: Footnote body.",
      "",
      "[Unsafe](javascript:alert%281%29)",
      "",
      "```ts",
      "const fixture = 1;",
      "```"
    ].join("\n");
    const renderAt = (cwd: string) => renderToStaticMarkup(
      <MarkdownPathScope.Provider value={cwd}><MarkdownContent text={text} /></MarkdownPathScope.Provider>
    );

    const one = renderAt("/fixture/one");
    const two = renderAt("/fixture/two");
    const oneAgain = renderAt("/fixture/one");
    const withoutPath = (html: string) => html.replace(/\/fixture\/(?:one|two)\/guide\.md/g, "/fixture/<cwd>/guide.md");

    expect(one).toContain("：/fixture/one/guide.md");
    expect(one).not.toContain("/fixture/two/guide.md");
    expect(two).toContain("：/fixture/two/guide.md");
    expect(two).not.toContain("/fixture/one/guide.md");
    expect(oneAgain).toContain("：/fixture/one/guide.md");
    for (const html of [one, two, oneAgain]) {
      expect(html).toContain("<table>");
      expect(html).toContain("GFM");
      expect(html).toContain("type=\"checkbox\"");
      expect(html).toContain("checked");
      expect(html).toContain("Footnote body.");
      expect(html).toContain("class=\"markdown-code\"");
      expect(html).toContain("const fixture = 1;");
      expect(html).not.toMatch(/href=[\"']javascript:/i);
    }
    expect(withoutPath(two)).toBe(withoutPath(one));
    expect(withoutPath(oneAgain)).toBe(withoutPath(one));
  });

  test("renders updated text under the same path scope without retaining old content", () => {
    const render = (text: string) => renderToStaticMarkup(
      <MarkdownPathScope.Provider value="/fixture/one"><MarkdownContent text={text} /></MarkdownPathScope.Provider>
    );
    const before = render("Previous body.\n\n[Guide](./old.md)");
    const after = render("Updated body.\n\n[Guide](./new.md)");

    expect(after).toContain("Updated body.");
    expect(after).not.toContain("Previous body.");
    expect(after).toContain("/fixture/one/new.md");
    expect(after).not.toContain("/fixture/one/old.md");
    expect(before).toContain("/fixture/one/old.md");
  });

  test.each(["entries", "bytes"] as const)("preserves semantics and path isolation after %s eviction", kind => {
    const text = `Cache eviction fixture ${kind}.\n\n${cacheSemantics}`;
    const first = renderScoped(text, "/fixture/one");
    const warm = renderScoped(text, "/fixture/two");
    expect(parseCounts.get(text)).toBe(1);
    expect(first).toContain("：/fixture/one/guide.md");
    expect(warm).toContain("：/fixture/two/guide.md");

    // Four fillers stay under the entry limit; each one's text weighs just over
    // half of the byte budget, so only byte pressure can evict the fixture.
    const count = kind === "entries" ? cacheBudget.entries + 1 : 4;
    for (let index = 0; index < count; index += 1) {
      const filler = `Eviction fixture ${kind} ${index}. ${kind === "bytes" ? "x".repeat(cacheBudget.markdownBytes / 4) : "Small body."}`;
      renderScoped(filler, "/fixture/one");
      renderScoped(filler, "/fixture/two");
      expect(parseCounts.get(filler)).toBe(1);
    }

    const rebuilt = renderScoped(text, "/fixture/two");
    expect(parseCounts.get(text)).toBe(2);
    const withoutScope = renderScoped(text, null);
    const firstAgain = renderScoped(text, "/fixture/one");
    expect(parseCounts.get(text)).toBe(2);
    expect(rebuilt).not.toContain("/fixture/one/guide.md");
    expect(withoutScope).toContain("：./guide.md");
    expect(withoutScope).not.toContain("/fixture/");
    expect(firstAgain).not.toContain("/fixture/two/guide.md");
    const normalizePath = (html: string) => html.replace(/\/fixture\/(?:one|two)\/guide\.md/g, "./guide.md");
    for (const html of [warm, rebuilt, withoutScope, firstAgain]) {
      expectCacheSemantics(html);
      expect(normalizePath(html)).toBe(normalizePath(first));
    }
  });

  test("renders oversize uncached Markdown independently in each path scope", () => {
    const text = `Oversize fixture.\n\n${cacheSemantics}\n\n${"x".repeat(cacheBudget.markdownBytes / 2 + 1)}`;
    const scopes = ["/fixture/one", "/fixture/two", null, "/fixture/one"];
    for (const [index, scope] of scopes.entries()) {
      const html = renderScoped(text, scope);
      expectCacheSemantics(html);
      expect(html).toContain(`：${scope ? `${scope}/guide.md` : "./guide.md"}`);
      if (scope !== "/fixture/one") expect(html).not.toContain("/fixture/one/guide.md");
      if (scope !== "/fixture/two") expect(html).not.toContain("/fixture/two/guide.md");
      expect(parseCounts.get(text)).toBe(index + 1);
    }
  });
});
