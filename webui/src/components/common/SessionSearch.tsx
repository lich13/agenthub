import { ArrowDown, ArrowUp, Search, X } from "lucide-react";
import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type KeyboardEvent as ReactKeyboardEvent, type ReactNode } from "react";
import { useSessionSearch } from "../../lib/query/search";
import { useConnection } from "../../lib/query/connection";
import { matchesTimelinePosition } from "../../lib/domain/timelineViewModel";
import type { SearchProvider as SessionProvider, SearchScope, SessionSearchRequest, SessionSearchResult } from "../../types";

type SearchWorkspaceRegistration = {
  provider: SessionProvider;
  sessionKey: string | null;
  selectResult: (result: SessionSearchResult) => void;
};

type SearchContextValue = {
  workspace: SearchWorkspaceRegistration | null;
  register: (workspace: SearchWorkspaceRegistration) => () => void;
};

const SearchContext = createContext<SearchContextValue | null>(null);

export function SearchWorkspaceProvider({ children }: { children: ReactNode }) {
  const [workspace, setWorkspace] = useState<SearchWorkspaceRegistration | null>(null);
  const register = useCallback((next: SearchWorkspaceRegistration) => {
    setWorkspace(current => current?.provider === next.provider && current.sessionKey === next.sessionKey ? current : next);
    return () => setWorkspace(current => current === next ? null : current);
  }, []);
  return <SearchContext.Provider value={{ workspace, register }}>{children}<SearchPanel /></SearchContext.Provider>;
}

export function useSearchWorkspace(workspace: SearchWorkspaceRegistration) {
  const context = useContext(SearchContext);
  const selectResultRef = useRef(workspace.selectResult);
  selectResultRef.current = workspace.selectResult;
  const stableWorkspace = useMemo<SearchWorkspaceRegistration>(() => ({
    provider: workspace.provider,
    sessionKey: workspace.sessionKey,
    selectResult: result => selectResultRef.current(result)
  }), [workspace.provider, workspace.sessionKey]);
  useEffect(() => context?.register(stableWorkspace), [context, stableWorkspace]);
}

const providerLabels: Record<SessionProvider, string> = {
  codex: "Codex",
  claude_code: "Claude Code",
  grok: "Grok Build",
  antigravity: "Antigravity",
  dsh: "DeepSeek Harness",
};

const kindLabels: Record<string, string> = {
  metadata: "线程信息",
  user_message: "用户消息",
  assistant_message: "助手回复",
  thinking: "思考",
  plan: "计划",
  tool_call: "工具",
  tool_result: "工具结果",
  exec: "命令"
};

function labelForKind(kind: string): string {
  return kindLabels[kind] ?? kind.replace(/_/g, " ");
}

