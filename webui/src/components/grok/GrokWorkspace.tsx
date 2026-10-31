import {
  ProviderSessionRowBody,
  WorkspaceEmptyState,
  WorkspaceInlineRename,
  WorkspaceListHeader,
  WorkspaceSearchInput,
  WorkspaceSplitShell,
  WorkspaceStageHeader,
} from "../common/WorkspaceSplitShell";
import { UserMessage, UserMessageScope } from "../common/UserMessage";
import { MarkdownPathScope } from "../common/FilePathLink";
import { DisclosureRevealContext, DisclosureScope } from "../common/ActivityDetails";
import { visibleMarkdown } from "../../lib/domain/visibleMarkdown";
import { Copy, FolderOpen, Pencil, Terminal, Trash2 } from "lucide-react";
import { canRevealLocalPath, revealLocalPath } from "../../lib/api/filePaths";
import { useCallback, useMemo, useRef, useState, type ReactNode } from "react";
import { useGrokActions, useGrokDetail, useGrokSessions } from "../../lib/query/grok";
import { useHistoryScroll } from "../../lib/useHistoryScroll";
import { HistoryLoading } from "../common/HistoryLoading";
import { machineScope } from "../../lib/query/connection";
import { MarkdownContent } from "../common/MarkdownContent";
import { CopyReplyButton } from "../common/CopyReplyButton";
import { ExecutionGroupView } from "../common/ExecutionGroupView";
import { ProcessGroupView } from "../common/ProcessGroupView";
import { PlanActions } from "../common/PlanActions";
import { TaskMenu } from "../common/TaskMenu";
import { ConfirmDialog } from "../common/ConfirmDialog";
import type { GrokDeletePreview, GrokHistoryEvent, GrokSessionSummary, SessionSearchResult } from "../../types";
import { useSessionSelection } from "../../lib/query/sessions";
import { SessionBatchControls, buildBatchContextMenuActions } from "../common/SessionBatchControls";
import { RenameableSession } from "../common/RenameableSession";
import { groupGrokCommandEvents } from "../../lib/domain/executionGroups";
import type { ExecutionRenderItem } from "../../lib/domain/executionGroups";
import { foldProcessSequence, thinkingStepFromText } from "../../lib/domain/processGroups";
import { TimelineRail } from "../common/TimelineRail";
import { grokTimelinePage, useTimelineWindow } from "../../lib/useTimelineWindow";
import { getGrokSession } from "../../lib/api/grok";
import { TimelineHistoryStatus } from "../common/TimelineHistoryStatus";
import { userTimelineEntries } from "../../lib/domain/timelineViewModel";
import { useSearchWorkspace } from "../common/SessionSearch";
import { useCopyFeedback } from "../../hooks/useCopyFeedback";
import {
  useNarrowViewport,
  useProviderSearchLocator,
  useTimelineNavigator,
} from "../../lib/useProviderWorkspace";

export function grokHandoffPath(session: Pick<GrokSessionSummary, "handoffPath" | "path">): string {
  return session.handoffPath?.trim() || session.path;
}

export function grokResumeCommand(cwd: string, id: string): string {
  const quote = (value: string) => `'${value.replaceAll("'", "'\\''")}'`;
  return `${cwd ? `cd -- ${quote(cwd)} && ` : ""}grok --resume ${quote(id)}`;
}

