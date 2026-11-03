import { SessionSize } from "../common/SessionSize";
import {
  WorkspaceEmptyState,
  WorkspaceListHeader,
  WorkspaceSearchInput,
  WorkspaceSplitShell,
} from "../common/WorkspaceSplitShell";
import {
  Archive,
  ArchiveRestore,
  ChevronRight,
  Copy,
  FolderOpen,
  Trash2,
} from "lucide-react";
import { canRevealLocalPath, revealLocalPath } from "../../lib/api/filePaths";
import { useCallback, useEffect, useMemo, useState, type ReactNode } from "react";
import {
  threadDetailFromSlot,
  useConversationController,
} from "../../hooks/useConversationController";
import {
  isThreadListItemRunning,
  threadListItemPreviewText,
  threadListItemStatusText,
  threadListItemText,
  type SelectedThread,
  type View,
} from "../../lib/domain/codexViewModel";
import type { RuntimeCapabilityMatrix } from "../../lib/query/system";
import type { SessionSearchResult, ThreadSummary } from "../../types";
import { Conversation } from "./Conversation";
import { useSessionSelection, type SessionSelection } from "../../lib/query/sessions";
import {
  SessionBatchControls,
  buildBatchContextMenuActions,
} from "../common/SessionBatchControls";
import { RunningIndicator } from "../common/RunningIndicator";
import { RenameableSession } from "../common/RenameableSession";
import { useReadOnlyThreadActions } from "../../lib/query/threads";
import { useCopyFeedback } from "../../hooks/useCopyFeedback";
import { threadResumeCommand } from "../../lib/domain/conversationViewModel";
import { sharedDisabledStates } from "../../lib/domain/visualContract";
import { useSearchWorkspace } from "../common/SessionSearch";
import { NewDesktopThread } from "./DesktopComposer";
import { useConnection } from "../../lib/query/connection";

export function ChatWorkspace({
  mobileThreadsOpen,
  setMobileThreadsOpen,
  setView,
  capabilities,
}: {
  mobileThreadsOpen: boolean;
  setMobileThreadsOpen: (open: boolean) => void;
  setView: (view: View) => void;
  capabilities: RuntimeCapabilityMatrix;
}) {
  const [archivedOpen, setArchivedOpen] = useState(false);
  const [q, setQ] = useState("");
  const connection = useConnection();
  const {
    threadCache,
    messageStore,
    threads,
    archivedThreads,
    visibleThreads,
    visibleArchivedThreads,
    resolvedSelected,
    selectedThreadSummary,
    selectedDetail,
    detailLoading,
    nextThreadAfterRemoval,
    selectThread,
  } = useConversationController({
    archivedOpen,
    q,
    setMobileThreadsOpen,
  });

  const [pendingSearch, setPendingSearch] = useState<SessionSearchResult | null>(null);
  const selectSearchResult = useCallback(
    (result: SessionSearchResult) => {
      selectThread(result.sessionKey);
      setPendingSearch(result);
    },
    [selectThread],
  );

  useEffect(() => {
    if (
      pendingSearch &&
      !threads.isLoading &&
      !visibleThreads.some(thread => thread.id === pendingSearch.sessionKey)
    ) {
      setArchivedOpen(true);
    }
  }, [pendingSearch, threads.isLoading, visibleThreads]);

  useSearchWorkspace(
    useMemo(
      () => ({
        provider: "codex" as const,
        sessionKey: resolvedSelected,
        selectResult: selectSearchResult,
      }),
      [resolvedSelected, selectSearchResult],
    ),
  );

  const list = (
    <ThreadList
      q={q}
      setQ={setQ}
      threads={visibleThreads}
      archivedOpen={archivedOpen}
      setArchivedOpen={setArchivedOpen}
      archivedThreads={visibleArchivedThreads}
      archivedLoading={archivedThreads.isLoading}
      archivedError={archivedThreads.error?.message}
      selectedId={resolvedSelected}
      onSelect={selectThread}
      onRefresh={() => threadCache.invalidateThreads()}
      loading={threads.isLoading}
      newThread={
        capabilities.codexDesktop === true && connection.target === "local" ? (
          <NewDesktopThread
            onCreated={async id => {
              setQ("");
              await threadCache.invalidateThreads();
              selectThread(id);
            }}
          />
        ) : undefined
      }
      error={threads.error?.message}
      onBatchSucceeded={keys => {
        for (const key of keys) threadCache.clearArchivedThreadClientState(messageStore, key);
        if (resolvedSelected && keys.includes(resolvedSelected)) selectThread(null);
      }}
    />
  );

  const selectedSlot = resolvedSelected ? messageStore.getSlot(resolvedSelected) : null;
  const selectedFallback = selectedThreadSummary ?? selectedSlot?.summary ?? null;
  const isSelectedArchived = Boolean(
    selectedFallback &&
      (selectedFallback.status === "Archived" || Boolean(selectedFallback.archived_at)),
  );

  return (
    <WorkspaceSplitShell
      layoutClass="chat-layout"
      sidebarClass="thread-column"
      stageClass="conversation-column"
      stageTag="section"
      hasSelection={Boolean(!mobileThreadsOpen && resolvedSelected)}
      sidebar={list}
    >
      {resolvedSelected && selectedFallback ? (
        <Conversation
          threadId={resolvedSelected}
          detail={
            selectedDetail ??
            threadDetailFromSlot(
              resolvedSelected,
              selectedSlot ?? messageStore.getSlot(resolvedSelected),
              selectedFallback,
            )
          }
          selectedSummary={selectedThreadSummary}
          archivedView={isSelectedArchived}
          slot={messageStore.getSlot(resolvedSelected)}
          messageStore={messageStore}
          onSelect={id => selectThread(id)}
          onPanelSelect={setView}
          nextThreadAfterArchive={nextThreadAfterRemoval}
          capabilities={capabilities}
          onBack={() => setMobileThreadsOpen(true)}
          searchTarget={pendingSearch}
          searchReady={Boolean(selectedDetail || selectedSlot?.blocks.length)}
          onSearchResolved={() => setPendingSearch(null)}
          onSearchMiss={() => {
            setPendingSearch(null);
          }}
        />
      ) : (
        <WorkspaceEmptyState
          agent="codex"
          title={detailLoading ? "正在读取任务" : "选择一个任务"}
          subtitle="从左侧选择或新建会话，浏览完整消息流与命令执行时间线"
        />
      )}
    </WorkspaceSplitShell>
  );
}

