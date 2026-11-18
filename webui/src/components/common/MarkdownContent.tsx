import { RENDER_CACHE_BUDGET } from "../../lib/domain/resourceBudgets";
import { Check, Copy } from "lucide-react";
import { memo, useContext, useMemo, useState, type ComponentProps, type ReactElement } from "react";
import Markdown, { defaultUrlTransform } from "react-markdown";
import remarkGfm from "remark-gfm";
import { instructionSegments, visibleMarkdown } from "../../lib/domain/visibleMarkdown";
import { ActivityDetails, DisclosureRevealContext } from "./ActivityDetails";
import { matchesTimelinePosition } from "../../lib/domain/timelineViewModel";
import { fileLinkTarget } from "../../lib/domain/fileLinks";
import { FilePathLink } from "./FilePathLink";

type Element = { tagName?: string; children?: Element[] };

// Reuse the element tree built for a text across thread remounts, so a hit
// skips the whole syntax pipeline. The tree is context-free: file links read
// their path scope and code blocks keep their copy state when React renders
// them. An entry weighs its source text, two bytes per UTF-16 code unit.
const compiledMarkdown = new Map<string, { bytes: number; element: ReactElement }>();
let compiledMarkdownBytes = 0;
const compiledMarkdownLimit = RENDER_CACHE_BUDGET.markdownBytes;

function omitMemoryElements() {
  return (tree: Element) => {
    function visit(node: Element) {
      if (node.tagName === "code" || node.tagName === "pre") return;
      node.children = node.children?.filter(child => !/^(oai-mem-citation|citation_entries|rollout_ids|memory_citation)$/i.test(child.tagName ?? ""));
      node.children?.forEach(visit);
    }
    visit(tree);
  };
}

function CodeBlock({ children, ...props }: ComponentProps<"pre">) {
  const [copied, setCopied] = useState(false);
  return <div className="markdown-code"><button className="icon-button" title={copied ? "已复制代码" : "复制代码"} onClick={async (event) => {
    const text = event.currentTarget.parentElement?.querySelector("pre")?.textContent;
    if (!text) return;
    try { await navigator.clipboard.writeText(text); setCopied(true); }
    catch { setCopied(false); }
  }}>{copied ? <Check size={14} /> : <Copy size={14} />}</button><pre {...props}>{children}</pre></div>;
}

const MarkdownBody = memo(function MarkdownBody({ text }: { text: string }) {
  const cached = compiledMarkdown.get(text);
  if (cached) {
    compiledMarkdown.delete(text);
    compiledMarkdown.set(text, cached);
    return cached.element;
  }
  const element = Markdown({
    remarkPlugins: [remarkGfm],
    rehypePlugins: [omitMemoryElements],
    urlTransform: (url, key, node) => key === "href" && node.tagName === "a" && fileLinkTarget(url) ? url : defaultUrlTransform(url),
    components: {
      pre: ({ node: _node, ...props }) => <CodeBlock {...props} />,
      a: ({ node: _node, href, children, ...props }) => href && fileLinkTarget(href)
        ? <FilePathLink href={href}>{children}</FilePathLink>
        : <a {...props} href={href} target={href?.startsWith("#") ? undefined : "_blank"} rel="noopener noreferrer">{children}</a>
    },
    children: text
  });
  const bytes = text.length * 2;
  if (bytes <= compiledMarkdownLimit) {
    while (compiledMarkdown.size >= RENDER_CACHE_BUDGET.entries || compiledMarkdownBytes + bytes > compiledMarkdownLimit) {
      const key = compiledMarkdown.keys().next().value;
      if (key === undefined) break;
      compiledMarkdownBytes -= compiledMarkdown.get(key)!.bytes;
      compiledMarkdown.delete(key);
    }
    compiledMarkdown.set(text, { bytes, element });
    compiledMarkdownBytes += bytes;
  }
  return element;
});

export const MarkdownContent = memo(function MarkdownContent({ text, activityId = "message", timelineAliases = [], foldInstructions = false }: { text: string; activityId?: string; timelineAliases?: string[]; foldInstructions?: boolean }) {
  const cleaned = useMemo(() => visibleMarkdown(text), [text]);
  const segments = useMemo(() => foldInstructions ? instructionSegments(cleaned) : null, [cleaned, foldInstructions]);
  const positionKey = useContext(DisclosureRevealContext);
  const reveal = matchesTimelinePosition(positionKey, activityId, timelineAliases);
  return <div className="markdown-content">{segments ? segments.map(segment => segment.kind === "instructions"
    ? <ActivityDetails key={segment.id} stateKey={`${activityId}:${segment.id}`} className="instruction-file execution-command" initiallyOpen={false} reveal={reveal} summary={<><span className="tool-title">AGENTS.md</span><small>{segment.lines} 行 · {segment.bytes} 字节</small></>}>
      {() => <div className="instruction-file-body"><MarkdownBody text={segment.text} /></div>}
    </ActivityDetails>
    : <MarkdownBody key={segment.id} text={segment.text} />) : <MarkdownBody text={cleaned} />}</div>;
});
