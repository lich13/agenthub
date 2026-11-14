import { UserMessageScope } from "../common/UserMessage";
import { MarkdownPathScope } from "../common/FilePathLink";
import { DisclosureRevealContext, DisclosureScope } from "../common/ActivityDetails";
import { Archive, ArchiveRestore, Check, ChevronLeft, Copy, FolderOpen, Pencil, Trash2, X } from "lucide-react";
import { canRevealLocalPath, revealLocalPath } from "../../lib/api/filePaths";
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { MessageBlockView } from "./MessageStream";
import { ExecutionGroupView } from "../common/ExecutionGroupView";
import { ProcessGroupView } from "../common/ProcessGroupView";
import { useSessionSelection } from "../../lib/query/sessions";
import { SessionBatchControls } from "../common/SessionBatchControls";
import { TaskMenu } from "../common/TaskMenu";
import { useCopyFeedback } from "../../hooks/useCopyFeedback";
import { useReadOnlyThreadActions, type ThreadMessageSlot, type ThreadMessageStoreController } from "../../lib/query/threads";
import { threadStatusLabel, type SelectedThread, type View } from "../../lib/domain/codexViewModel";
import { sharedDisabledStates } from "../../lib/domain/visualContract";
import {
  isHistoryCollapsedBlock,
  isPlanBlock,
  isQuestionBlock,
  isQuestionResultBlock,
  isRunningToolBlock,
  isToolBlock,
  latestAssistantCopyText,
  threadResumeCommand,
  toolBlockDetailText,
  toolBlockTitle,
  visibleConversationBlocksForHistory,
} from "../../lib/domain/conversationViewModel";
import type { RuntimeCapabilityMatrix } from "../../lib/query/system";
import type { SessionSearchResult, SubagentActivity, ThreadDetail, ThreadSummary } from "../../types";
import { groupCodexCommandBlocks } from "../../lib/domain/executionGroups";
import { foldProcessSequence } from "../../lib/domain/processGroups";
import { TimelineRail } from "../common/TimelineRail";
import { userTimelineEntries } from "../../lib/domain/timelineViewModel";
import { locateTimelineTarget } from "../common/SessionSearch";
import { SubagentPanel } from "./SubagentPanel";
import { SubagentSummary } from "./SubagentSummary";
import { getThread, getThreadBlocks } from "../../lib/api/threads";
import { useHistoryScroll } from "../../lib/useHistoryScroll";
import { HistoryLoading } from "../common/HistoryLoading";
import { machineScope, useConnection } from "../../lib/query/connection";
import { codexTimelinePage, useTimelineWindow } from "../../lib/useTimelineWindow";
import { TimelineHistoryStatus } from "../common/TimelineHistoryStatus";
import { DesktopComposer } from "./DesktopComposer";
import type { MessageBlock } from "../../types";

function computeVisibleItems(visibleBlocks: MessageBlock[]) {
  const commandGrouped = groupCodexCommandBlocks(visibleBlocks);
  return foldProcessSequence("codex", commandGrouped, entry => {
    if (entry.kind === "group") return { kind: "execution", group: entry.group, turnId: entry.group.turnId };
    const block = entry.item;
    if (
      block.subagent ||
      isHistoryCollapsedBlock(block) ||
      isPlanBlock(block) ||
      isQuestionBlock(block) ||
      isQuestionResultBlock(block)
    ) {
      return null;
    }
    if (isToolBlock(block)) {
      const failed = /^(failed|error)$/i.test(block.status ?? "");
      return {
        kind: "detail",
        id: block.id,
        sourceIds: [block.id, ...(block.call_id ? [block.call_id] : [])],
        title: toolBlockTitle(block),
        detail: toolBlockDetailText(block),
        running: isRunningToolBlock(block),
        failed,
        turnId: block.turn_id,
      };
    }
    return null;
  });
}

type DerivedBlocksEntry = {
  visibleBlocks: MessageBlock[];
  visibleItems: ReturnType<typeof computeVisibleItems>;
};

