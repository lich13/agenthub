import { Archive, ArchiveRestore, ChevronRight, Copy, FolderOpen, Pencil, Trash2 } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { useCopyFeedback } from "../../hooks/useCopyFeedback";
import { canRevealLocalPath, revealLocalPath } from "../../lib/api/filePaths";
import { groupProviderEvents, type ExecutionRenderItem } from "../../lib/domain/executionGroups";
import { foldProcessSequence, thinkingStepFromText } from "../../lib/domain/processGroups";
import {
  readArchivableProviderSession,
  useProviderActions,
  useProviderDetail,
  useProviderSessions,
  type ArchivableProviderId,
} from "../../lib/query/providerSessions";
import { useSessionSelection, type SessionSelection } from "../../lib/query/sessions";
import type { RuntimeCapabilityMatrix } from "../../lib/query/system";
import { groupedTimelineEntries, useNarrowViewport, useProviderTimelineController } from "../../lib/useProviderWorkspace";
import { providerTimelinePage } from "../../lib/useTimelineWindow";
import type { ProviderHistoryEvent, ProviderSessionSummary, SessionSearchResult } from "../../types";
import { ActivityDetails, DisclosureScope } from "../common/ActivityDetails";
import { ExecutionGroupView } from "../common/ExecutionGroupView";
import { ProcessGroupView } from "../common/ProcessGroupView";
import { RenameableSession } from "../common/RenameableSession";
import { SessionBatchControls, buildBatchContextMenuActions } from "../common/SessionBatchControls";
import { TaskMenu, ThreadMenuItems, type ThreadMenuAction } from "../common/TaskMenu";
import {
  ProviderSessionRowBody,
  StandardProviderEventView,
  WorkspaceEmptyState,
  WorkspaceInlineRename,
  WorkspaceListHeader,
  WorkspaceSearchInput,
  WorkspaceSplitShell,
  WorkspaceStageHeader,
  WorkspaceTimelineReadingShell,
  latestAssistantReply,
  providerSessionStatusText,
} from "../common/WorkspaceSplitShell";

export function AntigravityWorkspace({ capabilities }: { capabilities: RuntimeCapabilityMatrix }) {
  return <ArchivableProviderWorkspace provider="antigravity" capabilities={capabilities} />;
}

