import { Copy, FolderOpen, Pencil, Terminal, Trash2 } from "lucide-react";
import { useCallback, useMemo, useRef, useState, type ReactNode } from "react";
import { useCopyFeedback } from "../../hooks/useCopyFeedback";
import { getClaudeSession } from "../../lib/api/claude";
import { canRevealLocalPath, revealLocalPath } from "../../lib/api/filePaths";
import { groupClaudeProcesses, type ClaudeRenderItem } from "../../lib/domain/claudeProcessGroups";
import { claudeResumeCommand } from "../../lib/domain/claudeSession";
import { foldProcessSequence, thinkingStepFromText } from "../../lib/domain/processGroups";
import { useClaudeActions, useClaudeDetail, useClaudeSessions } from "../../lib/query/claude";
import { useConnection } from "../../lib/query/connection";
import { useSessionSelection } from "../../lib/query/sessions";
import { groupedTimelineEntries, useNarrowViewport, useProviderTimelineController } from "../../lib/useProviderWorkspace";
import { claudeTimelinePage } from "../../lib/useTimelineWindow";
import type { ClaudeDeletePreview, ClaudeHistoryEvent, ClaudeSessionSummary, SessionSearchResult } from "../../types";
import { ActivityDetails, DisclosureScope } from "../common/ActivityDetails";
import { ExecutionGroupView } from "../common/ExecutionGroupView";
import { ProcessGroupView } from "../common/ProcessGroupView";
import { RenameableSession } from "../common/RenameableSession";
import { SessionBatchControls, buildBatchContextMenuActions } from "../common/SessionBatchControls";
import { TaskMenu, ThreadMenuItems, type ThreadMenuAction } from "../common/TaskMenu";
import {
  ProviderSessionRowBody,
  SessionDeleteConfirmDialog,
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
import { ClaudeProcessView } from "./ClaudeProcessView";

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
  const sessions = useClaudeSessions(query);
  const listed = sessions.data?.find(item => item.sessionKey === selectedKey) ?? sessions.data?.[0];
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
    for (const page of [...(detail.data?.pages ?? [])].reverse()) {
      for (const event of page.events) latest.set(event.id, event);
    }
    return [...latest.values()];
  }, [detail.data]);

  const resolvedTimeline = detail.data?.pages[0]?.timeline;
  const oldestPage = detail.data?.pages[detail.data.pages.length - 1];
  const onSelectSearchSession = useCallback((result: SessionSearchResult) => {
    setSelectedKey(result.sessionKey);
    setRenaming(false);
    setPreview(null);
    setFeedback("");
  }, []);

  const { events, navigation, history, pendingSearch, handleTimelineSelect, loadOlderForRail } =
    useProviderTimelineController({
      provider: "claude_code",
      scopePrefix: "claude",
      sessionKey: selected?.sessionKey,
      streamRef: stream,
      loadedEvents,
      historyMeta: detail.data?.pages[0]?.history,
      resolvedTimeline,
      beforeCursor: oldestPage?.beforeCursor,
      hasNextPage: Boolean(detail.hasNextPage),
      ready: detailVisible && Boolean(detail.data),
      isLoading: detail.isLoading,
      isFetching: detail.isFetching,
      readWindowPage: async cursor =>
        claudeTimelinePage(await getClaudeSession(selected!.sessionKey, cursor)),
      fetchNextDetailPage: async () => {
        const result = await detail.fetchNextPage({ cancelRefetch: false });
        if (result.isError) throw result.error;
        const page = result.data?.pages[result.data.pages.length - 1];
        if (!page) throw new Error("历史读取失败");
        return page;
      },
      onSelectSearchSession,
      setFeedback,
    });

  const groupedEvents = useMemo(() => groupClaudeProcesses(events), [events]);
  const latestReplyText = useMemo(() => latestAssistantReply(events), [events]);
  const timelineEntries = useMemo(
    () => groupedTimelineEntries("claude_code", resolvedTimeline, groupedEvents),
    [resolvedTimeline, groupedEvents],
  );
  const actions = useClaudeActions();
  const error =
    actions.rename.error ??
    actions.preview.error ??
    actions.remove.error ??
    (detail.isFetchNextPageError ? null : detail.error) ??
    sessions.error;

  const menuActions = (item: ClaudeSessionSummary): ThreadMenuAction[] => [
    { id: "copy-id", label: "复制线程 ID", icon: <Copy size={15} />, run: () => void copy(item.id) },
    { id: "copy-path", label: "复制文件路径", icon: <Copy size={15} />, run: () => void copy(item.path) },
    {
      id: "copy-resume",
      label: "复制恢复命令",
      icon: <Terminal size={15} />,
      run: () => void copy(claudeResumeCommand(item.cwd, item.id)),
    },
    {
      id: "reveal",
      label: "在 Finder 中显示",
      icon: <FolderOpen size={15} />,
      disabled: !canRevealLocalPath(),
      reason: canRevealLocalPath() ? null : "仅可在本机 Finder 中显示本机会话",
      run: () => {
        void revealLocalPath(item.path).catch(() =>
          setFeedback("无法在 Finder 中显示，请检查文件是否仍存在"),
        );
      },
    },
    {
      id: "delete",
      label: "删除任务文件",
      icon: <Trash2 size={15} />,
      danger: true,
      disabled: actions.preview.isPending,
      run: () => {
        actions.remove.reset();
        actions.preview.mutate(item.sessionKey, { onSuccess: setPreview });
      },
    },
  ];

  const roleLabels: Record<string, string> = {
    session: "会话",
    daemon: "后台服务",
    pty_host: "终端宿主",
    unknown: "未核验进程",
  };

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
            <WorkspaceSearchInput value={query} onChange={setQuery} placeholder="搜索标题、ID 或工作目录" />
            <div className="provider-list-scroll" onKeyDown={batch.onListKeyDown}>
              {sessions.error && <div className="form-error" role="alert">{sessions.error.message}</div>}
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
                    onRename={next => actions.rename.mutateAsync({ sessionKey: item.sessionKey, title: next })}
                    batchMenuActions={batchMenu}
                    menuActions={menuActions(item)}
                  >
                    <ProviderSessionRowBody
                      title={claudeSessionLabel(item)}
                      idText={item.id}
                      cwd={item.cwd}
                      running={item.status === "running"}
                      statusText={providerSessionStatusText(item.status, item.updatedAt)}
                      storageSize={item.storageSize}
                    />
                  </RenameableSession>
                </div>
              ))}
              {sessions.isLoading && <div className="muted-row">正在读取任务...</div>}
              {!sessions.isLoading && !sessions.data?.length && <div className="muted-row">未发现 Claude Code 会话</div>}
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
                ) : selected.status === "waiting" ? (
                  <span className="status-chip tone-warning">等待处理</span>
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
                    onClick={() => void copy(latestReplyText)}
                  >
                    <Copy size={15} />
                    复制答复
                  </button>
                  <ThreadMenuItems actions={menuActions(selected)} itemRole="button" />
                </TaskMenu>
              }
            />
            {feedback && <div role="status" className="task-feedback">{feedback}</div>}
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
            {selected.readWarning && (
              <DisclosureScope.Provider value={`claude:${selected.sessionKey}`}>
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
            {!selected.readError && selected.renameBlockReason && (
              <div className="task-feedback" role="status">
                {selected.renameBlockReason}
                {connection.target === "local" && (
                  <button
                    disabled={actions.diagnostics.isPending}
                    onClick={() => actions.diagnostics.mutate(selected.sessionKey)}
                  >
                    查看占用详情
                  </button>
                )}
              </div>
            )}
            {connection.target === "local" && actions.diagnostics.variables === selected.sessionKey && (
              <>
                {actions.diagnostics.error && (
                  <div className="form-error" role="alert">{actions.diagnostics.error.message}</div>
                )}
                {actions.diagnostics.data && (
                  <ActivityDetails
                    className="task-feedback"
                    summary="原生进程归属"
                    initiallyOpen
                    stateKey={`ownership:${selected.sessionKey}`}
                  >
                    <p>
                      {actions.diagnostics.data.ownership === "inactive"
                        ? "未发现关联的活跃会话进程"
                        : actions.diagnostics.data.ownership === "open"
                          ? "存在活跃会话或文件占用"
                          : "归属尚未确认"}
                    </p>
                    {actions.diagnostics.data.processes.map((process, index) => (
                      <p key={`${process.pid}:${index}`}>
                        PID {process.pid} · {roleLabels[process.role] ?? process.role} ·{" "}
                        {process.registrationVerified ? "登记已核验" : "登记未核验"}
                        {process.mismatchedFields.length > 0 && ` · 不匹配字段：${process.mismatchedFields.join("、")}`}
                      </p>
                    ))}
                  </ActivityDetails>
                )}
              </>
            )}
            {error && <div className="form-error" role="alert">{error.message}</div>}
            <WorkspaceTimelineReadingShell
              entries={timelineEntries}
              streamRef={stream}
              onSelect={handleTimelineSelect}
              loadOlder={loadOlderForRail}
              history={history}
              navigation={navigation}
              provider="claude_code"
              disclosurePrefix="claude"
              sessionKey={selected.sessionKey}
              cwd={selected.cwd}
              pendingSearch={pendingSearch}
              isLoading={detail.isLoading}
              isEmpty={!events.length}
            >
              {renderClaudeEvents(groupedEvents, selected.title)}
            </WorkspaceTimelineReadingShell>
          </>
        )}
      </WorkspaceSplitShell>
      {preview && (
        <SessionDeleteConfirmDialog
          labelledBy="claude-delete-title"
          title={preview.title}
          paths={preview.paths ?? [preview.path]}
          summaryText={`${preview.fileCount} 个会话及专属文件，${preview.bytes.toLocaleString()} 字节。`}
          busy={actions.remove.isPending}
          error={actions.remove.error}
          warnings={preview.warnings ?? undefined}
          residuals={actions.remove.data?.residuals ?? undefined}
          returnFocus={menuTrigger}
          onCancel={() => setPreview(null)}
          onConfirm={() =>
            actions.remove.mutate(
              { sessionKey: preview.sessionKey, confirmed: true },
              {
                onSuccess: result => {
                  if (!result.deleted) return;
                  setPreview(null);
                  setSelectedKey(current => (current === preview.sessionKey ? null : current));
                },
              },
            )
          }
        />
      )}
    </>
  );
}

