import { UserMessage, UserMessageScope } from "../common/UserMessage";
import {
  ProviderSessionRowBody,
  WorkspaceEmptyState,
  WorkspaceInlineRename,
  WorkspaceListHeader,
  WorkspaceSearchInput,
  WorkspaceSplitShell,
  WorkspaceStageHeader,
} from "../common/WorkspaceSplitShell";
import { ToolOutput, MarkdownPathScope } from "../common/FilePathLink";
import { ActivityDetails, DisclosureRevealContext, DisclosureScope } from "../common/ActivityDetails";
import { visibleMarkdown } from "../../lib/domain/visibleMarkdown";
import { Copy, FolderOpen, Pencil, Terminal, Trash2 } from "lucide-react";
import { useCallback, useRef, useMemo, useState, type ReactNode } from "react";
import { useHistoryScroll } from "../../lib/useHistoryScroll";
import { HistoryLoading } from "../common/HistoryLoading";
import { machineScope, useConnection } from "../../lib/query/connection";
import { useClaudeActions, useClaudeDetail, useClaudeSessions } from "../../lib/query/claude";
import type { ClaudeDeletePreview, ClaudeHistoryEvent, ClaudeSessionSummary, SessionSearchResult } from "../../types";
import { ConfirmDialog } from "../common/ConfirmDialog";
import { MarkdownContent } from "../common/MarkdownContent";
import { PlanActions } from "../common/PlanActions";
import { CopyReplyButton } from "../common/CopyReplyButton";
import { ExecutionGroupView } from "../common/ExecutionGroupView";
import { ProcessGroupView } from "../common/ProcessGroupView";
import { TaskMenu, ThreadMenuItems, type ThreadMenuAction } from "../common/TaskMenu";
import { canRevealLocalPath, revealLocalPath } from "../../lib/api/filePaths";
import { claudeResumeCommand } from "../../lib/domain/claudeSession";
import { useSessionSelection } from "../../lib/query/sessions";
import { SessionBatchControls, buildBatchContextMenuActions } from "../common/SessionBatchControls";
import { RenameableSession } from "../common/RenameableSession";
import { groupClaudeProcesses, type ClaudeRenderItem } from "../../lib/domain/claudeProcessGroups";
import { foldProcessSequence, thinkingStepFromText } from "../../lib/domain/processGroups";
import { ClaudeProcessView } from "./ClaudeProcessView";
import { TimelineRail } from "../common/TimelineRail";
import { claudeTimelinePage, useTimelineWindow } from "../../lib/useTimelineWindow";
import { getClaudeSession } from "../../lib/api/claude";
import { TimelineHistoryStatus } from "../common/TimelineHistoryStatus";
import { userTimelineEntries } from "../../lib/domain/timelineViewModel";
import { useSearchWorkspace } from "../common/SessionSearch";
import { useCopyFeedback } from "../../hooks/useCopyFeedback";
import {
  useNarrowViewport,
  useProviderSearchLocator,
  useTimelineNavigator,
} from "../../lib/useProviderWorkspace";