export function DshWorkspace({ capabilities }: { capabilities: RuntimeCapabilityMatrix }) {
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
  const supported = provider === "antigravity" ? capabilities.antigravitySessions : capabilities.dshSessions;
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
  const archivedSessions = useProviderSessions(provider, { q: query, archived: true }, queryEnabled && archivedOpen);
  const activeKeys = useMemo(() => (sessions.data ?? []).map(s => s.sessionKey), [sessions.data]);
  const archivedKeys = useMemo(() => (archivedSessions.data ?? []).map(s => s.sessionKey), [archivedSessions.data]);
  const allListed = useMemo(
    () => [...(sessions.data ?? []), ...(archivedOpen ? (archivedSessions.data ?? []) : [])],
    [archivedOpen, archivedSessions.data, sessions.data],
  );
  const listed = allListed.find(item => item.sessionKey === selectedKey) ?? sessions.data?.[0];
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
  const batch = useSessionSelection(provider, `active:${query}`, activeKeys, handleBatchSucceeded, activeSelectedKey);
  const archivedBatch = useSessionSelection(
    provider,
    `archived:${query}`,
    archivedKeys,
    handleBatchSucceeded,
    archivedSelectedKey,
  );

  const detailTargetKey = pendingSearch && selectedKey ? selectedKey : listed?.sessionKey ?? null;
  const detailVisible = !narrow || Boolean(selectedKey);
  const detail = useProviderDetail(provider, detailVisible ? detailTargetKey : null, queryEnabled);
  const selected = detail.data?.summary ?? listed;

  const loadedEvents = useMemo(() => {
    const latest = new Map<string, ProviderHistoryEvent>();
    for (const page of detail.data ? [detail.data] : []) {
      for (const event of page.events) latest.set(event.id, event);
    }
    return [...latest.values()];
  }, [detail.data]);

  const onSelectSearchSession = useCallback((result: SessionSearchResult) => {
    setSelectedKey(result.sessionKey);
    setRenaming(false);
    setFeedback("");
  }, []);

  const { events, navigation, history, handleTimelineSelect, loadOlderForRail } =
    useProviderTimelineController({
      provider,
      sessionKey: selected?.sessionKey,
      streamRef: stream,
      loadedEvents,
      historyMeta: detail.data?.history,
      resolvedTimeline: detail.data?.timeline,
      beforeCursor: detail.data?.beforeCursor,
      hasNextPage: Boolean(detail.hasNextPage),
      ready: detailVisible && Boolean(detail.data),
      isLoading: detail.isLoading,
      isFetching: detail.isFetching,
      readWindowPage: async cursor =>
        providerTimelinePage(
          await readArchivableProviderSession(provider, {
            sessionKey: selected!.sessionKey,
            limit: 160,
            before: cursor,
          }),
        ),
      fetchNextDetailPage: async () => {
        const result = await detail.fetchNextPage({ cancelRefetch: false });
        if (result.isError) throw result.error;
        const pages = result.data?.pages;
        const page = pages?.[pages.length - 1];
        if (!page) throw new Error("历史读取失败");
        return page;
      },
      onSelectSearchSession,
      setFeedback,
      pendingSearchState: [pendingSearch, setPendingSearch],
    });

  const groupedEvents = useMemo(() => groupProviderEvents(events, label), [events, label]);
  const timelineEntries = useMemo(
    () => groupedTimelineEntries(provider, detail.data?.timeline, groupedEvents),
    [detail.data?.timeline, groupedEvents, provider],
  );
  const latestReplyText = useMemo(() => latestAssistantReply(events), [events]);

  const actions = useProviderActions(provider);
  const error =
    actions.rename.error ??
    actions.archive.error ??
    actions.restore.error ??
    (detail.isFetchNextPageError ? null : detail.error) ??
    sessions.error;

  useEffect(() => {
    if (
      pendingSearch &&
      detail.data?.summary.sessionKey === pendingSearch.sessionKey &&
      detail.data.summary.archived
    ) {
      setArchivedOpen(true);
    }
  }, [detail.data?.summary.archived, detail.data?.summary.sessionKey, pendingSearch]);

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
            const next = (index >= 0 ? list[index + 1] ?? list[index - 1] : undefined)?.sessionKey ?? null;
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
    returnFocus?: HTMLElement | null,
  ): ThreadMenuAction[] => [
    {
      id: item.archived ? "restore" : "archive",
      label: item.archived ? "恢复" : "归档",
      icon: item.archived ? <ArchiveRestore size={15} /> : <Archive size={15} />,
      disabled:
        !item.canArchive || targetBatch.busy || actions.archive.isPending || actions.restore.isPending,
      reason: !item.canArchive ? item.archiveBlockReason : null,
      run: () => handleArchiveToggle(item),
    },
    { id: "copy-id", label: "复制 ID", icon: <Copy size={15} />, run: () => void copy(item.id, "已复制 ID") },
    { id: "copy-path", label: "复制路径", icon: <Copy size={15} />, run: () => void copy(item.path, "已复制路径") },
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
      run: () => targetBatch.prepareSingle(item.sessionKey, "delete", returnFocus),
    },
  ];

  const activeBatchMenu = useMemo(() => buildBatchContextMenuActions(batch, ["archive", "delete"]), [batch]);
  const archivedBatchMenu = useMemo(
    () => buildBatchContextMenuActions(archivedBatch, ["restore", "delete"]),
    [archivedBatch],
  );

  const renderSessionRow = (
    item: ProviderSessionSummary,
    targetBatch: SessionSelection,
    otherBatch: SessionSelection,
    batchMenu: ThreadMenuAction[],
  ) => {
    const isSelected = item.sessionKey === selected?.sessionKey;
    return (
      <div key={item.sessionKey} className="selectable-session">
        <RenameableSession
          title={providerSessionLabel(item)}
          className={`provider-session ${isSelected ? "selected" : ""}`}
          disabled={targetBatch.busy}
          selecting={targetBatch.selecting}
          selected={isSelected}
          multiSelected={targetBatch.selection.includes(item.sessionKey)}
          renameBlockReason={!item.canRename ? item.renameBlockReason || "当前线程不可改名" : undefined}
          onSelect={event => {
            if (otherBatch.selecting || otherBatch.selection.length) otherBatch.clear();
            targetBatch.select(item.sessionKey, event, () => {
              setSelectedKey(item.sessionKey);
              setRenaming(false);
              setFeedback("");
            });
          }}
          onRename={next => actions.rename.mutateAsync({ sessionKey: item.sessionKey, title: next })}
          batchMenuActions={batchMenu}
          menuActions={rowMenuActions(item, targetBatch)}
        >
          <ProviderSessionRowBody
            title={providerSessionLabel(item)}
            idText={item.id}
            cwd={item.cwd}
            running={item.status === "running"}
            statusText={providerSessionStatusText(item.status, item.updatedAt, item.archived)}
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
                {sessions.error && <div className="form-error" role="alert">{sessions.error.message}</div>}
                {sessions.data?.map(item => renderSessionRow(item, batch, archivedBatch, activeBatchMenu))}
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
                          <div className="form-error" role="alert">{archivedSessions.error.message}</div>
                        )}
                        {archivedSessions.isLoading && <div className="muted-row">正在读取已归档会话...</div>}
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
                <SessionBatchControls batch={archivedBatch} operations={["restore", "delete"]} />
              )}
            </>
          )}
        </>
      }
    >
      {!selected && (
        <WorkspaceEmptyState
          agent={provider}
          title={supported === false ? `升级远程服务后即可浏览 ${label} 会话` : `选择一个 ${label} 任务`}
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
                  title={!selected.canArchive ? selected.archiveBlockReason ?? undefined : undefined}
                  onClick={() => handleArchiveToggle(selected)}
                >
                  {selected.archived ? <ArchiveRestore size={15} /> : <Archive size={15} />}
                  {selected.archived ? "取消归档" : "归档"}
                </button>
                <button
                  disabled={!latestReplyText}
                  title={!latestReplyText ? "当前会话暂无可复制的助手答复" : undefined}
                  onClick={() => void copy(latestReplyText, "已复制答复")}
                >
                  <Copy size={15} />
                  复制答复
                </button>
                <ThreadMenuItems
                  actions={rowMenuActions(selected, activeDetailBatch, menuTrigger.current).slice(1)}
                  itemRole="button"
                />
              </TaskMenu>
            }
          />
          {feedback && <div role="status" className="task-feedback">{feedback}</div>}
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
          {selected.readError && <div className="form-error" role="alert">{selected.readError}</div>}
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
          {error && <div className="form-error" role="alert">{error.message}</div>}
          <WorkspaceTimelineReadingShell
            entries={timelineEntries}
            streamRef={stream}
            onSelect={handleTimelineSelect}
            loadOlder={loadOlderForRail}
            history={history}
            navigation={navigation}
            provider={provider}
            sessionKey={selected.sessionKey}
            cwd={selected.cwd}
            pendingSearch={pendingSearch}
            isLoading={detail.isLoading}
            isEmpty={!events.length}
          >
            {renderProviderEvents(groupedEvents, selected.title, label, selected.status === "running")}
          </WorkspaceTimelineReadingShell>
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
  const prefix = providerLabel === "Antigravity" ? "antigravity" : providerLabel === "Grok Bot" ? "grokbot" : "dsh";
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
      event.kind !== "interaction" &&
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
      <StandardProviderEventView
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

export function providerSessionLabel(session: Pick<ProviderSessionSummary, "id" | "title">): string {
  return session.title.trim() || session.id;
}