function isFoldableClaudeCore(entry: ClaudeRenderItem): boolean {
  if (entry.kind === "group") return true;
  if (entry.kind !== "item") return false;
  if (entry.item.kind === "thinking") return true;
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

export function renderClaudeEvents(
  entries: ClaudeRenderItem[],
  title: string,
  insideProcess = false,
): ReactNode {
  if (!insideProcess) {
    const indexMap = new Map<ClaudeRenderItem, number>();
    entries.forEach((entry, idx) => indexMap.set(entry, idx));

    const folded = foldProcessSequence("claude", entries, entry => {
      if (entry.kind === "group") {
        return { kind: "execution", group: entry.group, turnId: entry.group.turnId };
      }
      if (
        entry.kind === "item" &&
        entry.item.kind === "thinking"
      ) {
        return thinkingStepFromText(entry.key, entry.sourceIds, entry.item.text, false, entry.item.turnId);
      }
      if (isNonCompactionMetadata(entry)) {
        const idx = indexMap.get(entry) ?? -1;
        const turnId = claudeEntryTurnId(entry);
        let left = idx - 1;
        while (
          left >= 0 &&
          isNonCompactionMetadata(entries[left]) &&
          sameOrUnsetTurn(turnId, claudeEntryTurnId(entries[left]))
        ) {
          left -= 1;
        }
        let right = idx + 1;
        while (
          right < entries.length &&
          isNonCompactionMetadata(entries[right]) &&
          sameOrUnsetTurn(turnId, claudeEntryTurnId(entries[right]))
        ) {
          right += 1;
        }
        const attachedLeft =
          left >= 0 && isFoldableClaudeCore(entries[left]) && sameOrUnsetTurn(turnId, claudeEntryTurnId(entries[left]));
        const attachedRight =
          right < entries.length &&
          isFoldableClaudeCore(entries[right]) &&
          sameOrUnsetTurn(turnId, claudeEntryTurnId(entries[right]));
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
    <StandardProviderEventView
      key={entry.key}
      activityId={entry.key}
      event={entry.item}
      title={title}
      providerLabel="Claude Code"
      timelineAliases={entry.sourceIds}
      insideProcess={insideProcess}
    />
  );
}

export function claudeSessionLabel(session: ClaudeSessionSummary): string {
  return session.title || session.id;
}
