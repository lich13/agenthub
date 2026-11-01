import { Check, ChevronLeft, RefreshCw, Search, Trash2, X } from "lucide-react";
import type { ReactNode, RefObject } from "react";
import type { SessionProvider } from "../../lib/api/sessions";
import { visibleMarkdown } from "../../lib/domain/visibleMarkdown";
import type { TimelineEntry } from "../../lib/domain/timelineViewModel";
import type { useHistoryScroll } from "../../lib/useHistoryScroll";
import type { SessionSearchResult, SessionStorageSize, UserMessageContent } from "../../types";
import { ActivityDetails, DisclosureRevealContext, DisclosureScope } from "./ActivityDetails";
import { AgentBrandIcon, type AgentBrandId } from "./AgentBrandIcon";
import { ConfirmDialog } from "./ConfirmDialog";
import { CopyReplyButton } from "./CopyReplyButton";
import { MarkdownPathScope, ToolOutput } from "./FilePathLink";
import { HistoryLoading } from "./HistoryLoading";
import { MarkdownContent } from "./MarkdownContent";
import { PlanActions } from "./PlanActions";
import { RunningIndicator } from "./RunningIndicator";
import { SessionSize } from "./SessionSize";
import { TimelineHistoryStatus } from "./TimelineHistoryStatus";
import { TimelineRail } from "./TimelineRail";
import { UserMessage, UserMessageScope } from "./UserMessage";

export function WorkspaceSplitShell({
  layoutClass = "provider-workspace",
  sidebarClass = "provider-list",
  stageClass = "provider-detail",
  hasSelection = false,
  stageTag: Stage = "main",
  sidebar,
  children,
}: {
  layoutClass?: string;
  sidebarClass?: string;
  stageClass?: string;
  hasSelection?: boolean;
  stageTag?: "main" | "section";
  sidebar: ReactNode;
  children: ReactNode;
}) {
  return (
    <div className={`ws-split-layout ${layoutClass}${hasSelection ? " has-selection" : ""}`}>
      <aside className={`ws-sidebar ${sidebarClass}`}>{sidebar}</aside>
      <Stage className={`ws-stage ${stageClass}`}>{children}</Stage>
    </div>
  );
}

export function WorkspaceListHeader(props: {
  agent: AgentBrandId;
  title: string;
  count?: number | null;
  onRefresh: () => void;
  refreshDisabled?: boolean;
  actions?: ReactNode;
}) {
  return (
    <header className="workspace-heading">
      <div className="workspace-title-brand">
        <span className="agent-brand-badge" data-agent={props.agent} aria-hidden="true">
          <AgentBrandIcon agent={props.agent} size={16} />
        </span>
        <h1>{props.title}</h1>
        {typeof props.count === "number" && (
          <span className="workspace-count-pill" aria-hidden="true">{props.count}</span>
        )}
      </div>
      <div className="thread-title-actions">
        {props.actions}
        <button
          className="icon-button compact"
          disabled={props.refreshDisabled}
          onClick={props.onRefresh}
          title="刷新任务"
        >
          <RefreshCw size={15} />
        </button>
      </div>
    </header>
  );
}

export function WorkspaceSearchInput(props: {
  value: string;
  onChange: (value: string) => void;
  placeholder: string;
  disabled?: boolean;
}) {
  return (
    <label className="search-box">
      <Search size={15} />
      <input
        value={props.value}
        disabled={props.disabled}
        onChange={event => props.onChange(event.target.value)}
        placeholder={props.placeholder}
      />
    </label>
  );
}

export function WorkspaceEmptyState(props: { agent: AgentBrandId; title: string; subtitle: string }) {
  return (
    <div className="empty-state">
      <span className="empty-brand-badge" data-agent={props.agent} aria-hidden="true">
        <AgentBrandIcon agent={props.agent} size={26} />
      </span>
      <strong>{props.title}</strong>
      <small>{props.subtitle}</small>
    </div>
  );
}