function ThreadList({
  q,
  setQ,
  threads,
  archivedOpen,
  setArchivedOpen,
  archivedThreads,
  archivedLoading,
  archivedError,
  selectedId,
  onSelect,
  onRefresh,
  loading,
  error,
  onBatchSucceeded,
  newThread,
}: {
  q: string;
  setQ: (value: string) => void;
  threads: ThreadSummary[];
  archivedOpen: boolean;
  setArchivedOpen: (updater: boolean | ((open: boolean) => boolean)) => void;
  archivedThreads: ThreadSummary[];
  archivedLoading: boolean;
  archivedError?: string;
  selectedId: string | null;
  onSelect: (id: SelectedThread) => void;
  onRefresh: () => void;
  loading: boolean;
  error?: string;
  onBatchSucceeded: (keys: string[]) => void;
  newThread?: ReactNode;
}) {
  const activeIds = useMemo(() => threads.map(t => t.id), [threads]);
  const archivedIds = useMemo(() => archivedThreads.map(t => t.id), [archivedThreads]);
  const activeSelectedId = selectedId && activeIds.includes(selectedId) ? selectedId : null;
  const archivedSelectedId = selectedId && archivedIds.includes(selectedId) ? selectedId : null;

  const batch = useSessionSelection(
    "codex",
    `active\0${q}`,
    activeIds,
    onBatchSucceeded,
    activeSelectedId,
  );
  const archivedBatch = useSessionSelection(
    "codex",
    `archived\0${q}`,
    archivedIds,
    onBatchSucceeded,
    archivedSelectedId,
  );
  const rename = useReadOnlyThreadActions({});
  const { feedback: menuFeedback, setFeedback: setMenuFeedback, copy: copyWithFeedback } =
    useCopyFeedback();
  const copy = (value: string | null | undefined) =>
    copyWithFeedback(value, "已复制", "没有可复制的内容");

  const activeBatchMenu = useMemo(
    () => buildBatchContextMenuActions(batch, ["archive", "delete"]),
    [batch],
  );
  const archivedBatchMenu = useMemo(
    () => buildBatchContextMenuActions(archivedBatch, ["restore", "delete"]),
    [archivedBatch],
  );

  const renderThreadRow = (
    thread: ThreadSummary,
    targetBatch: SessionSelection,
    otherBatch: SessionSelection,
    batchMenu: ReturnType<typeof buildBatchContextMenuActions>,
  ) => {
    const title = threadListItemText(thread);
    const preview = threadListItemPreviewText(thread);
    const running = isThreadListItemRunning(thread);
    const isArchived = thread.status === "Archived" || Boolean(thread.archived_at);
    const multiSelected = targetBatch.selection.includes(thread.id);
    return (
      <div className="selectable-session" key={thread.id}>
        <RenameableSession
          title={title}
          className={`thread-item ${selectedId === thread.id ? "selected" : ""}${running ? " running" : ""}`}
          disabled={targetBatch.busy}
          selecting={targetBatch.selecting}
          selected={selectedId === thread.id}
          multiSelected={multiSelected}
          renameBlockReason={isArchived ? sharedDisabledStates.renameArchivedThread : undefined}
          onSelect={event => {
            if (otherBatch.selecting || otherBatch.selection.length) otherBatch.clear();
            targetBatch.select(thread.id, event, () => onSelect(thread.id));
          }}
          onRename={next => rename.mutateAsync({ kind: "rename", id: thread.id, title: next })}
          batchMenuActions={batchMenu}
          menuActions={[
            {
              id: "archive",
              label: isArchived ? "恢复" : "归档",
              icon: isArchived ? <ArchiveRestore size={15} /> : <Archive size={15} />,
              disabled: targetBatch.busy,
              run: () =>
                targetBatch.prepareSingle(thread.id, isArchived ? "restore" : "archive"),
            },
            {
              id: "copy-id",
              label: "复制 ID",
              icon: <Copy size={15} />,
              run: () => {
                void copy(thread.id);
              },
            },
            {
              id: "copy-path",
              label: "复制路径",
              icon: <Copy size={15} />,
              disabled: !thread.rollout_path,
              run: () => {
                void copy(thread.rollout_path);
              },
            },
            {
              id: "copy-resume",
              label: "复制恢复命令",
              icon: <Copy size={15} />,
              run: () => {
                void copy(threadResumeCommand(thread.id));
              },
            },
            {
              id: "reveal",
              label: "在 Finder 中显示",
              icon: <FolderOpen size={15} />,
              disabled: !canRevealLocalPath() || !thread.rollout_path,
              reason: !canRevealLocalPath()
                ? "仅可在本机 Finder 中显示本机会话"
                : !thread.rollout_path
                  ? "当前会话暂无本地文件路径"
                  : null,
              run: () => {
                if (!thread.rollout_path) return;
                void revealLocalPath(thread.rollout_path).catch(() =>
                  setMenuFeedback("无法在 Finder 中显示，请检查文件是否仍存在"),
                );
              },
            },
            {
              id: "delete",
              label: "永久删除",
              icon: <Trash2 size={15} />,
              danger: true,
              disabled: targetBatch.busy,
              run: () => targetBatch.prepareSingle(thread.id, "delete"),
            },
          ]}
        >
          <span className="thread-item-content">
            <span className="thread-item-title">{title}</span>
            <span className="thread-item-meta">
              {running ? (
                <RunningIndicator />
              ) : (
                <span className={`thread-item-status ${thread.status}`}>
                  {threadListItemStatusText(thread)}
                </span>
              )}
              {preview && <span className="thread-item-preview">{preview}</span>}
              <SessionSize size={thread.storageSize} />
            </span>
          </span>
        </RenameableSession>
      </div>
    );
  };

  return (
    <div className="thread-list">
      <WorkspaceListHeader
        agent="codex"
        title="Codex"
        count={loading ? null : threads.length}
        onRefresh={onRefresh}
        actions={newThread}
      />
      <WorkspaceSearchInput value={q} onChange={setQ} placeholder="搜索标题或 ID" />
      <div className="thread-scroll" onKeyDown={batch.onListKeyDown}>
        {error && (
          <div className="form-error" role="alert">
            {error}
          </div>
        )}
        {loading && <div className="muted-row">正在读取 Codex 状态...</div>}
        {threads.map((thread) => {
          return renderThreadRow(thread, batch, archivedBatch, activeBatchMenu);
        })}
        {!loading && !error && threads.length === 0 && <div className="muted-row">没有匹配任务</div>}
        <div className={`archived-drawer ${archivedOpen ? "is-open" : ""}`}>
          <button
            type="button"
            className="archived-toggle"
            aria-expanded={archivedOpen}
            onClick={() => setArchivedOpen(open => !open)}
          >
            <ChevronRight size={14} className="archived-chevron" aria-hidden="true" />
            <span>已归档</span>
            {archivedOpen && !archivedLoading && (
              <small aria-hidden="true">{archivedThreads.length}</small>
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
              {archivedError && (
                <div className="form-error" role="alert">
                  {archivedError}
                </div>
              )}
              {archivedLoading && <div className="muted-row">正在读取已归档任务...</div>}
              {archivedThreads.map(thread =>
                renderThreadRow(thread, archivedBatch, batch, archivedBatchMenu),
              )}
              {!archivedLoading && !archivedError && archivedThreads.length === 0 && (
                <div className="muted-row">暂无已归档任务</div>
              )}
            </div>
          )}
        </div>
      </div>
      <SessionBatchControls batch={batch} operations={["archive", "delete"]} />
      {archivedOpen && (
        <SessionBatchControls batch={archivedBatch} operations={["restore", "delete"]} />
      )}
      {menuFeedback && (
        <div role="status" className="task-feedback">
          {menuFeedback}
        </div>
      )}
    </div>
  );
}