export function ClaudeWorkspace() {
  const connection = useConnection();
  const menuTrigger = useRef<HTMLElement>(null);
  const stream = useRef<HTMLDivElement>(null);
  const [query, setQuery] = useState("");
  const [selectedKey, setSelectedKey] = useState<string | null>(null);
  const narrow = useNarrowViewport();
  const [renaming, setRenaming] = useState(false);
  const [title, setTitle] = useState("");
  const [preview, setPreview] = useState<ClaudeDeletePreview | null>(null);
  const { feedback, setFeedback, copy } = useCopyFeedback();
  const [pendingSearch, setPendingSearch] = useState<SessionSearchResult | null>(null);
  const sessions = useClaudeSessions(query);
  const listed = sessions.data?.find((item) => item.sessionKey === selectedKey) ?? sessions.data?.[0];
  const sessionKeys = useMemo(() => (sessions.data ?? []).map(s => s.sessionKey), [sessions.data]);
  const batch = useSessionSelection(
    "claude_code",
    query,
    sessionKeys,
    keys => {
      if (selectedKey && keys.includes(selectedKey)) setSelectedKey(null);
      setRenaming(false);
      setPreview(null);
    },
    listed?.sessionKey ?? null,
  );
  const batchMenu = useMemo(() => buildBatchContextMenuActions(batch, ["delete"]), [batch]);
  const detailVisible = !narrow || Boolean(selectedKey);
  const detail = useClaudeDetail(detailVisible ? listed?.sessionKey : undefined);
  const selected = detail.data?.pages[0]?.summary ?? listed;
  const loadedEvents = useMemo(() => {
    const latest = new Map<string, ClaudeHistoryEvent>();
    for (const page of [...(detail.data?.pages ?? [])].reverse()) for (const event of page.events) latest.set(event.id, event);
    return [...latest.values()];
  }, [detail.data]);
  const navigation = useTimelineWindow({
    scope: `${machineScope()}:claude:${selected?.sessionKey ?? ""}`,
    streamRef: stream,
    history: detail.data?.pages[0]?.history,
    latestId: loadedEvents[loadedEvents.length - 1]?.id,
    id: (event: ClaudeHistoryEvent) => event.id ?? "",
    read: async cursor => claudeTimelinePage(await getClaudeSession(selected!.sessionKey, cursor)),
    onNavigate: () => { history.follow.current = false; setPendingSearch(null); },
    onLocated: follow => { history.follow.current = Boolean(follow); history.capture(); }
  });
  const events = navigation.items ?? loadedEvents;
  const groupedEvents = useMemo(() => groupClaudeProcesses(events), [events]);
  const latestReplyText = useMemo(() => {
    for (let index = events.length - 1; index >= 0; index -= 1) {
      const candidate = events[index];
      if (!candidate.kind.startsWith("assistant_message")) continue;
      const cleaned = visibleMarkdown(candidate.text ?? "").trim();
      if (cleaned) return cleaned;
    }
    return "";
  }, [events]);
  const resolvedTimeline = detail.data?.pages[0]?.timeline;
  const timelineEntries = useMemo(() => resolvedTimeline ?? userTimelineEntries("claude_code", groupedEvents.flatMap(entry => entry.kind === "item"
    ? [{ id: entry.key, kind: entry.item.kind, text: entry.item.text, userMessage: entry.item.userMessage }] : [])), [resolvedTimeline, groupedEvents]);
  const actions = useClaudeActions();
  const error = actions.rename.error ?? actions.preview.error ?? actions.remove.error ?? (detail.isFetchNextPageError ? null : detail.error) ?? sessions.error;
  const selectSearchResult = useCallback((result: SessionSearchResult) => {
    setSelectedKey(result.sessionKey);
    setPendingSearch(result);
    setRenaming(false);
    setPreview(null);
    setFeedback("");
  }, []);
  useSearchWorkspace(useMemo(() => ({ provider: "claude_code" as const, sessionKey: selected?.sessionKey ?? null, selectResult: selectSearchResult }), [selected?.sessionKey, selectSearchResult]));

  const oldestPage = detail.data?.pages[detail.data.pages.length - 1];
  const history = useHistoryScroll({
    scopeKey: `${machineScope()}:claude:${selected?.sessionKey ?? ""}:${navigation.generation}`,
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
      return { cursor: page.beforeCursor, hasMore: page.hasMore };
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
    selectedSessionKey: selected?.sessionKey,
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

  const menuActions = (item: ClaudeSessionSummary): ThreadMenuAction[] => [
    { id: "copy-id", label: "复制线程 ID", icon: <Copy size={15} />, run: () => { void copy(item.id); } },
    { id: "copy-path", label: "复制文件路径", icon: <Copy size={15} />, run: () => { void copy(item.path); } },
    { id: "copy-resume", label: "复制恢复命令", icon: <Terminal size={15} />, run: () => { void copy(claudeResumeCommand(item.cwd, item.id)); } },
    { id: "reveal", label: "在 Finder 中显示", icon: <FolderOpen size={15} />, disabled: !canRevealLocalPath(),
      reason: canRevealLocalPath() ? null : "仅可在本机 Finder 中显示本机会话", run: () => { void revealLocalPath(item.path).catch(() => setFeedback("无法在 Finder 中显示，请检查文件是否仍存在")); } },
    { id: "delete", label: "删除任务文件", icon: <Trash2 size={15} />, danger: true, disabled: actions.preview.isPending,
      run: () => { actions.remove.reset(); actions.preview.mutate(item.sessionKey, { onSuccess: setPreview }); } },
  ];

  return (
    <>
      <WorkspaceSplitShell
        hasSelection={Boolean(selectedKey)}
        sidebar={
          <>
            <WorkspaceListHeader
              agent="claude"
              title="Claude Code"
              count={!sessions.isLoading && sessions.data ? sessions.data.length : null}
              onRefresh={() => {
                void sessions.refetch();
                if (selected && detailVisible) void detail.refetch();
              }}
            />
            <WorkspaceSearchInput
              value={query}
              onChange={setQuery}
              placeholder="搜索标题、ID 或工作目录"
            />
            <div className="provider-list-scroll" onKeyDown={batch.onListKeyDown}>
              {sessions.error && (
                <div className="form-error" role="alert">
                  {sessions.error.message}
                </div>
              )}
              {sessions.data?.map(item => (
                <div key={item.sessionKey} className="selectable-session">
                  <RenameableSession
                    title={claudeSessionLabel(item)}
                    className={`provider-session ${item.sessionKey === selected?.sessionKey ? "selected" : ""}`}
                    disabled={batch.busy}
                    selecting={batch.selecting}
                    selected={item.sessionKey === selected?.sessionKey}
                    multiSelected={batch.selection.includes(item.sessionKey)}
                    renameBlockReason={!item.canRename ? item.renameBlockReason : undefined}
                    onSelect={event =>
                      batch.select(item.sessionKey, event, () => {
                        setSelectedKey(item.sessionKey);
                        setRenaming(false);
                        setPreview(null);
                        setFeedback("");
                      })
                    }
                    onRename={next =>
                      actions.rename.mutateAsync({ sessionKey: item.sessionKey, title: next })
                    }
                    batchMenuActions={batchMenu}
                    menuActions={menuActions(item)}
                  >
                    <ProviderSessionRowBody
                      title={claudeSessionLabel(item)}
                      idText={item.id}
                      cwd={item.cwd}
                      running={item.status === "running"}
                      statusText={claudeStatusLabel(item)}
                      storageSize={item.storageSize}
                    />
                  </RenameableSession>
                </div>
              ))}
              {sessions.isLoading && <div className="muted-row">正在读取任务...</div>}
              {!sessions.isLoading && !sessions.data?.length && (
                <div className="muted-row">未发现 Claude Code 会话</div>
              )}
            </div>
            <SessionBatchControls batch={batch} operations={["delete"]} />
          </>
        }
      >
        {!selected && (
          <WorkspaceEmptyState
            agent="claude"
            title="选择一个 Claude Code 任务"
            subtitle="支持多选清理、重命名与完整思维链及工具调用过程检视"
          />
        )}
        {selected && (
          <>
            <WorkspaceStageHeader
              title={claudeSessionLabel(selected)}
              subtitle={selected.cwd}
              onBack={() => setSelectedKey(null)}
              badges={
                selected.status === "running" ? (
                  <span className="status-chip tone-info">运行中</span>
                ) : selected.readError ? (
                  <span className="status-chip tone-danger">异常</span>
                ) : null
              }
              actions={
                <TaskMenu label="Claude Code 任务操作" triggerRef={menuTrigger}>
                  <button
                    disabled={!selected.canRename}
                    title={selected.renameBlockReason ?? undefined}
                    onClick={() => {
                      setTitle(selected.title);
                      setRenaming(true);
                      actions.rename.reset();
                    }}
                  >
                    <Pencil size={15} />
                    改名
                  </button>
                  <button
                    disabled={!latestReplyText}
                    title={!latestReplyText ? "当前会话暂无可复制的助手答复" : undefined}
                    onClick={() => {
                      void copy(latestReplyText);
                    }}
                  >
                    <Copy size={15} />
                    复制答复
                  </button>
                  <ThreadMenuItems actions={menuActions(selected)} />
                </TaskMenu>
              }
            />
            {feedback && (
              <div role="status" className="task-feedback">
                {feedback}
              </div>
            )}
            {renaming && (
              <WorkspaceInlineRename
                ariaLabel="Claude Code 任务名称"
                value={title}
                onChange={setTitle}
                busy={actions.rename.isPending}
                onSubmit={() =>
                  actions.rename.mutate(
                    { sessionKey: selected.sessionKey, title },
                    { onSuccess: () => setRenaming(false) },
                  )
                }
                onCancel={() => setRenaming(false)}
              />
            )}
            {selected.readError && <div className="form-error" role="alert">{selected.readError}</div>}
            {selected.readWarning && <DisclosureScope.Provider value={`claude:${selected.sessionKey}`}><ActivityDetails className="task-feedback" summary="读取提示" initiallyOpen={false} stateKey="read-warning">{() => <div>{selected.readWarning}</div>}</ActivityDetails></DisclosureScope.Provider>}
            {!selected.readError && selected.renameBlockReason && <div className="task-feedback" role="status">{selected.renameBlockReason}{connection.target === "local" && <button disabled={actions.diagnostics.isPending} onClick={() => actions.diagnostics.mutate(selected.sessionKey)}>查看占用详情</button>}</div>}
            {connection.target === "local" && actions.diagnostics.variables === selected.sessionKey && <>
              {actions.diagnostics.error && <div className="form-error" role="alert">{actions.diagnostics.error.message}</div>}
              {actions.diagnostics.data && <ActivityDetails className="task-feedback" summary="原生进程归属" initiallyOpen stateKey={`ownership:${selected.sessionKey}`}>
                <p>{actions.diagnostics.data.ownership === "inactive" ? "未发现关联的活跃会话进程" : actions.diagnostics.data.ownership === "open" ? "存在活跃会话或文件占用" : "归属尚未确认"}</p>
                {actions.diagnostics.data.processes.map((process, index) => <p key={`${process.pid}:${index}`}>PID {process.pid} · {({ session: "会话", daemon: "后台服务", pty_host: "终端宿主", unknown: "未核验进程" } as Record<string, string>)[process.role] ?? process.role} · {process.registrationVerified ? "登记已核验" : "登记未核验"}{process.mismatchedFields.length > 0 && ` · 不匹配字段：${process.mismatchedFields.join("、")}`}</p>)}
              </ActivityDetails>}
            </>}
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
                <UserMessageScope.Provider value={{ provider: "claude_code", sessionKey: selected.sessionKey }}>
                  <MarkdownPathScope.Provider value={selected.cwd}>
                    <DisclosureScope.Provider value={`claude:${selected.sessionKey}`}>
                      <DisclosureRevealContext.Provider value={pendingSearch?.sessionKey === selected.sessionKey ? pendingSearch.positionKey : null}>
                        {renderClaudeEvents(groupedEvents, selected.title)}
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
        <ConfirmDialog labelledBy="claude-delete-title" busy={actions.remove.isPending} onCancel={() => setPreview(null)} returnFocus={menuTrigger}>
          <h2 id="claude-delete-title">删除任务文件</h2>
          <strong>{preview.title}</strong>
          {(preview.paths ?? [preview.path]).map(path => <code className="delete-path" key={path}>{path}</code>)}
          <p>{preview.fileCount} 个会话及专属文件，{preview.bytes.toLocaleString()} 字节。</p>
          {actions.remove.error && <div role="alert" className="form-error">{actions.remove.error.message}</div>}
          {preview.warnings?.map(warning => <p className="form-error" key={warning}>{warning}</p>)}
          {actions.remove.data?.residuals?.map(path => <p className="form-error" role="alert" key={path}>{path}</p>)}
          <div className="button-row">
            <button className="secondary-button" disabled={actions.remove.isPending} onClick={() => setPreview(null)}>取消</button>
            <button className="danger-button" disabled={actions.remove.isPending} onClick={() => actions.remove.mutate({ sessionKey: preview.sessionKey, confirmed: true }, { onSuccess: result => { if (!result.deleted) return; setPreview(null); setSelectedKey(current => current === preview.sessionKey ? null : current); } })}>
              <Trash2 size={16} />{actions.remove.isPending ? "正在删除..." : "确认删除任务文件"}
            </button>
          </div>
        </ConfirmDialog>
      )}
    </>
  );
}

function ClaudeEvent({
  event,
  activityId,
  title,
  timelineAliases = [],
  insideProcess = false,
}: {
  event: ClaudeHistoryEvent;
  activityId: string;
  title: string;
  timelineAliases?: string[];
  insideProcess?: boolean;
}) {
  const timelineProps = { "data-timeline-id": activityId, "data-timeline-aliases": timelineAliases.length ? timelineAliases.join(" ") : undefined };
  if (event.kind === "user_message") {
    const aliases = [...timelineAliases, ...(event.userMessage?.id ? [event.userMessage.id] : [])];
    return <UserMessage message={event.userMessage} text={event.text ?? ""} activityId={activityId} timelineId={activityId} timelineAliases={aliases} />;
  }
  if (event.kind === "plan") {
    return (
      <article className="provider-event plan" {...timelineProps}>
        <header className="plan-heading">
          <strong>计划</strong>
          <PlanActions markdown={event.text ?? ""} fallbackTitle={title} />
        </header>
        <MarkdownContent text={event.text ?? ""} activityId={activityId} foldInstructions={false} />
      </article>
    );
  }
  if (event.detail) {
    return (
      <ActivityDetails timelineId={activityId} timelineAliases={timelineAliases} className="grok-tool" stateKey={activityId} initiallyOpen={false} summary={<span className="tool-title">{event.text ?? "活动记录"}</span>}>
        {() => <ToolOutput text={event.detail!} />}
      </ActivityDetails>
    );
  }
  if (!visibleMarkdown(event.text ?? "").trim()) return null;
  if (insideProcess && event.kind === "thinking") {
    return (
      <div className="process-thought-step" {...timelineProps}>
        <MarkdownContent text={event.text ?? ""} activityId={activityId} timelineAliases={timelineAliases} foldInstructions={false} />
      </div>
    );
  }
  return (
    <article className={`provider-event ${event.kind}${event.kind.startsWith("assistant_message") ? " assistant-reply" : ""}`} {...timelineProps}>
      {!event.kind.startsWith("assistant_message") && <div className="chat-meta">{claudeEventLabel(event)}</div>}
      <MarkdownContent text={event.text ?? ""} activityId={activityId} timelineAliases={timelineAliases} foldInstructions={event.kind !== "compaction" && event.kind !== "branch_summary"} />
      {event.kind.startsWith("assistant_message") && <CopyReplyButton text={event.text ?? ""} />}
    </article>
  );
}

function isFoldableClaudeCore(entry: ClaudeRenderItem): boolean {
  if (entry.kind === "group") return true;
  if (entry.kind !== "item") return false;
  if (entry.item.kind === "thinking") return true;
  if (entry.item.kind === "assistant_message" && entry.item.assistantPhase === "intermediate") return true;
  return Boolean(
    entry.item.detail &&
      entry.item.kind !== "user_message" &&
      entry.item.kind !== "plan" &&
      !entry.item.kind.startsWith("assistant_message"),
  );
}

function claudeEntryTurnId(entry: ClaudeRenderItem): string | null | undefined {
  if (entry.kind === "process") return entry.process.turnId;
  if (entry.kind === "group") return entry.group.turnId;
  if (entry.kind === "item") return entry.item.turnId;
  const first = entry.group.entries[0];
  return first?.kind === "item" ? first.item.turnId : first?.group.turnId;
}

function isNonCompactionMetadata(
  entry: ClaudeRenderItem | undefined,
): entry is Extract<ClaudeRenderItem, { kind: "metadata" }> {
  return Boolean(entry && entry.kind === "metadata" && entry.group.label !== "压缩摘要");
}

function sameOrUnsetTurn(a: string | null | undefined, b: string | null | undefined): boolean {
  return !a || !b || a === b;
}

export function renderClaudeEvents(entries: ClaudeRenderItem[], title: string, insideProcess = false): ReactNode {
  if (!insideProcess) {
    const indexMap = new Map<ClaudeRenderItem, number>();
    entries.forEach((entry, idx) => indexMap.set(entry, idx));

    const folded = foldProcessSequence("claude", entries, entry => {
      if (entry.kind === "group") return { kind: "execution", group: entry.group, turnId: entry.group.turnId };
      if (
        entry.kind === "item" &&
        (entry.item.kind === "thinking" ||
          (entry.item.kind === "assistant_message" && entry.item.assistantPhase === "intermediate"))
      ) {
        return thinkingStepFromText(entry.key, entry.sourceIds, entry.item.text, false, entry.item.turnId);
      }
      if (isNonCompactionMetadata(entry)) {
        const idx = indexMap.get(entry) ?? -1;
        const turnId = claudeEntryTurnId(entry);
        let left = idx - 1;
        while (left >= 0 && isNonCompactionMetadata(entries[left]) && sameOrUnsetTurn(turnId, claudeEntryTurnId(entries[left]))) {
          left -= 1;
        }
        let right = idx + 1;
        while (right < entries.length && isNonCompactionMetadata(entries[right]) && sameOrUnsetTurn(turnId, claudeEntryTurnId(entries[right]))) {
          right += 1;
        }
        const attachedLeft = left >= 0 && isFoldableClaudeCore(entries[left]) && sameOrUnsetTurn(turnId, claudeEntryTurnId(entries[left]));
        const attachedRight = right < entries.length && isFoldableClaudeCore(entries[right]) && sameOrUnsetTurn(turnId, claudeEntryTurnId(entries[right]));
        if (attachedLeft || attachedRight) {
          const detailText = entry.group.entries
            .map(sub => (sub.kind === "item" ? sub.item.detail || sub.item.text || "" : ""))
            .filter(Boolean)
            .join("\n\n");
          return {
            kind: "detail",
            id: entry.group.id,
            sourceIds: entry.group.sourceIds,
            title: entry.group.label,
            detail: detailText || entry.group.label,
            turnId,
          };
        }
      }
      if (
        entry.kind === "item" &&
        entry.item.detail &&
        entry.item.kind !== "user_message" &&
        entry.item.kind !== "plan" &&
        !entry.item.kind.startsWith("assistant_message")
      ) {
        return {
          kind: "detail",
          id: entry.key,
          sourceIds: entry.sourceIds,
          title: entry.item.text ?? "活动记录",
          detail: entry.item.detail,
          failed: entry.item.kind === "failure" || /^(failed|error)$/i.test(entry.item.status ?? ""),
          turnId: entry.item.turnId,
        };
      }
      return null;
    });

    return folded.map(entry =>
      entry.kind === "process" ? (
        <ProcessGroupView key={entry.process.id} group={entry.process} />
      ) : entry.kind === "execution" ? (
        <ExecutionGroupView key={entry.group.id} group={entry.group} />
      ) : (
        renderSingleClaudeEntry(entry.item, title, false)
      ),
    );
  }

  return entries.map(entry => renderSingleClaudeEntry(entry, title, true));
}

function renderSingleClaudeEntry(entry: ClaudeRenderItem, title: string, insideProcess: boolean): ReactNode {
  if (entry.kind === "process") {
    return (
      <ClaudeProcessView key={entry.process.id} group={entry.process}>
        {() => renderClaudeEvents(entry.process.entries, title, true)}
      </ClaudeProcessView>
    );
  }
  if (entry.kind === "metadata") {
    return (
      <ActivityDetails
        key={entry.group.id}
        className="execution-group claude-metadata-group"
        stateKey={entry.group.id}
        stateAliases={entry.group.sourceIds.map(id => `claude-metadata:${id}`)}
        timelineId={entry.group.id}
        timelineAliases={entry.group.sourceIds}
        initiallyOpen={false}
        summary={<span className="tool-title">{entry.group.label}</span>}
      >
        {() => renderClaudeEvents(entry.group.entries, title, insideProcess)}
      </ActivityDetails>
    );
  }
  if (entry.kind === "group") {
    return <ExecutionGroupView key={entry.group.id} group={entry.group} />;
  }
  return (
    <ClaudeEvent
      key={entry.key}
      activityId={entry.key}
      event={entry.item}
      title={title}
      timelineAliases={entry.sourceIds}
      insideProcess={insideProcess}
    />
  );
}

function claudeEventLabel(event: ClaudeHistoryEvent): string {
  if (event.kind === "user_message") return "你";
  if (event.kind === "assistant_message") return "Claude Code";
  if (event.kind === "thinking") return "思考";
  if (event.kind === "failure") return "失败";
  if (event.kind === "compaction") return "压缩摘要";
  if (event.kind === "branch_summary") return "分支摘要";
  return event.role || "Claude Code";
}

function claudeStatusLabel(session: ClaudeSessionSummary): string {
  if (session.status === "running") return "运行中";
  if (session.status === "unknown") return "状态未知";
  return session.updatedAt ? new Date(session.updatedAt).toLocaleString() : "最近";
}

export function claudeSessionLabel(session: ClaudeSessionSummary): string { return session.title || session.id; }