export function GrokWorkspace({}: {}) {
  const menuTrigger = useRef<HTMLElement>(null);
  const stream = useRef<HTMLDivElement>(null);
  const [query, setQuery] = useState("");
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const narrow = useNarrowViewport();
  const [renaming, setRenaming] = useState(false);
  const [title, setTitle] = useState("");
  const [preview, setPreview] = useState<GrokDeletePreview | null>(null);
  const { feedback, setFeedback, copy: copyText } = useCopyFeedback();
  const [pendingSearch, setPendingSearch] = useState<SessionSearchResult | null>(null);
  const sessions = useGrokSessions(query);
  const selected = sessions.data?.find((item) => item.id === selectedId) ?? sessions.data?.[0];
  const sessionIds = useMemo(() => (sessions.data ?? []).map(s => s.id), [sessions.data]);
  const batch = useSessionSelection(
    "grok",
    query,
    sessionIds,
    (keys) => {
      if (selectedId && keys.includes(selectedId)) setSelectedId(null);
      setPreview(null);
      setRenaming(false);
    },
    selected?.id ?? null,
  );
  const batchMenu = useMemo(() => buildBatchContextMenuActions(batch, ["delete"]), [batch]);
  const detailVisible = !narrow || Boolean(selectedId);
  const detail = useGrokDetail(detailVisible ? selected?.id : undefined);
  const loadedEvents = useMemo(() => {
    const rows = new Map<string, GrokHistoryEvent>();
    for (const [pageIndex, page] of [...(detail.data?.pages ?? [])].reverse().entries()) {
      for (const [index, event] of page.events.entries()) rows.set(event.id ?? `${pageIndex}:${index}`, event);
    }
    return [...rows.values()];
  }, [detail.data]);
  const navigation = useTimelineWindow({
    scope: `${machineScope()}:grok:${selected?.id ?? ""}`,
    streamRef: stream,
    history: detail.data?.pages[0]?.history,
    latestId: loadedEvents[loadedEvents.length - 1]?.id,
    id: (event: GrokHistoryEvent) => event.id ?? "",
    read: async cursor => grokTimelinePage(await getGrokSession(selected!.id, { limit: 120, before: cursor })),
    onNavigate: () => { history.follow.current = false; setPendingSearch(null); },
    onLocated: follow => { history.follow.current = Boolean(follow); history.capture(); }
  });
  const events = navigation.items ?? loadedEvents;
  const groupedEvents = useMemo(() => groupGrokCommandEvents(events), [events]);
  const latestReplyText = useMemo(() => {
    for (let index = events.length - 1; index >= 0; index -= 1) {
      const candidate = events[index];
      if (!candidate.kind.startsWith("agent_message") && !candidate.kind.startsWith("assistant_message")) {
        continue;
      }
      const cleaned = visibleMarkdown(candidate.text ?? "").trim();
      if (cleaned) return cleaned;
    }
    return "";
  }, [events]);
  const resolvedTimeline = detail.data?.pages[0]?.timeline;
  const timelineEntries = useMemo(() => resolvedTimeline ?? userTimelineEntries("grok", groupedEvents.flatMap(entry => entry.kind === "item"
    ? [{ id: entry.key, kind: entry.item.kind, text: entry.item.text, userMessage: entry.item.userMessage }] : [])), [resolvedTimeline, groupedEvents]);
  const actions = useGrokActions();
  const error = actions.rename.error ?? actions.preview.error ?? actions.remove.error ?? (detail.isFetchNextPageError ? null : detail.error) ?? sessions.error;
  const selectSearchResult = useCallback((result: SessionSearchResult) => {
    setSelectedId(result.sessionKey);
    setPendingSearch(result);
    setRenaming(false);
    setPreview(null);
    setFeedback("");
  }, []);
  useSearchWorkspace(useMemo(() => ({ provider: "grok" as const, sessionKey: selected?.id ?? null, selectResult: selectSearchResult }), [selected?.id, selectSearchResult]));
  const oldestPage = detail.data?.pages[detail.data.pages.length - 1];
  const history = useHistoryScroll({
    scopeKey: `${machineScope()}:grok:${selected?.id ?? ""}:${navigation.generation}`,
    streamRef: stream,
    cursor: navigation.active ? navigation.before : oldestPage?.beforeCursor,
    hasMore: navigation.active ? navigation.hasMore : Boolean(detail.hasNextPage),
    ready: detailVisible && Boolean(detail.data),
    initialPosition: navigation.active ? { top: 0, follow: false } : undefined,
    busy: navigation.busy || (!navigation.active && detail.isFetching),
    searchActive: Boolean(pendingSearch),
    loadPage: async (_cursor, isCurrent) => {
      if (navigation.active) return navigation.loadOlder(_cursor, isCurrent);
      const result = await detail.fetchNextPage({ cancelRefetch: false });
      if (result.isError) throw result.error;
      const page = result.data?.pages[result.data.pages.length - 1];
      if (!page) throw new Error("历史读取失败");
      return { cursor: page.beforeCursor, hasMore: Boolean(page.hasMore) };
    }
  });
  const handleTimelineSelect = useTimelineNavigator({
    streamRef: stream,
    followRef: history.follow,
    capture: history.capture,
    hasResolvedTimeline: Boolean(resolvedTimeline),
    selectWindowTarget: navigation.select,
  });

  useProviderSearchLocator({
    pendingSearch,
    selectedSessionKey: selected?.id,
    isLoading: detail.isLoading,
    isFetching: detail.isFetching,
    hasNextPage: Boolean(detail.hasNextPage),
    navigationActive: navigation.active,
    navigationLatest: navigation.latest,
    historyLoading: history.loading,
    historyError: history.error,
    historyLoadOlder: history.loadOlder,
    followRef: history.follow,
    capture: history.capture,
    streamRef: stream,
    setFeedback,
    setPendingSearch,
    events,
  });

  return (
    <>
      <WorkspaceSplitShell
        hasSelection={Boolean(selectedId)}
        sidebar={
          <>
            <WorkspaceListHeader
              agent="grok"
              title="Grok Build"
              count={!sessions.isLoading && sessions.data ? sessions.data.length : null}
              onRefresh={() => {
                void sessions.refetch();
                if (selected && detailVisible) void detail.refetch();
              }}
            />
            <WorkspaceSearchInput value={query} onChange={setQuery} placeholder="搜索任务" />
            <div className="provider-list-scroll" onKeyDown={batch.onListKeyDown}>
              {sessions.error && <div className="form-error" role="alert">{sessions.error.message}</div>}
              {sessions.data?.map((item) => (
                <div key={item.id} className="selectable-session">
                  <RenameableSession
                    title={grokSessionLabel(item)}
                    className={`provider-session ${item.id === selected?.id ? "selected" : ""}`}
                    disabled={batch.busy}
                    selecting={batch.selecting}
                    selected={item.id === selected?.id}
                    multiSelected={batch.selection.includes(item.id)}
                    onSelect={event => batch.select(item.id, event, () => { setSelectedId(item.id); setRenaming(false); setPreview(null); setFeedback(""); })}
                    onRename={(next) => actions.rename.mutateAsync({ id: item.id, title: next })}
                    batchMenuActions={batchMenu}
                    menuActions={[
                      { id: "copy-id", label: "复制线程 ID", icon: <Copy size={15} />, run: () => { void copyText(item.id, "已复制线程 ID"); } },
                      { id: "copy-path", label: "复制路径", icon: <Copy size={15} />, run: () => { void copyText(grokHandoffPath(item), "已复制路径"); } },
                      { id: "copy-resume", label: "复制恢复命令", icon: <Terminal size={15} />, run: () => { void copyText(grokResumeCommand(item.cwd, item.id), "已复制恢复命令"); } },
                      { id: "reveal", label: "在 Finder 中显示", icon: <FolderOpen size={15} />, disabled: !canRevealLocalPath(), reason: canRevealLocalPath() ? null : "仅可在本机 Finder 中显示本机会话", run: () => { void revealLocalPath(grokHandoffPath(item)).catch(() => setFeedback("无法在 Finder 中显示，请检查文件是否仍存在")); } },
                      { id: "delete", label: "删除任务文件", icon: <Trash2 size={15} />, danger: true, disabled: actions.preview.isPending, run: () => { actions.remove.reset(); actions.preview.mutate(item.id, { onSuccess: setPreview }); } },
                    ]}
                  >
                    <ProviderSessionRowBody
                      title={grokSessionLabel(item)}
                      cwd={item.cwd}
                      running={item.status === "running"}
                      statusText={item.status === "unknown" ? "状态未知" : item.updatedAt ? new Date(item.updatedAt).toLocaleString() : "最近"}
                      storageSize={item.storageSize}
                    />
                  </RenameableSession>
                </div>
              ))}
              {sessions.isLoading && <div className="muted-row">正在读取任务...</div>}
              {!sessions.isLoading && !sessions.data?.length && <div className="muted-row">暂无 Grok 任务</div>}
            </div>
            <SessionBatchControls batch={batch} operations={["delete"]} />
          </>
        }
      >
        {!selected && (
          <WorkspaceEmptyState
            agent="grok"
            title="选择一个 Grok 任务"
            subtitle="支持浏览本地会话历史、命令输出分组与任务文件管理"
          />
        )}
        {selected && (
          <>
            <WorkspaceStageHeader
              title={grokSessionLabel(selected)}
              subtitle={selected.cwd}
              onBack={() => setSelectedId(null)}
              badges={selected.status === "running" ? <span className="status-chip tone-info">运行中</span> : null}
              actions={
                <TaskMenu label="Grok 任务操作" triggerRef={menuTrigger}>
                  <button onClick={() => { setTitle(selected.title); setRenaming(true); actions.rename.reset(); }}><Pencil size={15} />改名</button>
                  <button disabled={!latestReplyText} title={!latestReplyText ? "当前会话暂无可复制的助手答复" : undefined} onClick={() => { void copyText(latestReplyText, "已复制答复"); }}><Copy size={15} />复制答复</button>
                  <button onClick={() => { void copyText(selected.id, "已复制线程 ID"); }}><Copy size={15} />复制线程 ID</button>
                  <button onClick={() => { void copyText(grokHandoffPath(selected), "已复制路径"); }}><Copy size={15} />复制路径</button>
                  <button onClick={() => { void copyText(grokResumeCommand(selected.cwd, selected.id), "已复制恢复命令"); }}><Terminal size={15} />复制恢复命令</button>
                  <button disabled={!canRevealLocalPath()} title={canRevealLocalPath() ? undefined : "仅可在本机 Finder 中显示本机会话"} onClick={() => { void revealLocalPath(grokHandoffPath(selected)).catch(() => setFeedback("无法在 Finder 中显示，请检查文件是否仍存在")); }}><FolderOpen size={15} />在 Finder 中显示</button>
                  <button className="danger-menu-item" disabled={actions.preview.isPending} onClick={() => { actions.remove.reset(); actions.preview.mutate(selected.id, { onSuccess: setPreview }); }}><Trash2 size={15} />删除任务文件</button>
                </TaskMenu>
              }
            />
            {feedback && <div role="status" className="task-feedback">{feedback}</div>}
            {renaming && (
              <WorkspaceInlineRename
                ariaLabel="Grok 任务名称"
                value={title}
                onChange={setTitle}
                busy={actions.rename.isPending}
                onSubmit={() => actions.rename.mutate({ id: selected.id, title }, { onSuccess: () => setRenaming(false) })}
                onCancel={() => setRenaming(false)}
              />
            )}
            {error && <div className="form-error" role="alert">{error.message}</div>}
            <div className="timeline-reading-shell">
              <TimelineRail
                entries={timelineEntries}
                streamRef={stream}
                onSelect={handleTimelineSelect}
                loadOlder={!resolvedTimeline && detail.hasNextPage ? () => { void history.loadOlder("search"); } : undefined}
                loading={history.loading}
              />
              <div ref={stream} className="provider-events" onScroll={history.onScroll} onScrollCapture={history.onScrollCapture} onWheel={history.onWheel} onTouchStart={history.onTouchStart} onTouchMove={history.onTouchMove} onKeyDown={history.onKeyDown} tabIndex={0}>
                <HistoryLoading loading={history.loading} error={history.error} retry={history.retry} />
                <UserMessageScope.Provider value={{ provider: "grok", sessionKey: selected.id }}>
                  <MarkdownPathScope.Provider value={selected.cwd}>
                    <DisclosureScope.Provider value={`grok:${selected.id}`}>
                      <DisclosureRevealContext.Provider value={pendingSearch?.sessionKey === selected.id ? pendingSearch.positionKey : null}>
                        {renderGrokEvents(groupedEvents, grokSessionLabel(selected), selected.status === "running")}
                      </DisclosureRevealContext.Provider>
                    </DisclosureScope.Provider>
                  </MarkdownPathScope.Provider>
                </UserMessageScope.Provider>
                <TimelineHistoryStatus navigation={{ ...navigation, busy: navigation.busy || history.loading }} />
                {detail.isLoading && <div className="muted-row">正在读取消息...</div>}
                {!detail.isLoading && !events.length && <div className="muted-row">暂无对话</div>}
              </div>
            </div>
          </>
        )}
      </WorkspaceSplitShell>
      {preview && (
        <ConfirmDialog labelledBy="grok-delete-title" busy={actions.remove.isPending} onCancel={() => setPreview(null)} returnFocus={menuTrigger}>
          <h2 id="grok-delete-title">删除任务文件</h2>
          <strong>{preview.title}</strong>
          <code className="delete-path">{preview.path}</code>
          <p>{preview.fileCount} 个文件，{preview.bytes.toLocaleString()} 字节。仅删除此 session 目录；工作目录、worktree、配置与云端任务保留。</p>
          {actions.remove.error && <div role="alert" className="form-error">{actions.remove.error.message}</div>}
          {actions.remove.data?.residuals?.map(path => <p className="form-error" role="alert" key={path}>{path}</p>)}
          <div className="button-row">
            <button className="secondary-button" disabled={actions.remove.isPending} onClick={() => setPreview(null)}>取消</button>
            <button className="danger-button" disabled={actions.remove.isPending} onClick={() => actions.remove.mutate({ id: preview.id, confirmed: true, fingerprint: preview.fingerprint }, { onSuccess: result => { if (!result.deleted) return; setPreview(null); setSelectedId(current => current === preview.id ? null : current); } })}>
              <Trash2 size={16} />{actions.remove.isPending ? "正在删除..." : "确认删除任务文件"}
            </button>
          </div>
        </ConfirmDialog>
      )}
    </>
  );
}

