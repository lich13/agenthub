import { Copy, FolderOpen } from "lucide-react";
import { useCallback, useMemo, useRef, useState } from "react";
import { useCopyFeedback } from "../../hooks/useCopyFeedback";
import { canRevealLocalPath, revealLocalPath } from "../../lib/api/filePaths";
import { groupProviderEvents } from "../../lib/domain/executionGroups";
import { readArchivableProviderSession, useProviderDetail, useProviderSessions } from "../../lib/query/providerSessions";
import { groupedTimelineEntries, useNarrowViewport, useProviderTimelineController } from "../../lib/useProviderWorkspace";
import { providerTimelinePage } from "../../lib/useTimelineWindow";
import type { ProviderSessionSummary, SessionSearchResult } from "../../types";
import { RenameableSession } from "../common/RenameableSession";
import { TaskMenu, ThreadMenuItems, type ThreadMenuAction } from "../common/TaskMenu";
import {
  ProviderSessionRowBody, WorkspaceEmptyState, WorkspaceListHeader, WorkspaceSearchInput,
  WorkspaceSplitShell, WorkspaceStageHeader, WorkspaceTimelineReadingShell, latestAssistantReply,
} from "../common/WorkspaceSplitShell";
import { providerSessionLabel, renderProviderEvents } from "./ArchivableProviderWorkspace";

