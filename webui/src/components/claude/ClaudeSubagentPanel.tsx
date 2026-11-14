import { ArrowLeft, Bot, ChevronRight, X } from "lucide-react";
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { getClaudeSession } from "../../lib/api/claude";
import { groupClaudeProcesses } from "../../lib/domain/claudeProcessGroups";
import { visibleMarkdown } from "../../lib/domain/visibleMarkdown";
import { groupedTimelineEntries } from "../../lib/useProviderWorkspace";
import { useClaudeSubagentDetail } from "../../lib/query/claude";
import { machineScope } from "../../lib/query/connection";
import { useHistoryScroll } from "../../lib/useHistoryScroll";
import { claudeTimelinePage, useTimelineWindow } from "../../lib/useTimelineWindow";
import type { ClaudeHistoryEvent, SubagentActivity, SubagentCollection } from "../../types";
import { DisclosureScope } from "../common/ActivityDetails";
import { HistoryLoading } from "../common/HistoryLoading";
import { TimelineHistoryStatus } from "../common/TimelineHistoryStatus";
import { TimelineRail } from "../common/TimelineRail";
import { UserMessageScope } from "../common/UserMessage";
import { renderClaudeEvents } from "./ClaudeHistory";

type Position = { top: number; follow: boolean; windowId?: string };
export function ClaudeSubagentPanel({ parentKey, collection, onClose, docked }: {
  parentKey: string; collection?: SubagentCollection; onClose: () => void; docked: boolean;
}) {
  const [selected, setSelected] = useState<SubagentActivity | null>(null);
  const panel = useRef<HTMLElement>(null);
  const close = useRef<HTMLButtonElement>(null);
  const list = useRef<HTMLDivElement>(null);
  const listTop = useRef(0);
  const returnTo = useRef<string | null>(null);
  const positions = useRef(new Map<string, Position>());
  const agent = collection?.agents.find(item => item.agentId === selected?.agentId) ?? selected;
  useLayoutEffect(() => {
    if (!selected && list.current) {
      list.current.scrollTop = listTop.current;
      const button = Array.from(list.current.querySelectorAll<HTMLButtonElement>("[data-agent-id]"))
        .find(item => item.dataset.agentId === returnTo.current);
      if (button) { button.focus({ preventScroll: true }); return; }
    }
    close.current?.focus({ preventScroll: true });
  }, [selected]);
  useEffect(() => {
    const keydown = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !document.querySelector("dialog[open]")) { event.preventDefault(); onClose(); }
      if (docked || event.key !== "Tab" || !panel.current) return;
      const controls = Array.from(panel.current.querySelectorAll<HTMLElement>('button:not(:disabled), [href], summary, [tabindex="0"]'))
        .filter(element => element.getClientRects().length > 0);
      const first = controls[0], last = controls[controls.length - 1];
      if (event.shiftKey && (document.activeElement === first || !panel.current.contains(document.activeElement))) { event.preventDefault(); last?.focus(); }
      else if (!event.shiftKey && (document.activeElement === last || !panel.current.contains(document.activeElement))) { event.preventDefault(); first?.focus(); }
    };
    document.addEventListener("keydown", keydown);
    return () => document.removeEventListener("keydown", keydown);
  }, [docked, onClose]);
  return <>
    {!docked && <button type="button" className="subagent-backdrop" aria-label="关闭子智能体详情" tabIndex={-1} onClick={onClose} />}
    <aside ref={panel} className={`subagent-panel claude-subagent-panel ${docked ? "docked" : "overlay"}`} role="dialog" aria-modal={!docked} aria-label="子智能体详情">
      <header className="subagent-panel-header">
        {selected && <button type="button" className="icon-button" aria-label="返回子智能体列表" onClick={() => setSelected(null)}><ArrowLeft size={17} /></button>}
        <div><strong title={selected ? visibleMarkdown(agent?.name ?? selected.name) : "本会话子智能体"}>{selected ? visibleMarkdown(agent?.name ?? selected.name) : "本会话子智能体"}</strong>
          {selected && <span>状态未知</span>}</div>
        <button ref={close} type="button" className="icon-button" aria-label="关闭子智能体详情" onClick={onClose}><X size={18} /></button>
      </header>
      {selected?.agentId ? <ClaudeSubagentHistory key={selected.agentId} parentKey={parentKey} agent={agent ?? selected}
        initialPosition={positions.current.get(selected.agentId)} onPosition={position => positions.current.set(selected.agentId!, position)} /> :
        <div ref={list} className="message-stream subagent-stream claude-subagent-list">
          {collection?.warning && <div role="alert" className="form-error">{collection.warning}</div>}
          {collection?.agents.map(item => <button key={item.agentId} type="button" className="subagent-list-item" data-agent-id={item.agentId}
            disabled={!item.available || !item.agentId} title={item.unavailableReason ?? undefined} onClick={() => {
              listTop.current = list.current?.scrollTop ?? 0;
              returnTo.current = item.agentId ?? null;
              setSelected(item);
            }}><Bot size={16} /><span className="tool-title">{visibleMarkdown(item.name)}</span><span className="subagent-state">{item.available ? "状态未知" : "不可用"}</span><ChevronRight size={15} /></button>)}
          {!collection?.agents.length && <div className="muted-row">暂无子智能体记录</div>}
        </div>}
    </aside>
  </>;
}