const derivedBlocksCache = new WeakMap<
  MessageBlock[],
  {
    compacted?: DerivedBlocksEntry;
    expanded?: DerivedBlocksEntry;
    timeline?: ReturnType<typeof userTimelineEntries>;
  }
>();

function getDerivedConversationBlocks(
  blocks: MessageBlock[],
  historyExpanded: boolean,
  revealedBlockId: string | null,
): DerivedBlocksEntry {
  if (!revealedBlockId) {
    const slotKey = historyExpanded ? "expanded" : "compacted";
    let cached = derivedBlocksCache.get(blocks);
    if (cached?.[slotKey]) return cached[slotKey]!;
    const visibleBlocks = visibleConversationBlocksForHistory(blocks, historyExpanded, undefined, undefined, null);
    const visibleItems = computeVisibleItems(visibleBlocks);
    const entry: DerivedBlocksEntry = { visibleBlocks, visibleItems };
    if (!cached) {
      cached = {};
      derivedBlocksCache.set(blocks, cached);
    }
    cached[slotKey] = entry;
    return entry;
  }
  const visibleBlocks = visibleConversationBlocksForHistory(blocks, historyExpanded, undefined, undefined, revealedBlockId);
  const visibleItems = computeVisibleItems(visibleBlocks);
  return { visibleBlocks, visibleItems };
}

function getDerivedTimelineEntries(blocks: MessageBlock[]) {
  let cached = derivedBlocksCache.get(blocks);
  if (cached?.timeline) return cached.timeline;
  const timeline = userTimelineEntries(
    "codex",
    blocks.map(block => ({ id: block.id, role: block.role, text: block.text, userMessage: block.user_message })),
  );
  if (!cached) {
    cached = {};
    derivedBlocksCache.set(blocks, cached);
  }
  cached.timeline = timeline;
  return timeline;
}