function renderGrokEvents(
  entries: ExecutionRenderItem<GrokHistoryEvent>[],
  sessionTitle: string,
  sessionRunning = false,
): ReactNode {
  const lastIndex = entries.length - 1;
  const folded = foldProcessSequence("grok", entries, entry => {
    if (entry.kind === "group") return { kind: "execution", group: entry.group };
    const event = entry.item;
    if (event.kind === "agent_thought_chunk" || event.kind === "thinking") {
      const isTrailing = entries.indexOf(entry) === lastIndex;
      return thinkingStepFromText(
        entry.key,
        entry.sourceIds,
        event.text,
        sessionRunning && isTrailing,
      );
    }
    return null;
  });

  return folded.map(entry =>
    entry.kind === "process" ? (
      <ProcessGroupView key={entry.process.id} group={entry.process} />
    ) : entry.kind === "execution" ? (
      <ExecutionGroupView key={entry.group.id} group={entry.group} />
    ) : entry.item.kind === "group" ? (
      <ExecutionGroupView key={entry.item.group.id} group={entry.item.group} />
    ) : (
      renderGrokEvent(entry.item.item, entry.item.key, sessionTitle, entry.item.sourceIds)
    ),
  );
}

function renderGrokEvent(event: GrokHistoryEvent, key: string, sessionTitle: string, sourceIds: string[]): ReactNode {
  const timelineProps = { "data-timeline-id": key, "data-timeline-aliases": sourceIds.join(" ") || undefined };
  if (event.kind === "user_message_chunk") {
    const aliases = [...sourceIds, ...(event.userMessage?.id ? [event.userMessage.id] : [])];
    return <UserMessage key={event.userMessage?.id ?? key} message={event.userMessage} text={event.text ?? ""} activityId={key} timelineId={key} timelineAliases={aliases} />;
  }
  if (!visibleMarkdown(event.text ?? "").trim() && event.kind !== "plan") return null;
  const isAssistant = event.kind.startsWith("agent_message") || event.kind.startsWith("assistant_message");
  return (
    <article className={`provider-event ${event.kind}${isAssistant ? " assistant-reply" : ""}`} {...timelineProps} key={key}>
      {event.kind === "plan" && (
        <div className="plan-header">
          <span className="chat-meta">计划</span>
          <PlanActions markdown={event.text ?? ""} fallbackTitle={sessionTitle} />
        </div>
      )}
      <MarkdownContent text={event.text ?? ""} activityId={key} timelineAliases={sourceIds} foldInstructions={event.kind !== "plan"} />
      {isAssistant && <CopyReplyButton text={event.text ?? ""} />}
    </article>
  );
}

export function grokSessionLabel(session: GrokSessionSummary): string { return session.title || session.id; }

export function canPreviewGrokDelete(_session: GrokSessionSummary): boolean { return true; }