export function SearchPanel() {
  const context = useContext(SearchContext);
  const workspace = context?.workspace ?? null;
  const connection = useConnection();
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [scope, setScope] = useState<SearchScope>("thread");
  const [cursor, setCursor] = useState<string | null>(null);
  const [results, setResults] = useState<SessionSearchResult[]>([]);
  const [nextCursor, setNextCursor] = useState<string | null>(null);
  const [activeIndex, setActiveIndex] = useState(0);
  const [feedback, setFeedback] = useState("");
  const inputRef = useRef<HTMLInputElement>(null);
  const request = useMemo<SessionSearchRequest | null>(() => workspace && query.trim() ? {
    provider: workspace.provider,
    scope: scope === "thread" && workspace.sessionKey ? "thread" : "provider",
    sessionKey: scope === "thread" ? workspace.sessionKey : null,
    query,
    cursor,
    limit: 50
  } : null, [workspace, scope, query, cursor]);
  const search = useSessionSearch(request);

  useEffect(() => {
    const handler = (event: KeyboardEvent) => {
      if (!(event.metaKey || event.ctrlKey) || event.key.toLowerCase() !== "f") return;
      event.preventDefault();
      setOpen(true);
      window.setTimeout(() => inputRef.current?.focus(), 0);
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, []);

  useEffect(() => {
    setScope(workspace?.sessionKey ? "thread" : "provider");
  }, [workspace?.provider, workspace?.sessionKey]);

  useEffect(() => {
    setCursor(null);
    setResults([]);
    setNextCursor(null);
    setActiveIndex(0);
    setFeedback("");
  }, [connection.revision, connection.target, connection.sshHost, workspace?.provider, workspace?.sessionKey, scope, query]);

  useEffect(() => {
    if (!search.data) return;
    if (search.data.query !== query.trim() || search.data.provider !== workspace?.provider || search.data.scope !== request?.scope) return;
    setResults(current => cursor ? [...current, ...search.data!.results.filter(item => !current.some(existing => existing.resultId === item.resultId))] : search.data!.results);
    setNextCursor(search.data.nextCursor ?? null);
  }, [search.data, cursor, query, workspace?.provider, request?.scope]);

  const close = () => { setOpen(false); setFeedback(""); };
  const select = (result: SessionSearchResult) => {
    if (!workspace) return;
    workspace.selectResult(result);
    close();
  };
  const move = (delta: number) => {
    if (!results.length) return;
    setActiveIndex(index => (index + delta + results.length) % results.length);
  };
  const onInputKeyDown = (event: ReactKeyboardEvent<HTMLInputElement>) => {
    if (event.key === "Escape") { event.preventDefault(); close(); return; }
    if (event.key === "ArrowDown") { event.preventDefault(); move(1); return; }
    if (event.key === "ArrowUp") { event.preventDefault(); move(-1); return; }
    if (event.key === "Enter" && results[activeIndex]) { event.preventDefault(); select(results[activeIndex]); }
  };
  const currentTarget = results[activeIndex];
  return <>
    <button className="global-search-trigger" type="button" title="搜索会话（⌘F / Ctrl+F）" onClick={() => { setOpen(true); window.setTimeout(() => inputRef.current?.focus(), 0); }}><Search size={15} /><span>搜索</span><kbd>⌘F</kbd></button>
    {open && <div className="search-overlay" role="dialog" aria-modal="true" aria-label="搜索会话" onMouseDown={event => { if (event.target === event.currentTarget) close(); }}>
      <section className="session-search-panel">
        <header className="session-search-header"><strong>搜索会话</strong><button className="icon-button" type="button" title="关闭搜索" onClick={close}><X size={17} /></button></header>
        <div className="session-search-input"><Search size={17} /><input ref={inputRef} value={query} onChange={event => setQuery(event.target.value)} onKeyDown={onInputKeyDown} placeholder={workspace ? `搜索 ${providerLabels[workspace.provider]}` : "没有可搜索的会话"} disabled={!workspace} autoComplete="off" /></div>
        <div className="session-search-scopes" role="tablist" aria-label="搜索范围">
          <button type="button" role="tab" aria-selected={scope === "thread"} disabled={!workspace?.sessionKey} onClick={() => setScope("thread")}>当前线程</button>
          <button type="button" role="tab" aria-selected={scope === "provider"} onClick={() => setScope("provider")}>当前 Provider</button>
          {results.length > 0 && <span className="muted-text">{results.length}{nextCursor ? "+" : ""} 个结果</span>}
        </div>
        {search.isFetching && <div className="muted-row">正在搜索...</div>}
        {search.error && <div role="alert" className="form-error">{search.error.message}</div>}
        {search.data?.warnings?.map(warning => <div key={warning} role="status" className="task-feedback">{warning}</div>)}
        {!search.isFetching && !search.error && query.trim() && !results.length && <div className="muted-row">没有匹配内容</div>}
        <div className="session-search-results" role="listbox" aria-label="搜索结果">
          {results.map((result, index) => <button key={result.resultId} type="button" role="option" aria-selected={index === activeIndex} className={`session-search-result ${index === activeIndex ? "active" : ""}`} onMouseEnter={() => setActiveIndex(index)} onClick={() => select(result)}>
            <span className="search-result-heading"><strong>{result.title}</strong><small>{labelForKind(result.matchKind)}</small></span>
            <span className="search-result-snippet">{result.snippet}</span>
            <small className="muted-text">{result.timestamp ? new Date(result.timestamp).toLocaleString() : result.nativeId}</small>
          </button>)}
        </div>
        {nextCursor && <button type="button" className="secondary-button search-load-more" disabled={search.isFetching} onClick={() => setCursor(nextCursor)}>加载更多</button>}
        {currentTarget && <div className="session-search-footer"><button className="icon-button" type="button" title="上一个结果" onClick={() => move(-1)}><ArrowUp size={15} /></button><button className="icon-button" type="button" title="下一个结果" onClick={() => move(1)}><ArrowDown size={15} /></button>{feedback && <span role="status">{feedback}</span>}</div>}
      </section>
    </div>}
  </>;
}

export function locateTimelineTarget(positionKey: string, root: ParentNode = document, behavior?: ScrollBehavior): boolean {
  const candidates = Array.from(root.querySelectorAll<HTMLElement>("[data-timeline-id], [data-timeline-aliases]"));
  const element = candidates.find(candidate => candidate.dataset.timelineId === positionKey)
    ?? candidates.filter(candidate => matchesTimelinePosition(positionKey, candidate.dataset.timelineId, candidate.dataset.timelineAliases?.split(/\s+/)))
      .reduce<HTMLElement | undefined>((best, candidate) => !best || best.contains(candidate) ? candidate : best, undefined);
  if (!element) return false;
  element.scrollIntoView({ behavior: behavior ?? (window.matchMedia("(prefers-reduced-motion: reduce)").matches ? "auto" : "smooth"), block: "center" });
  element.classList.remove("timeline-target-highlight");
  void element.offsetWidth;
  element.classList.add("timeline-target-highlight");
  window.setTimeout(() => element.classList.remove("timeline-target-highlight"), 1800);
  return true;
}