export function WorkspaceStageHeader(props: {
  title: string;
  subtitle?: string | null;
  onBack: () => void;
  badges?: ReactNode;
  actions?: ReactNode;
}) {
  return (
    <header className="conversation-header" data-timeline-id="session">
      <button className="icon-button mobile-back" title="返回任务列表" onClick={props.onBack}>
        <ChevronLeft size={18} />
      </button>
      <div className="conversation-title-copy">
        <h1 className="conversation-title">{props.title}</h1>
        {props.subtitle && (
          <span className="conversation-subtitle" title={props.subtitle}>{props.subtitle}</span>
        )}
      </div>
      <div className="conversation-header-actions">
        {props.badges}
        {props.actions}
      </div>
    </header>
  );
}

export function WorkspaceInlineRename(props: {
  ariaLabel: string;
  value: string;
  onChange: (value: string) => void;
  busy?: boolean;
  onSubmit: () => void;
  onCancel: () => void;
}) {
  return (
    <form
      className="inline-rename"
      onSubmit={event => {
        event.preventDefault();
        props.onSubmit();
      }}
    >
      <input
        aria-label={props.ariaLabel}
        maxLength={200}
        value={props.value}
        onChange={event => props.onChange(event.target.value)}
        autoFocus
      />
      <button className="icon-button" title="保存名称" disabled={props.busy || !props.value.trim()}>
        <Check size={17} />
      </button>
      <button className="icon-button" type="button" title="取消改名" onClick={props.onCancel}>
        <X size={17} />
      </button>
    </form>
  );
}

export function ProviderSessionRowBody(props: {
  title: string;
  idText?: string | null;
  cwd?: string | null;
  running?: boolean;
  statusText: string;
  storageSize?: SessionStorageSize | null;
}) {
  return (
    <>
      <strong>{props.title}</strong>
      {props.idText && <span>{props.idText}</span>}
      {props.cwd && <span className="provider-session-path">{props.cwd}</span>}
      <span className="session-status-row">
        {props.running ? <RunningIndicator /> : <small>{props.statusText}</small>}
        <SessionSize size={props.storageSize} />
      </span>
    </>
  );
}

export function latestAssistantReply(
  events: ReadonlyArray<{ kind: string; text?: string | null }>,
  includeAgentMessage = false,
): string {
  for (let index = events.length - 1; index >= 0; index -= 1) {
    const candidate = events[index];
    const isMatch =
      candidate.kind.startsWith("assistant_message") ||
      (includeAgentMessage && candidate.kind.startsWith("agent_message"));
    if (!isMatch) continue;
    const cleaned = visibleMarkdown(candidate.text ?? "").trim();
    if (cleaned) return cleaned;
  }
  return "";
}

export function providerSessionStatusText(status: string, updatedAt?: string | null, archived = false): string {
  if (status === "running") return "运行中";
  if (archived) return "已归档";
  if (status === "unknown") return "状态未知";
  return updatedAt ? new Date(updatedAt).toLocaleString() : "最近";
}

export function WorkspaceTimelineReadingShell(props: {
  entries: TimelineEntry[];
  streamRef: RefObject<HTMLDivElement>;
  onSelect: (id: string) => void;
  loadOlder?: () => void;
  history: ReturnType<typeof useHistoryScroll>;
  navigation: {
    active: boolean;
    busy: boolean;
    error: string | null;
    hasNewer: boolean;
    latest: () => void;
    newer: () => void;
  };
  provider: SessionProvider;
  disclosurePrefix?: string;
  sessionKey: string;
  cwd?: string | null;
  pendingSearch: SessionSearchResult | null;
  isLoading: boolean;
  isEmpty: boolean;
  children: ReactNode;
}) {
  const { history, navigation, provider, sessionKey, pendingSearch } = props;
  const prefix = props.disclosurePrefix ?? provider;
  return (
    <div className="timeline-reading-shell">
      <TimelineRail
        entries={props.entries}
        streamRef={props.streamRef}
        onSelect={props.onSelect}
        loadOlder={props.loadOlder}
        loading={history.loading}
      />
      <div
        ref={props.streamRef}
        className="provider-events"
        onScroll={history.onScroll}
        onScrollCapture={history.onScrollCapture}
        onWheel={history.onWheel}
        onTouchStart={history.onTouchStart}
        onTouchMove={history.onTouchMove}
        onKeyDown={history.onKeyDown}
        tabIndex={0}
      >
        <HistoryLoading loading={history.loading} error={history.error} retry={history.retry} />
        <UserMessageScope.Provider value={{ provider, sessionKey }}>
          <MarkdownPathScope.Provider value={props.cwd ?? ""}>
            <DisclosureScope.Provider value={`${prefix}:${sessionKey}`}>
              <DisclosureRevealContext.Provider
                value={pendingSearch?.sessionKey === sessionKey ? pendingSearch.positionKey : null}
              >
                {props.children}
              </DisclosureRevealContext.Provider>
            </DisclosureScope.Provider>
          </MarkdownPathScope.Provider>
        </UserMessageScope.Provider>
        <TimelineHistoryStatus navigation={{ ...navigation, busy: navigation.busy || history.loading }} />
        {props.isLoading && <div className="muted-row">正在读取消息...</div>}
        {!props.isLoading && props.isEmpty && <div className="muted-row">暂无对话</div>}
      </div>
    </div>
  );
}