export function Conversation(props: {
  threadId: string;
  detail: ThreadDetail;
  selectedSummary?: ThreadSummary | null;
  archivedView?: boolean;
  slot: ThreadMessageSlot;
  messageStore: ThreadMessageStoreController;
  onSelect: (id: SelectedThread) => void;
  onPanelSelect: (view: View) => void;
  nextThreadAfterArchive: string | null;
  capabilities: RuntimeCapabilityMatrix;
  onBack?: () => void;
  searchTarget?: SessionSearchResult | null;
  searchReady?: boolean;
  onSearchResolved?: () => void;
  onSearchMiss?: () => void;
}) {
  const { detail, slot } = props;
  const connection = useConnection();
  const summary = props.selectedSummary?.id === detail.summary.id
    ? { ...detail.summary, ...props.selectedSummary }
    : detail.summary;
  const [renaming, setRenaming] = useState(false);
  const [title, setTitle] = useState("");
  const { feedback, setFeedback, copy, clear: clearFeedback } = useCopyFeedback();
  const [historyExpanded, setHistoryExpanded] = useState(false);
  const [revealedHistory, setRevealedHistory] = useState<{ scope: string; positionKey: string } | null>(null);
  const stream = useRef<HTMLDivElement>(null);
  const shell = useRef<HTMLDivElement>(null);
  const [agentSelection, setAgentSelection] = useState<{ root: string; machine: string; agent?: SubagentActivity; trigger: HTMLButtonElement } | null>(null);
  const [docked, setDocked] = useState(false);
  const machine = machineScope();
  const historyScope = `${machine}:codex:${props.threadId}`;
  const searchPosition = props.searchTarget?.sessionKey === props.threadId ? props.searchTarget.positionKey : null;
  const revealedBlockId = searchPosition ?? (revealedHistory?.scope === historyScope ? revealedHistory.positionKey : null);
  const selectedAgent = agentSelection?.root === props.threadId && agentSelection.machine === machine ? agentSelection : null;
  useEffect(() => { setAgentSelection(null); }, [props.threadId, machine]);
  const viewportAnchor = useRef<{ element: HTMLElement; offset: number } | null>(null);
  const captureViewport = useCallback(() => {
    const element = stream.current;
    if (!element) return;
    const top = element.getBoundingClientRect().top;
    const anchor = Array.from(element.querySelectorAll<HTMLElement>("[data-timeline-id]"))
      .find(item => item.getBoundingClientRect().bottom > top);
    viewportAnchor.current = anchor ? { element: anchor, offset: anchor.getBoundingClientRect().top - top } : null;
  }, []);
  const openAgent = useCallback((agent: SubagentActivity, trigger: HTMLButtonElement) => {
    captureViewport();
    setAgentSelection({ root: props.threadId, machine: machineScope(), agent, trigger });
  }, [captureViewport, props.threadId]);
  const closeAgent = () => {
    captureViewport();
    const trigger = selectedAgent?.trigger;
    setAgentSelection(null);
    requestAnimationFrame(() => { if (trigger?.isConnected) trigger.focus({ preventScroll: true }); });
  };
  const navigation = useTimelineWindow({
    scope: historyScope, streamRef: stream, history: detail.history, latestId: detail.blocks[detail.blocks.length - 1]?.id,
    id: (block: import("../../types").MessageBlock) => block.id,
    read: async cursor => codexTimelinePage(await getThread(props.threadId, { limit: 120, before: cursor })),
    onNavigate: () => { history.follow.current = false; setHistoryExpanded(true); props.onSearchResolved?.(); },
    onLocated: follow => { history.follow.current = Boolean(follow); history.capture(); }
  });
  const blocks = navigation.items ?? (slot.blocks.length ? slot.blocks : detail.blocks);
  const { visibleBlocks, visibleItems } = useMemo(
    () => getDerivedConversationBlocks(blocks, historyExpanded, revealedBlockId),
    [blocks, historyExpanded, revealedBlockId],
  );
  const resolvedTimeline = slot.timeline ?? detail.timeline;
  const timelineEntries = useMemo(() => resolvedTimeline ?? getDerivedTimelineEntries(blocks), [resolvedTimeline, blocks]);
  const deletion = useSessionSelection("codex", props.threadId, [props.threadId], (keys) => {
    if (keys.includes(props.threadId)) props.onSelect(null);
  });
  const actions = useReadOnlyThreadActions({ onSuccess: () => setRenaming(false) });
  const history = useHistoryScroll({
    scopeKey: `${historyScope}:${navigation.generation}`, streamRef: stream,
    cursor: navigation.active ? navigation.before : slot.beforeCursor, hasMore: navigation.active ? navigation.hasMore : slot.hasMoreBlocks, ready: detail.summary.id === props.threadId,
    searchActive: Boolean(props.searchTarget), busy: navigation.busy,
    initialPosition: navigation.active ? { top: 0, follow: false } : undefined,
    loadPage: async (cursor, isCurrent) => {
      if (navigation.active) return navigation.loadOlder(cursor, isCurrent);
      const page = await getThreadBlocks(props.threadId, { limit: 120, before: cursor });
      if (isCurrent()) props.messageStore.applyBlockPage(props.threadId, page, cursor);
      return { cursor: page.before_cursor, hasMore: page.has_more_blocks };
    }
  });
  const handleTimelineSelect = useCallback((id: string) => {
    history.follow.current = false;
    if (blocks.some(block => block.id === id)) {
      setHistoryExpanded(true);
      setRevealedHistory({ scope: historyScope, positionKey: id });
      requestAnimationFrame(() => {
        if (stream.current) locateTimelineTarget(id, stream.current, "auto");
        history.capture();
      });
      return;
    }
    if (resolvedTimeline) {
      navigation.select(id);
      return;
    }
    setHistoryExpanded(true);
    setRevealedHistory({ scope: historyScope, positionKey: id });
    requestAnimationFrame(() => {
      if (stream.current) locateTimelineTarget(id, stream.current, "auto");
      history.capture();
    });
  }, [blocks, history, historyScope, navigation, resolvedTimeline]);
  const showHistory = useCallback(() => {
    history.follow.current = false;
    history.capture();
    setHistoryExpanded(true);
  }, [history.follow, history.capture]);
  useEffect(() => {
    setHistoryExpanded(false);
    setRenaming(false);
    clearFeedback();
  }, [props.threadId, machine, clearFeedback]);
  useLayoutEffect(() => {
    const anchor = viewportAnchor.current;
    const element = stream.current;
    if (element && anchor?.element.isConnected) {
      element.scrollTop += anchor.element.getBoundingClientRect().top - element.getBoundingClientRect().top - anchor.offset;
    }
    viewportAnchor.current = null;
    history.capture();
  }, [selectedAgent, docked, history.capture]);
  useEffect(() => {
    if (!shell.current || typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(entries => {
      captureViewport();
      setDocked(entries[0].contentRect.width >= 960);
    });
    observer.observe(shell.current);
    return () => observer.disconnect();
  }, [captureViewport]);
  useEffect(() => {
    if (searchPosition) {
      setRevealedHistory({ scope: historyScope, positionKey: searchPosition });
      clearFeedback();
    }
  }, [searchPosition, historyScope, clearFeedback]);
  useEffect(() => {
    const target = props.searchTarget;
    if (!target || target.sessionKey !== props.threadId || !props.searchReady || history.loading) return;
    if (navigation.active) { navigation.latest(); return; }
    history.follow.current = false;
    if (locateTimelineTarget(target.positionKey, stream.current?.closest(".conversation-main") ?? undefined, "auto")) {
      history.capture();
      clearFeedback();
      props.onSearchResolved?.();
      return;
    }
    if (slot.hasMoreBlocks && slot.beforeCursor) {
      if (history.error) return;
      void history.loadOlder("search");
      return;
    }
    setFeedback("结果所在历史未加载，请刷新后重试");
    props.onSearchMiss?.();
  }, [props.searchTarget, props.threadId, props.searchReady, navigation.active, navigation.latest, props.onSearchResolved, props.onSearchMiss, slot.hasMoreBlocks, slot.beforeCursor, history.loading, history.error, history.loadOlder, history.capture, history.follow, blocks.length, clearFeedback, setFeedback]);
  const archived = Boolean(props.archivedView) || summary.status === "Archived";
  return <div ref={shell} className="conversation-shell compact-readonly-conversation">
    <main className="conversation-main">
      <header className="conversation-header" data-timeline-id="session">
        <button className="icon-button mobile-back" title="返回任务列表" onClick={props.onBack}><ChevronLeft size={18} /></button>
        <div className="conversation-title-copy">
          <h1 className="conversation-title">{summary.title}</h1>
        </div>
        <div className="conversation-header-actions">
          <span className={`status-chip ${summary.status}`}>{threadStatusLabel(summary.status)}</span>
          <TaskMenu triggerRef={deletion.returnFocus}>
            <button disabled={archived || actions.isPending} title={archived ? sharedDisabledStates.renameArchivedThread : undefined} onClick={() => { setTitle(summary.title); setRenaming(true); }}><Pencil size={15} />改名</button>
            <button disabled={actions.isPending} onClick={() => actions.mutate({ kind: archived ? "restore" : "archive", id: props.threadId })}>{archived ? <ArchiveRestore size={15} /> : <Archive size={15} />}{archived ? "取消归档" : "归档"}</button>
            <button onClick={() => copy(latestAssistantCopyText(blocks))}><Copy size={15} />复制答复</button>
            <button onClick={() => copy(detail.summary.id)}><Copy size={15} />复制 ID</button>
            <button onClick={() => copy(detail.summary.rollout_path)}><Copy size={15} />复制路径</button>
            <button onClick={() => copy(threadResumeCommand(detail.summary.id))}><Copy size={15} />复制恢复命令</button>
            <button disabled={!canRevealLocalPath() || !detail.summary.rollout_path} title={!canRevealLocalPath() ? "仅可在本机 Finder 中显示本机会话" : !detail.summary.rollout_path ? "当前会话暂无本地文件路径" : undefined} onClick={() => { if (!detail.summary.rollout_path) return; void revealLocalPath(detail.summary.rollout_path).catch(() => setFeedback("无法在 Finder 中显示，请检查文件是否仍存在")); }}><FolderOpen size={15} />在 Finder 中显示</button>
            <button className="danger-menu-item" disabled={actions.isPending || deletion.busy} onClick={() => deletion.prepareSingle(props.threadId, "delete", deletion.returnFocus.current)}><Trash2 size={15} />永久删除</button>
          </TaskMenu>
        </div>
      </header>
      {props.capabilities.threadSubagents === true && <SubagentSummary collection={slot.subagents ?? detail.subagents} onOpen={trigger => {
        captureViewport();
        setAgentSelection({ root: props.threadId, machine: machineScope(), trigger });
      }} />}
      <SessionBatchControls batch={deletion} operations={[]} showSelection={false} />
      {renaming && !archived && <form className="inline-rename" onSubmit={(event) => { event.preventDefault(); actions.mutate({ kind: "rename", id: props.threadId, title }); }}>
        <input aria-label="任务名称" maxLength={200} value={title} onChange={(event) => setTitle(event.target.value)} autoFocus />
        <button className="icon-button" title="保存名称" disabled={actions.isPending || !title.trim()}><Check size={17} /></button>
        <button type="button" className="icon-button" title="取消改名" onClick={() => setRenaming(false)}><X size={17} /></button>
      </form>}
      {actions.error && <div role="alert" className="form-error">{actions.error.message}</div>}
      {feedback && <div role="status" className="task-feedback">{feedback}</div>}
      <div className="timeline-reading-shell">
        <TimelineRail
          entries={timelineEntries}
          streamRef={stream}
          onSelect={handleTimelineSelect}
          loadOlder={!resolvedTimeline && slot.hasMoreBlocks ? () => { void history.loadOlder("search"); } : undefined}
          loading={history.loading}
        />
        <div ref={stream} className="message-stream readonly-message-stream" onScroll={history.onScroll} onScrollCapture={history.onScrollCapture} onWheel={history.onWheel} onTouchStart={history.onTouchStart} onTouchMove={history.onTouchMove} onKeyDown={history.onKeyDown} tabIndex={0}>
          <HistoryLoading loading={history.loading} error={history.error} retry={history.retry} />
          <UserMessageScope.Provider value={{ provider: "codex", sessionKey: summary.id }}>
            <MarkdownPathScope.Provider value={summary.cwd}>
              <DisclosureScope.Provider value={`codex:${props.threadId}`}>
                <DisclosureRevealContext.Provider value={searchPosition}>
                  {visibleItems.map((entry) =>
                    entry.kind === "process" ? (
                      <ProcessGroupView key={entry.process.id} group={entry.process} />
                    ) : entry.kind === "execution" ? (
                      <ExecutionGroupView key={entry.group.id} group={entry.group} />
                    ) : entry.item.kind === "group" ? (
                      <ExecutionGroupView key={entry.item.group.id} group={entry.item.group} />
                    ) : (
                      <MessageBlockView
                        key={entry.item.item.id}
                        block={entry.item.item}
                        onOpenSubagent={openAgent}
                        subagentsSupported={props.capabilities.threadSubagents === true}
                        planFallbackTitle={summary.title}
                        historyExpanded={historyExpanded}
                        onShowHistory={showHistory}
                      />
                    )
                  )}
                </DisclosureRevealContext.Provider>
              </DisclosureScope.Provider>
            </MarkdownPathScope.Provider>
          </UserMessageScope.Provider>
          <TimelineHistoryStatus navigation={{ ...navigation, busy: navigation.busy || history.loading }} />
          {!blocks.length && <div className="muted-row">暂无消息</div>}
        </div>
      </div>
      {!archived && props.capabilities.codexDesktop === true && connection.target === "local" && <DesktopComposer key={`${connection.revision}:${props.threadId}`} threadId={props.threadId} />}
    </main>
    {selectedAgent && <SubagentPanel key={`${selectedAgent.machine}:${props.threadId}:${selectedAgent.agent?.agentId ?? "list"}`} rootThreadId={props.threadId} initialAgent={selectedAgent.agent} rootSubagents={slot.subagents ?? detail.subagents} docked={docked} onClose={closeAgent} />}
  </div>;
}
