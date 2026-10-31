import {
  Archive,
  ArchiveRestore,
  ChevronRight,
  Copy,
  FolderOpen,
  Pencil,
  Trash2,
} from "lucide-react";
import { canRevealLocalPath, revealLocalPath } from "../../lib/api/filePaths";
import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import {
  ProviderSessionRowBody,
  WorkspaceEmptyState,
  WorkspaceInlineRename,
  WorkspaceListHeader,
  WorkspaceSearchInput,
  WorkspaceSplitShell,
  WorkspaceStageHeader,
} from "../common/WorkspaceSplitShell";
import { groupProviderEvents, type ExecutionRenderItem } from "../../lib/domain/executionGroups";
import { foldProcessSequence, thinkingStepFromText } from "../../lib/domain/processGroups";
import { userTimelineEntries } from "../../lib/domain/timelineViewModel";
import { visibleMarkdown } from "../../lib/domain/visibleMarkdown";
import { machineScope } from "../../lib/query/connection";
import {
  readArchivableProviderSession,
  useProviderActions,
  useProviderDetail,
  useProviderSessions,
  type ArchivableProviderId,
} from "../../lib/query/providerSessions";
import { useSessionSelection, type SessionSelection } from "../../lib/query/sessions";
import type { RuntimeCapabilityMatrix } from "../../lib/query/system";
import { useHistoryScroll } from "../../lib/useHistoryScroll";
import { providerTimelinePage, useTimelineWindow } from "../../lib/useTimelineWindow";
import type {
  ProviderHistoryEvent,
  ProviderSessionSummary,
  SessionSearchResult,
} from "../../types";
import {
  ActivityDetails,
  DisclosureRevealContext,
  DisclosureScope,
} from "../common/ActivityDetails";
import { CopyReplyButton } from "../common/CopyReplyButton";
import { ExecutionGroupView } from "../common/ExecutionGroupView";
import { MarkdownPathScope, ToolOutput } from "../common/FilePathLink";
import { HistoryLoading } from "../common/HistoryLoading";
import { MarkdownContent } from "../common/MarkdownContent";
import { PlanActions } from "../common/PlanActions";
import { ProcessGroupView } from "../common/ProcessGroupView";
import { RenameableSession } from "../common/RenameableSession";
import {
  SessionBatchControls,
  buildBatchContextMenuActions,
} from "../common/SessionBatchControls";
import { useSearchWorkspace } from "../common/SessionSearch";
import { useCopyFeedback } from "../../hooks/useCopyFeedback";
import {
  useNarrowViewport,
  useProviderSearchLocator,
  useTimelineNavigator,
} from "../../lib/useProviderWorkspace";
import { TaskMenu, ThreadMenuItems, type ThreadMenuAction } from "../common/TaskMenu";
import { TimelineHistoryStatus } from "../common/TimelineHistoryStatus";
import { TimelineRail } from "../common/TimelineRail";
import { UserMessage, UserMessageScope } from "../common/UserMessage";

export function AntigravityWorkspace({
  capabilities,
}: {
  capabilities: RuntimeCapabilityMatrix;
}) {
  return <ArchivableProviderWorkspace provider="antigravity" capabilities={capabilities} />;
}

export function DshWorkspace({
  capabilities,
}: {
  capabilities: RuntimeCapabilityMatrix;
}) {
  return <ArchivableProviderWorkspace provider="dsh" capabilities={capabilities} />;
}