const EVENT_KIND_LABELS: Record<string, string> = {
  user_message: "你",
  thinking: "思考",
  failure: "失败",
  compaction: "压缩摘要",
  branch_summary: "分支摘要",
};

export function StandardProviderEventView({
  event,
  activityId,
  title,
  providerLabel,
  timelineAliases = [],
  insideProcess = false,
}: {
  event: {
    kind: string;
    role?: string | null;
    text?: string | null;
    detail?: string | null;
    userMessage?: UserMessageContent | null;
  };
  activityId: string;
  title: string;
  providerLabel: string;
  timelineAliases?: string[];
  insideProcess?: boolean;
}) {
  const timelineProps = {
    "data-timeline-id": activityId,
    "data-timeline-aliases": timelineAliases.length ? timelineAliases.join(" ") : undefined,
  };
  if (event.kind === "user_message") {
    const aliases = [...timelineAliases, ...(event.userMessage?.id ? [event.userMessage.id] : [])];
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
        <MarkdownContent text={event.text ?? ""} activityId={activityId} foldInstructions={false} />
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
  if (insideProcess && event.kind === "thinking") {
    return (
      <div className="process-thought-step" {...timelineProps}>
        <MarkdownContent
          text={event.text ?? ""}
          activityId={activityId}
          timelineAliases={timelineAliases}
          foldInstructions={false}
        />
      </div>
    );
  }
  const isAssistant = event.kind.startsWith("assistant_message");
  const label =
    event.kind === "assistant_message"
      ? providerLabel
      : EVENT_KIND_LABELS[event.kind] ?? (event.role || providerLabel);
  return (
    <article
      className={`provider-event ${event.kind}${isAssistant ? " assistant-reply" : ""}`}
      {...timelineProps}
    >
      {!isAssistant && <div className="chat-meta">{label}</div>}
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

export function SessionDeleteConfirmDialog(props: {
  labelledBy: string;
  title: string;
  paths: string[];
  summaryText: string;
  busy: boolean;
  error?: Error | null;
  warnings?: string[];
  residuals?: string[];
  returnFocus?: RefObject<HTMLElement>;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  return (
    <ConfirmDialog
      labelledBy={props.labelledBy}
      busy={props.busy}
      onCancel={props.onCancel}
      returnFocus={props.returnFocus}
    >
      <h2 id={props.labelledBy}>删除任务文件</h2>
      <strong>{props.title}</strong>
      {props.paths.map(path => (
        <code className="delete-path" key={path}>{path}</code>
      ))}
      <p>{props.summaryText}</p>
      {props.error && <div role="alert" className="form-error">{props.error.message}</div>}
      {props.warnings?.map(warning => (
        <p className="form-error" key={warning}>{warning}</p>
      ))}
      {props.residuals?.map(path => (
        <p className="form-error" role="alert" key={path}>{path}</p>
      ))}
      <div className="button-row">
        <button className="secondary-button" disabled={props.busy} onClick={props.onCancel}>
          取消
        </button>
        <button className="danger-button" disabled={props.busy} onClick={props.onConfirm}>
          <Trash2 size={16} />
          {props.busy ? "正在删除..." : "确认删除任务文件"}
        </button>
      </div>
    </ConfirmDialog>
  );
}