export function GrokBotWorkspace() {
  const stream = useRef<HTMLDivElement>(null);
  const [query, setQuery] = useState("");
  const [selectedKey, setSelectedKey] = useState<string | null>(null);
  const [pendingSearch, setPendingSearch] = useState<SessionSearchResult | null>(null);
  const { feedback, setFeedback, copy } = useCopyFeedback();
  const narrow = useNarrowViewport();
  const sessions = useProviderSessions("grokbot", { q: query }, true);
  const listed = sessions.data?.find(item => item.sessionKey === selectedKey) ?? sessions.data?.[0];
  const detailVisible = !narrow || Boolean(selectedKey);
  const detail = useProviderDetail("grokbot", detailVisible ? (pendingSearch ? selectedKey : listed?.sessionKey ?? null) : null, true);
  const selected = detail.data?.summary ?? listed;
  const loadedEvents = useMemo(() => detail.data?.events ?? [], [detail.data]);
  const onSelectSearchSession = useCallback((result: SessionSearchResult) => {
    setSelectedKey(result.sessionKey);
    setFeedback("");
  }, [setFeedback]);
  const { events, navigation, history, handleTimelineSelect, loadOlderForRail } = useProviderTimelineController({
    provider: "grokbot", sessionKey: selected?.sessionKey, streamRef: stream, loadedEvents,
    historyMeta: detail.data?.history, resolvedTimeline: detail.data?.timeline,
    beforeCursor: detail.data?.beforeCursor, hasNextPage: Boolean(detail.hasNextPage),
    ready: detailVisible && Boolean(detail.data), isLoading: detail.isLoading, isFetching: detail.isFetching,
    readWindowPage: async cursor => providerTimelinePage(await readArchivableProviderSession("grokbot", {
      sessionKey: selected!.sessionKey, limit: 160, before: cursor,
    })),
    fetchNextDetailPage: async () => {
      const result = await detail.fetchNextPage({ cancelRefetch: false });
      if (result.isError) throw result.error;
      const page = result.data?.pages[result.data.pages.length - 1];
      if (!page) throw new Error("历史读取失败");
      return page;
    },
    onSelectSearchSession, setFeedback, pendingSearchState: [pendingSearch, setPendingSearch],
  });
  const groupedEvents = useMemo(() => groupProviderEvents(events, "Grok Bot"), [events]);
  const timelineEntries = useMemo(() => groupedTimelineEntries("grokbot", detail.data?.timeline, groupedEvents), [detail.data?.timeline, groupedEvents]);
  const latestReply = useMemo(() => latestAssistantReply(events), [events]);
  const usePath = async (item: ProviderSessionSummary, reveal: boolean) => {
    try {
      if (!item.path) return;
      if (reveal) await revealLocalPath(item.path);
      else await copy(item.path, "已复制路径");
    } catch (error) {
      setFeedback(error instanceof Error ? error.message : "无法读取路径");
    }
  };
  const actions = (item: ProviderSessionSummary): ThreadMenuAction[] => [
    { id: "copy-id", label: "复制 ID", icon: <Copy size={15} />, run: () => void copy(item.id, "已复制 ID") },
    { id: "copy-path", label: "复制路径", icon: <Copy size={15} />, disabled: !item.path, run: () => void usePath(item, false) },
    { id: "reveal", label: "在 Finder 中显示", icon: <FolderOpen size={15} />, disabled: !item.path || !canRevealLocalPath(), run: () => void usePath(item, true) },
  ];
  const error = detail.isFetchNextPageError ? null : detail.error;
  return (
    <WorkspaceSplitShell hasSelection={Boolean(selectedKey)} sidebar={
      <>
        <WorkspaceListHeader agent="grokbot" title="Grok Bot" count={sessions.data?.length} onRefresh={() => { void sessions.refetch(); void detail.refetch(); }} refreshDisabled={sessions.isFetching || detail.isFetching} />
        <WorkspaceSearchInput value={query} onChange={setQuery} placeholder="搜索 Grok Bot" />
        <div className="provider-list-scroll">
          {narrow && !selectedKey && feedback && <div role="status" className="task-feedback">{feedback}</div>}
          {sessions.error && <div className="form-error" role="alert">{sessions.error.message}</div>}
          {!sessions.isLoading && !sessions.error && !sessions.data?.length && <div className="muted-row">未发现 Grok Bot 会话</div>}
          {sessions.isLoading && <div className="muted-row">正在读取任务...</div>}
          {sessions.data?.map(item => (
            <RenameableSession selected={selected?.sessionKey === item.sessionKey} key={item.sessionKey} title={providerSessionLabel(item)} className={`provider-session ${selected?.sessionKey === item.sessionKey ? "selected" : ""}`}
              onSelect={() => { setSelectedKey(item.sessionKey); setPendingSearch(null); setFeedback(""); }} menuActions={actions(item)}>
              <ProviderSessionRowBody title={providerSessionLabel(item)} statusText={grokBotStatus(item)} storageSize={item.storageSize} />
            </RenameableSession>
          ))}
        </div>
      </>
    }>
      {!selected && <WorkspaceEmptyState agent="grokbot" title="选择一个 Grok Bot 任务" />}
      {error && <div className="form-error" role="alert">{error.message}</div>}
      {selected && <>
        <WorkspaceStageHeader title={providerSessionLabel(selected)} onBack={() => setSelectedKey(null)}
          badges={<span className="status-chip tone-neutral">{grokBotStatus(selected)}</span>}
          actions={<TaskMenu label="Grok Bot 任务操作">
            <ThreadMenuItems itemRole="button" actions={[
              { id: "copy-reply", label: "复制答复", icon: <Copy size={15} />, disabled: !latestReply, run: () => void copy(latestReply, "已复制答复") },
              ...actions(selected),
            ]} />
          </TaskMenu>} />
        {feedback && <div role="status" className="task-feedback">{feedback}</div>}
        <WorkspaceTimelineReadingShell entries={timelineEntries} streamRef={stream} onSelect={handleTimelineSelect} loadOlder={loadOlderForRail}
          history={history} navigation={navigation} provider="grokbot" sessionKey={selected.sessionKey} pendingSearch={pendingSearch}
          isLoading={detail.isLoading} isEmpty={!events.length}>
          {renderProviderEvents(groupedEvents, selected.title, "Grok Bot")}
        </WorkspaceTimelineReadingShell>
      </>}
    </WorkspaceSplitShell>
  );
}

function grokBotStatus(item: ProviderSessionSummary): string {
  if (item.status === "authorization") return "需要授权";
  if (item.status === "reply_needed") return "需要回复";
  return item.path ? "近期缓存" : "未缓存";
}