export function ClaudeSubagentHistory({ parentKey, agent, initialPosition, onPosition }: {
  parentKey: string; agent: SubagentActivity; initialPosition?: Position; onPosition: (position: Position) => void;
}) {
  const query = useClaudeSubagentDetail(parentKey, agent.agentId ?? undefined);
  const latest = query.data?.pages[0];
  const stream = useRef<HTMLDivElement>(null);
  const scope = `${machineScope()}:claude-subagent:${parentKey}:${agent.agentId}`;
  const loaded = useMemo(() => {
    const events = new Map<string, ClaudeHistoryEvent>();
    for (const page of [...(query.data?.pages ?? [])].reverse()) for (const event of page.events) events.set(event.id, event);
    return [...events.values()];
  }, [query.data]);
  const navigation = useTimelineWindow({
    scope, streamRef: stream, history: latest?.history, latestId: latest?.events[latest.events.length - 1]?.id,
    id: (event: ClaudeHistoryEvent) => event.id,
    read: async cursor => {
      const page = await getClaudeSession(parentKey, cursor, agent.agentId!);
      if (page.summary.sessionKey !== parentKey || page.agent?.agentId !== agent.agentId) throw new Error("子智能体记录已变化，请重新打开");
      return claudeTimelinePage(page);
    },
    onNavigate: () => { history.follow.current = false; },
    onLocated: follow => { history.follow.current = Boolean(follow); history.capture(); },
  });
  const oldest = query.data?.pages[query.data.pages.length - 1];
  const history = useHistoryScroll({
    scopeKey: `${scope}:${navigation.generation}`, streamRef: stream,
    cursor: navigation.active ? navigation.before : oldest?.beforeCursor,
    hasMore: navigation.active ? navigation.hasMore : Boolean(query.hasNextPage),
    ready: Boolean(latest), busy: navigation.busy || (!navigation.active && query.isFetching),
    initialPosition: navigation.active ? { top: 0, follow: false } : initialPosition,
    loadPage: async (cursor, isCurrent) => {
      if (navigation.active) return navigation.loadOlder(cursor, isCurrent);
      const result = await query.fetchNextPage({ cancelRefetch: false });
      if (result.isError) throw result.error;
      const page = result.data?.pages[result.data.pages.length - 1];
      if (!page) throw new Error("历史读取失败");
      return { cursor: page.beforeCursor, hasMore: page.hasMore };
    },
  });
  const events = navigation.items ?? loaded;
  const grouped = useMemo(() => groupClaudeProcesses(events, true), [events]);
  const entries = useMemo(() => groupedTimelineEntries("claude_code", latest?.timeline, grouped), [latest?.timeline, grouped]);
  const restore = useRef(initialPosition?.windowId);
  useEffect(() => {
    if (restore.current && latest) {
      const id = restore.current;
      restore.current = undefined;
      navigation.select(id);
    }
  }, [latest, navigation.select]);
  const windowActive = useRef(navigation.active); windowActive.current = navigation.active;
  const save = useRef(onPosition); save.current = onPosition;
  useEffect(() => {
    const element = stream.current;
    return () => {
      if (!element) return;
      const top = element.getBoundingClientRect().top;
      const anchor = windowActive.current ? Array.from(element.querySelectorAll<HTMLElement>("[data-timeline-id]"))
        .find(item => item.getBoundingClientRect().bottom > top) : undefined;
      save.current({ top: element.scrollTop, follow: history.follow.current, windowId: anchor?.dataset.timelineId });
    };
  }, [history.follow]);
  return <div className="timeline-reading-shell">
    <TimelineRail entries={entries} streamRef={stream} onSelect={navigation.select} loading={history.loading} />
    <div ref={stream} className="message-stream subagent-stream" tabIndex={0} onScroll={history.onScroll}
      onScrollCapture={history.onScrollCapture} onWheel={history.onWheel} onTouchStart={history.onTouchStart} onTouchMove={history.onTouchMove} onKeyDown={history.onKeyDown}>
      <HistoryLoading loading={history.loading} error={history.error} retry={history.retry} />
      {query.error && !query.isFetchNextPageError && <div role="alert" className="form-error">{query.error.message}<button type="button" onClick={() => void query.refetch()}>重试</button></div>}
      {query.isLoading && <div className="muted-row">正在读取消息...</div>}
      <UserMessageScope.Provider value={{ provider: "claude_code", sessionKey: parentKey, attachments: "names" }}>
        <DisclosureScope.Provider value={scope}>{renderClaudeEvents(grouped, latest?.agent?.name ?? agent.name)}</DisclosureScope.Provider>
      </UserMessageScope.Provider>
      <TimelineHistoryStatus navigation={{ ...navigation, busy: navigation.busy || history.loading }} />
      {latest && !events.length && <div className="muted-row">暂无子智能体消息</div>}
    </div>
  </div>;
}