export function ArchivableProviderWorkspace({
  provider,
  capabilities,
}: {
  provider: ArchivableProviderId;
  capabilities: RuntimeCapabilityMatrix;
}) {
  const label = provider === "antigravity" ? "Antigravity" : "DeepSeek Harness";
  const supported =
    provider === "antigravity"
      ? capabilities.antigravitySessions
      : capabilities.dshSessions;
  const queryEnabled = supported === true;

  const menuTrigger = useRef<HTMLElement>(null);
  const stream = useRef<HTMLDivElement>(null);
  const [query, setQuery] = useState("");
  const [archivedOpen, setArchivedOpen] = useState(false);
  const [selectedKey, setSelectedKey] = useState<string | null>(null);
  const narrow = useNarrowViewport();
  const [renaming, setRenaming] = useState(false);
  const [title, setTitle] = useState("");
  const { feedback, setFeedback, copy } = useCopyFeedback();
  const [pendingSearch, setPendingSearch] = useState<SessionSearchResult | null>(null);

  const sessions = useProviderSessions(provider, { q: query, archived: false }, queryEnabled);
  const archivedSessions = useProviderSessions(
    provider,
    { q: query, archived: true },
    queryEnabled && archivedOpen,
  );

  const activeKeys = useMemo(
    () => (sessions.data ?? []).map(item => item.sessionKey),
    [sessions.data],
  );
  const archivedKeys = useMemo(
    () => (archivedSessions.data ?? []).map(item => item.sessionKey),
    [archivedSessions.data],
  );

  const allListed = useMemo(
    () => [...(sessions.data ?? []), ...(archivedOpen ? (archivedSessions.data ?? []) : [])],
    [archivedOpen, archivedSessions.data, sessions.data],
  );
  const listed =
    allListed.find(item => item.sessionKey === selectedKey) ?? sessions.data?.[0];

  const activeSelectedKey =
    listed?.sessionKey && activeKeys.includes(listed.sessionKey) ? listed.sessionKey : null;
  const archivedSelectedKey =
    listed?.sessionKey && archivedKeys.includes(listed.sessionKey) ? listed.sessionKey : null;

  const handleBatchSucceeded = useCallback(
    (keys: string[]) => {
      if (selectedKey && keys.includes(selectedKey)) setSelectedKey(null);
      setRenaming(false);
    },
    [selectedKey],
  );

  const batch = useSessionSelection(
    provider,
    `active:${query}`,
    activeKeys,
    handleBatchSucceeded,
    activeSelectedKey,
  );
  const archivedBatch = useSessionSelection(
    provider,
    `archived:${query}`,
    archivedKeys,
    handleBatchSucceeded,
    archivedSelectedKey,
  );

  const detailTargetKey =
    pendingSearch && selectedKey ? selectedKey : listed?.sessionKey ?? null;
  const detailVisible = !narrow || Boolean(selectedKey);
  const detail = useProviderDetail(
    provider,
    detailVisible ? detailTargetKey : null,
    queryEnabled,
  );
  const selected = detail.data?.summary ?? listed;

  const loadedEvents = useMemo(() => {
    const latest = new Map<string, ProviderHistoryEvent>();
    for (const page of [...(detail.data ? [detail.data] : [])]) {
      for (const event of page.events) latest.set(event.id, event);
    }
    return [...latest.values()];
  }, [detail.data]);

  const navigation = useTimelineWindow({
    scope: `${machineScope()}:${provider}:${selected?.sessionKey ?? ""}`,
    streamRef: stream,
    history: detail.data?.history,
    latestId: loadedEvents[loadedEvents.length - 1]?.id,
    id: (event: ProviderHistoryEvent) => event.id ?? "",
    read: async cursor =>
      providerTimelinePage(
        await readArchivableProviderSession(provider, {
          sessionKey: selected!.sessionKey,
          limit: 160,
          before: cursor,
        }),
      ),
    onNavigate: () => {
      history.follow.current = false;
      setPendingSearch(null);
    },
    onLocated: follow => {
      history.follow.current = Boolean(follow);
      history.capture();
    },
  });

  const events = navigation.items ?? loadedEvents;
  const groupedEvents = useMemo(() => groupProviderEvents(events, label), [events, label]);
  const timelineEntries = useMemo(
    () =>
      detail.data?.timeline ??
      userTimelineEntries(
        provider,
        groupedEvents.flatMap(entry =>
          entry.kind === "item"
            ? [
                {
                  id: entry.key,
                  kind: entry.item.kind,
                  text: entry.item.text,
                  userMessage: entry.item.userMessage,
                },
              ]
            : [],
        ),
      ),
    [detail.data?.timeline, groupedEvents, provider],
  );

  const latestReplyText = useMemo(() => {
    for (let index = events.length - 1; index >= 0; index -= 1) {
      const candidate = events[index];
      if (!candidate.kind.startsWith("assistant_message")) continue;
      const cleaned = visibleMarkdown(candidate.text ?? "").trim();
      if (cleaned) return cleaned;
    }
    return "";
  }, [events]);

  const actions = useProviderActions(provider);
  const error =
    actions.rename.error ??
    actions.archive.error ??
    actions.restore.error ??
    (detail.isFetchNextPageError ? null : detail.error) ??
    sessions.error;

  const selectSearchResult = useCallback((result: SessionSearchResult) => {
    setSelectedKey(result.sessionKey);
    setPendingSearch(result);
    setRenaming(false);
    setFeedback("");
  }, []);

  useSearchWorkspace(
    useMemo(
      () => ({
        provider,
        sessionKey: selected?.sessionKey ?? null,
        selectResult: selectSearchResult,
      }),
      [provider, selected?.sessionKey, selectSearchResult],
    ),
  );

  useEffect(() => {
    if (
      pendingSearch &&
      detail.data?.summary.sessionKey === pendingSearch.sessionKey &&
      detail.data.summary.archived
    ) {
      setArchivedOpen(true);
    }
  }, [detail.data?.summary.archived, detail.data?.summary.sessionKey, pendingSearch]);

  const history = useHistoryScroll({
    scopeKey: `${machineScope()}:${provider}:${selected?.sessionKey ?? ""}:${navigation.generation}`,
    streamRef: stream,
    cursor: navigation.active ? navigation.before : detail.data?.beforeCursor,
    hasMore: navigation.active ? navigation.hasMore : Boolean(detail.hasNextPage),
    ready: detailVisible && Boolean(detail.data),
    initialPosition: navigation.active ? { top: 0, follow: false } : undefined,
    busy: navigation.busy || (!navigation.active && detail.isFetching),
    searchActive: Boolean(pendingSearch),
    loadPage: async (cursor, isCurrent) => {
      if (navigation.active) return navigation.loadOlder(cursor, isCurrent);
      const result = await detail.fetchNextPage({ cancelRefetch: false });
      if (result.isError) throw result.error;
      const pages = result.data?.pages;
      const page = pages?.[pages.length - 1];
      if (!page) throw new Error("历史读取失败");
      return { cursor: page.beforeCursor, hasMore: page.hasMore };
    },
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

  const handleArchiveToggle = (item: ProviderSessionSummary) => {
    actions.archive.reset();
    actions.restore.reset();
    const mutation = item.archived ? actions.restore : actions.archive;
    mutation.mutate(
      { sessionKey: item.sessionKey },
      {
        onSuccess: () => {
          setFeedback(item.archived ? "已恢复线程" : "已归档线程");
          if (!item.archived && selectedKey === item.sessionKey && !archivedOpen) {
            const list = sessions.data ?? [];
            const index = list.findIndex(entry => entry.sessionKey === item.sessionKey);
            const next =
              (index >= 0 ? list[index + 1] ?? list[index - 1] : undefined)?.sessionKey ?? null;
            setSelectedKey(next);
            setRenaming(false);
          }
        },
      },
    );
  };

  const rowMenuActions = (
    item: ProviderSessionSummary,
    targetBatch: SessionSelection,
  ): ThreadMenuAction[] => [
    {
      id: item.archived ? "restore" : "archive",
      label: item.archived ? "恢复" : "归档",
      icon: item.archived ? <ArchiveRestore size={15} /> : <Archive size={15} />,
      disabled:
        !item.canArchive ||
        targetBatch.busy ||
        actions.archive.isPending ||
        actions.restore.isPending,
      reason: !item.canArchive ? item.archiveBlockReason : null,
      run: () => handleArchiveToggle(item),
    },
    {
      id: "copy-id",
      label: "复制 ID",
      icon: <Copy size={15} />,
      run: () => {
        void copy(item.id, "已复制 ID");
      },
    },
    {
      id: "copy-path",
      label: "复制路径",
      icon: <Copy size={15} />,
      run: () => {
        void copy(item.path, "已复制路径");
      },
    },
    {
      id: "reveal",
      label: "在 Finder 中显示",
      icon: <FolderOpen size={15} />,
      disabled: !canRevealLocalPath() || !item.path,
      reason: !canRevealLocalPath()
        ? "仅可在本机 Finder 中显示本机会话"
        : !item.path
          ? "当前会话暂无本地文件路径"
          : null,
      run: () => {
        if (!item.path) return;
        void revealLocalPath(item.path).catch(() =>
          setFeedback("无法在 Finder 中显示，请检查文件是否仍存在"),
        );
      },
    },
    {
      id: "delete",
      label: "永久删除",
      icon: <Trash2 size={15} />,
      danger: true,
      disabled: !item.canDelete || targetBatch.busy,
      reason: !item.canDelete ? item.deleteBlockReason : null,
      run: () => {
        targetBatch.prepareSingle(item.sessionKey, "delete");
      },
    },
  ];

  const activeBatchMenu = useMemo(
    () => buildBatchContextMenuActions(batch, ["archive", "delete"]),
    [batch],
  );
  const archivedBatchMenu = useMemo(
    () => buildBatchContextMenuActions(archivedBatch, ["restore", "delete"]),
    [archivedBatch],
  );

  const handleTimelineSelect = useTimelineNavigator({
    streamRef: stream,
    followRef: history.follow,
    capture: history.capture,
    hasResolvedTimeline: Boolean(detail.data?.timeline),
    selectWindowTarget: navigation.select,
  });

  const renderSessionRow = (
    item: ProviderSessionSummary,
    targetBatch: SessionSelection,
    otherBatch: SessionSelection,
    batchMenu: ThreadMenuAction[],
  ) => {
    const isSelected = item.sessionKey === selected?.sessionKey;
    const multiSelected = targetBatch.selection.includes(item.sessionKey);
    return (
      <div key={item.sessionKey} className="selectable-session">
        <RenameableSession
          title={providerSessionLabel(item)}
          className={`provider-session ${isSelected ? "selected" : ""}`}
          disabled={targetBatch.busy}
          selecting={targetBatch.selecting}
          selected={isSelected}
          multiSelected={multiSelected}
          renameBlockReason={
            !item.canRename ? item.renameBlockReason || "当前线程不可改名" : undefined
          }
          onSelect={event => {
            if (otherBatch.selecting || otherBatch.selection.length) otherBatch.clear();
            targetBatch.select(item.sessionKey, event, () => {
              setSelectedKey(item.sessionKey);
              setRenaming(false);
              setFeedback("");
            });
          }}
          onRename={next =>
            actions.rename.mutateAsync({ sessionKey: item.sessionKey, title: next })
          }
          batchMenuActions={batchMenu}
          menuActions={rowMenuActions(item, targetBatch)}
        >
          <ProviderSessionRowBody
            title={providerSessionLabel(item)}
            idText={item.id}
            cwd={item.cwd}
            running={item.status === "running"}
            statusText={providerStatusLabel(item)}
            storageSize={item.storageSize}
          />
        </RenameableSession>
      </div>
    );
  };

  const anyBatchBusy = batch.busy || archivedBatch.busy;
  const activeDetailBatch = selected?.archived ? archivedBatch : batch;

  return (
    <WorkspaceSplitShell
      hasSelection={Boolean(selectedKey)}
      sidebar={
        <>
          <WorkspaceListHeader
            agent={provider}
            title={label}
            count={queryEnabled && !sessions.isLoading && sessions.data ? sessions.data.length : null}
            refreshDisabled={!queryEnabled}
            onRefresh={() => {
              void sessions.refetch();
              if (archivedOpen) void archivedSessions.refetch();
              if (selected && detailVisible) void detail.refetch();
            }}
          />
          <WorkspaceSearchInput
            value={query}
            disabled={supported === false}
            onChange={setQuery}
            placeholder="搜索标题、ID 或工作目录"
          />
          <div className="provider-list-scroll" onKeyDown={batch.onListKeyDown}>
            {supported === false ? (
              <div className="muted-row" role="status">
                当前连接的远程服务版本较旧，尚未支持 {label} 会话列表。请先在「设置 → 更新与维护」升级远程服务。
              </div>
            ) : (
              <>
                {sessions.error && (
                  <div className="form-error" role="alert">
                    {sessions.error.message}
                  </div>
                )}
                {sessions.data?.map(item =>
                  renderSessionRow(item, batch, archivedBatch, activeBatchMenu),
                )}
                {(supported === undefined || sessions.isLoading) && (
                  <div className="muted-row">正在读取任务...</div>
                )}
                {queryEnabled && !sessions.isLoading && !sessions.data?.length && (
                  <div className="muted-row">{`未发现 ${label} 会话`}</div>
                )}
                {queryEnabled && (
                  <div className={`archived-drawer ${archivedOpen ? "is-open" : ""}`}>
                    <button
                      type="button"
                      className="archived-toggle"
                      aria-expanded={archivedOpen}
                      onClick={() => setArchivedOpen(open => !open)}
                    >
                      <ChevronRight size={14} className="archived-chevron" aria-hidden="true" />
                      <span>已归档</span>
                      {archivedOpen && !archivedSessions.isLoading && (
                        <small aria-hidden="true">{archivedSessions.data?.length ?? 0}</small>
                      )}
                    </button>
                    {archivedOpen && (
                      <div
                        className="archived-list"
                        onKeyDown={event => {
                          event.stopPropagation();
                          archivedBatch.onListKeyDown(event);
                        }}
                      >
                        {archivedSessions.error && (
                          <div className="form-error" role="alert">
                            {archivedSessions.error.message}
                          </div>
                        )}
                        {archivedSessions.isLoading && (
                          <div className="muted-row">正在读取已归档会话...</div>
                        )}
                        {archivedSessions.data?.map(item =>
                          renderSessionRow(item, archivedBatch, batch, archivedBatchMenu),
                        )}
                        {!archivedSessions.isLoading &&
                          !archivedSessions.error &&
                          !archivedSessions.data?.length && (
                            <div className="muted-row">{`暂无已归档的 ${label} 会话`}</div>
                          )}
                      </div>
                    )}
                  </div>
                )}
              </>
            )}
          </div>
          {supported === true && (
            <>
              <SessionBatchControls batch={batch} operations={["archive", "delete"]} />
              {archivedOpen && (
                <SessionBatchControls
                  batch={archivedBatch}
                  operations={["restore", "delete"]}
                />
              )}
            </>
          )}
        </>
      }
    >
      {!selected && (
        <WorkspaceEmptyState
          agent={provider}
          title={
            supported === false
              ? `升级远程服务后即可浏览 ${label} 会话`
              : `选择一个 ${label} 任务`
          }
          subtitle="支持重命名、归档与恢复、批量清理以及消息时间线定位"
        />
      )}
      {selected && (
        <>
          <WorkspaceStageHeader
            title={providerSessionLabel(selected)}
            subtitle={selected.cwd}
            onBack={() => setSelectedKey(null)}
            badges={
              selected.archived ? (
                <span className="status-chip tone-neutral">已归档</span>
              ) : selected.status === "running" ? (
                <span className="status-chip tone-info">运行中</span>
              ) : null
            }
            actions={
              <TaskMenu label={`${label} 任务操作`} triggerRef={menuTrigger}>
                <button
                  disabled={!selected.canRename || anyBatchBusy}
                  title={!selected.canRename ? selected.renameBlockReason ?? undefined : undefined}
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
                  disabled={
                    !selected.canArchive ||
                    anyBatchBusy ||
                    actions.archive.isPending ||
                    actions.restore.isPending
                  }
                  title={
                    !selected.canArchive ? selected.archiveBlockReason ?? undefined : undefined
                  }
                  onClick={() => handleArchiveToggle(selected)}
                >
                  {selected.archived ? (
                    <>
                      <ArchiveRestore size={15} />
                      取消归档
                    </>
                  ) : (
                    <>
                      <Archive size={15} />
                      归档
                    </>
                  )}
                </button>
                <button
                  disabled={!latestReplyText}
                  title={!latestReplyText ? "当前会话暂无可复制的助手答复" : undefined}
                  onClick={() => {
                    void copy(latestReplyText, "已复制答复");
                  }}
                >
                  <Copy size={15} />
                  复制答复
                </button>
                <ThreadMenuItems
                  actions={[
                    {
                      id: "copy-id",
                      label: "复制 ID",
                      icon: <Copy size={15} />,
                      run: () => {
                        void copy(selected.id, "已复制 ID");
                      },
                    },
                    {
                      id: "copy-path",
                      label: "复制路径",
                      icon: <Copy size={15} />,
                      run: () => {
                        void copy(selected.path, "已复制路径");
                      },
                    },
                    {
                      id: "reveal",
                      label: "在 Finder 中显示",
                      icon: <FolderOpen size={15} />,
                      disabled: !canRevealLocalPath() || !selected.path,
                      reason: !canRevealLocalPath()
                        ? "仅可在本机 Finder 中显示本机会话"
                        : !selected.path
                          ? "当前会话暂无本地文件路径"
                          : null,
                      run: () => {
                        if (!selected.path) return;
                        void revealLocalPath(selected.path).catch(() =>
                          setFeedback("无法在 Finder 中显示，请检查文件是否仍存在"),
                        );
                      },
                    },
                    {
                      id: "delete",
                      label: "永久删除",
                      icon: <Trash2 size={15} />,
                      danger: true,
                      disabled: !selected.canDelete || anyBatchBusy,
                      reason: !selected.canDelete ? selected.deleteBlockReason : null,
                      run: () => {
                        activeDetailBatch.prepareSingle(
                          selected.sessionKey,
                          "delete",
                          menuTrigger.current,
                        );
                      },
                    },
                  ]}
                />
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
              ariaLabel={`${label} 任务名称`}
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
          {selected.readError && (
            <div className="form-error" role="alert">
              {selected.readError}
            </div>
          )}
          {selected.readWarning && (
            <DisclosureScope.Provider value={`${provider}:${selected.sessionKey}`}>
              <ActivityDetails
                className="task-feedback"
                summary="读取提示"
                initiallyOpen={false}
                stateKey="read-warning"
              >
                {() => <div>{selected.readWarning}</div>}
              </ActivityDetails>
            </DisclosureScope.Provider>
          )}
          {error && (
            <div className="form-error" role="alert">
              {error.message}
            </div>
          )}
          <div className="timeline-reading-shell">
            <TimelineRail
              entries={timelineEntries}
              streamRef={stream}
              onSelect={handleTimelineSelect}
              loadOlder={
                !detail.data?.timeline && detail.hasNextPage
                  ? () => {
                      void history.loadOlder("search");
                    }
                  : undefined
              }
              loading={history.loading}
            />
            <div
              ref={stream}
              className="provider-events"
              onScroll={history.onScroll}
              onScrollCapture={history.onScrollCapture}
              onWheel={history.onWheel}
              onTouchStart={history.onTouchStart}
              onTouchMove={history.onTouchMove}
              onKeyDown={history.onKeyDown}
              tabIndex={0}
            >
              <HistoryLoading
                loading={history.loading}
                error={history.error}
                retry={history.retry}
              />
              <UserMessageScope.Provider
                value={{ provider, sessionKey: selected.sessionKey }}
              >
                <MarkdownPathScope.Provider value={selected.cwd}>
                  <DisclosureScope.Provider value={`${provider}:${selected.sessionKey}`}>
                    <DisclosureRevealContext.Provider
                      value={
                        pendingSearch?.sessionKey === selected.sessionKey
                          ? pendingSearch.positionKey
                          : null
                      }
                    >
                      {renderProviderEvents(
                        groupedEvents,
                        selected.title,
                        label,
                        selected.status === "running",
                      )}
                    </DisclosureRevealContext.Provider>
                  </DisclosureScope.Provider>
                </MarkdownPathScope.Provider>
              </UserMessageScope.Provider>
              <TimelineHistoryStatus
                navigation={{ ...navigation, busy: navigation.busy || history.loading }}
              />
              {detail.isLoading && <div className="muted-row">正在读取消息...</div>}
              {!detail.isLoading && !events.length && (
                <div className="muted-row">暂无对话</div>
              )}
            </div>
          </div>
        </>
      )}
    </WorkspaceSplitShell>
  );
}

export function renderProviderEvents(
  entries: ExecutionRenderItem<ProviderHistoryEvent>[],
  title: string,
  providerLabel: string,
  sessionRunning = false,
): ReactNode {
  const prefix = providerLabel === "Antigravity" ? "antigravity" : "dsh";
  const lastIndex = entries.length - 1;
  const folded = foldProcessSequence(prefix, entries, entry => {
    if (entry.kind === "group") {
      return { kind: "execution", group: entry.group, turnId: entry.group.turnId };
    }
    const event = entry.item;
    if (event.kind === "thinking") {
      const isTrailing = entries.indexOf(entry) === lastIndex;
      return thinkingStepFromText(
        entry.key,
        entry.sourceIds,
        event.text,
        sessionRunning && isTrailing,
        event.turnId,
      );
    }
    if (
      event.detail &&
      event.kind !== "user_message" &&
      event.kind !== "plan" &&
      !event.kind.startsWith("assistant_message")
    ) {
      return {
        kind: "detail",
        id: entry.key,
        sourceIds: entry.sourceIds,
        title: event.text ?? "活动记录",
        detail: event.detail,
        failed: event.kind === "failure" || /^(failed|error)$/i.test(event.status ?? ""),
        turnId: event.turnId,
      };
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
      <ProviderEventView
        key={entry.item.key}
        activityId={entry.item.key}
        event={entry.item.item}
        title={title}
        providerLabel={providerLabel}
        timelineAliases={entry.item.sourceIds}
      />
    ),
  );
}

function ProviderEventView({
  event,
  activityId,
  title,
  providerLabel,
  timelineAliases = [],
}: {
  event: ProviderHistoryEvent;
  activityId: string;
  title: string;
  providerLabel: string;
  timelineAliases?: string[];
}) {
  const timelineProps = {
    "data-timeline-id": activityId,
    "data-timeline-aliases": timelineAliases.length ? timelineAliases.join(" ") : undefined,
  };
  if (event.kind === "user_message") {
    const aliases = [
      ...timelineAliases,
      ...(event.userMessage?.id ? [event.userMessage.id] : []),
    ];
    return (
      <UserMessage
        message={event.userMessage}
        text={event.text ?? ""}
        activityId={activityId}
        timelineId={activityId}
        timelineAliases={aliases}
      />
    );
  }
  if (event.kind === "plan") {
    return (
      <article className="provider-event plan" {...timelineProps}>
        <header className="plan-heading">
          <strong>计划</strong>
          <PlanActions markdown={event.text ?? ""} fallbackTitle={title} />
        </header>
        <MarkdownContent
          text={event.text ?? ""}
          activityId={activityId}
          foldInstructions={false}
        />
      </article>
    );
  }
  if (event.detail) {
    return (
      <ActivityDetails
        timelineId={activityId}
        timelineAliases={timelineAliases}
        className="grok-tool"
        stateKey={activityId}
        initiallyOpen={false}
        summary={<span className="tool-title">{event.text ?? "活动记录"}</span>}
      >
        {() => <ToolOutput text={event.detail!} />}
      </ActivityDetails>
    );
  }
  if (!visibleMarkdown(event.text ?? "").trim()) return null;
  const isAssistant = event.kind.startsWith("assistant_message");
  return (
    <article
      className={`provider-event ${event.kind}${isAssistant ? " assistant-reply" : ""}`}
      {...timelineProps}
    >
      {!isAssistant && (
        <div className="chat-meta">{providerEventLabel(event, providerLabel)}</div>
      )}
      <MarkdownContent
        text={event.text ?? ""}
        activityId={activityId}
        timelineAliases={timelineAliases}
        foldInstructions={event.kind !== "compaction" && event.kind !== "branch_summary"}
      />
      {isAssistant && <CopyReplyButton text={event.text ?? ""} />}
    </article>
  );
}

function providerEventLabel(event: ProviderHistoryEvent, providerLabel: string): string {
  if (event.kind === "user_message") return "你";
  if (event.kind === "assistant_message") return providerLabel;
  if (event.kind === "thinking") return "思考";
  if (event.kind === "failure") return "失败";
  if (event.kind === "compaction") return "压缩摘要";
  if (event.kind === "branch_summary") return "分支摘要";
  return event.role || providerLabel;
}

function providerStatusLabel(session: ProviderSessionSummary): string {
  if (session.status === "running") return "运行中";
  if (session.archived) return "已归档";
  if (session.status === "unknown") return "状态未知";
  return session.updatedAt ? new Date(session.updatedAt).toLocaleString() : "最近";
}

export function providerSessionLabel(session: Pick<ProviderSessionSummary, "id" | "title">): string {
  return session.title.trim() || session.id;
}
