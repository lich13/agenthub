import { Bot, Copy, FolderOpen, Pencil, Terminal, Trash2 } from "lucide-react";
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { useCopyFeedback } from "../../hooks/useCopyFeedback";
import { getClaudeSession } from "../../lib/api/claude";
import { canRevealLocalPath, revealLocalPath } from "../../lib/api/filePaths";
import { groupClaudeProcesses } from "../../lib/domain/claudeProcessGroups";
import { claudeResumeCommand } from "../../lib/domain/claudeSession";
import { useClaudeActions, useClaudeDetail, useClaudeSessions } from "../../lib/query/claude";
import { machineScope, useConnection } from "../../lib/query/connection";
import { useSessionSelection } from "../../lib/query/sessions";
import { groupedTimelineEntries, useNarrowViewport, useProviderTimelineController } from "../../lib/useProviderWorkspace";
import { claudeTimelinePage } from "../../lib/useTimelineWindow";
import type { ClaudeDeletePreview, ClaudeHistoryEvent, ClaudeSessionSummary, SessionSearchResult } from "../../types";
import { ActivityDetails, DisclosureScope } from "../common/ActivityDetails";
import { RenameableSession } from "../common/RenameableSession";
import { SessionBatchControls, buildBatchContextMenuActions } from "../common/SessionBatchControls";
import { TaskMenu, ThreadMenuItems, type ThreadMenuAction } from "../common/TaskMenu";
import {
  ProviderSessionRowBody,
  SessionDeleteConfirmDialog,
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
import { renderClaudeEvents } from "./ClaudeHistory";
import { ClaudeSubagentPanel } from "./ClaudeSubagentPanel";
import type { RuntimeCapabilityMatrix } from "../../lib/domain/capabilities";
export { renderClaudeEvents } from "./ClaudeHistory";

export function ClaudeWorkspace({ capabilities }: { capabilities?: RuntimeCapabilityMatrix } = {}) {
  const connection = useConnection();
  const menuTrigger = useRef<HTMLElement>(null);
  const stream = useRef<HTMLDivElement>(null);
  const reading = useRef<HTMLDivElement>(null);
  const [docked, setDocked] = useState(false);
  const [agentSelection, setAgentSelection] = useState<{ parent: string; machine: string; trigger: HTMLButtonElement } | null>(null);
  const machine = machineScope();
  const anchor = useRef<{ element: HTMLElement; offset: number } | null>(null);
  const captureReading = () => {
    const viewport = stream.current?.getBoundingClientRect();
    const element = Array.from(stream.current?.querySelectorAll<HTMLElement>("[data-timeline-id]") ?? [])
      .find(item => viewport && item.getBoundingClientRect().bottom > viewport.top);
    anchor.current = viewport && element ? { element, offset: element.getBoundingClientRect().top - viewport.top } : null;
  };
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
  const panelOpen = detailVisible && capabilities?.claudeSubagents === true && agentSelection?.parent === selected?.sessionKey && agentSelection?.machine === machine;
  const closeAgents = () => {
    captureReading();
    const trigger = agentSelection?.trigger;
    setAgentSelection(null);
    requestAnimationFrame(() => { if (trigger?.isConnected) trigger.focus({ preventScroll: true }); });
  };
  useEffect(() => { setAgentSelection(null); }, [selected?.sessionKey, machine]);
  useEffect(() => {
    const element = reading.current;
    if (!element) return;
    const resize = () => setDocked(element.clientWidth >= 960);
    resize();
    const observer = new ResizeObserver(resize);
    observer.observe(element);
    return () => observer.disconnect();
  }, [Boolean(selected)]);
  useLayoutEffect(() => {
    if (anchor.current && stream.current && anchor.current.element.isConnected) {
      stream.current.scrollTop += anchor.current.element.getBoundingClientRect().top - stream.current.getBoundingClientRect().top - anchor.current.offset;
      anchor.current = null;
    }
  }, [panelOpen, docked]);
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
            {capabilities?.claudeSubagents === true && <div className="subagent-summary"><button type="button" aria-label="本会话子智能体" onClick={event => {
              captureReading();
              setAgentSelection({ parent: selected.sessionKey, machine, trigger: event.currentTarget });
            }}><Bot size={16} /><span>本会话子智能体</span><span>{detail.data?.pages[0]?.subagents?.agents.length ?? 0}</span></button></div>}
            <div ref={reading} className="claude-reading-layout">
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
            {panelOpen && <ClaudeSubagentPanel key={`${machine}:${selected.sessionKey}`} parentKey={selected.sessionKey} collection={detail.data?.pages[0]?.subagents} docked={docked} onClose={closeAgents} />}
            </div>
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

export function claudeSessionLabel(session: ClaudeSessionSummary): string {
  return session.title || session.id;
}
