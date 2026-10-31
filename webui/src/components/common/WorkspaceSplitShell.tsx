import { Check, ChevronLeft, RefreshCw, Search, X } from "lucide-react";
import type { ReactNode } from "react";
import type { SessionStorageSize } from "../../types";
import { AgentBrandIcon, type AgentBrandId } from "./AgentBrandIcon";
import { RunningIndicator } from "./RunningIndicator";
import { SessionSize } from "./SessionSize";

export function WorkspaceSplitShell({
  layoutClass = "provider-workspace",
  sidebarClass = "provider-list",
  stageClass = "provider-detail",
  hasSelection = false,
  stageTag = "main",
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
  const Stage = stageTag;
  return (
    <div className={`ws-split-layout ${layoutClass}${hasSelection ? " has-selection" : ""}`}>
      <aside className={`ws-sidebar ${sidebarClass}`}>{sidebar}</aside>
      <Stage className={`ws-stage ${stageClass}`}>{children}</Stage>
    </div>
  );
}

export function WorkspaceListHeader({
  agent,
  title,
  count,
  onRefresh,
  refreshDisabled = false,
  actions,
}: {
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
        <span className="agent-brand-badge" data-agent={agent} aria-hidden="true">
          <AgentBrandIcon agent={agent} size={16} />
        </span>
        <h1>{title}</h1>
        {typeof count === "number" && (
          <span className="workspace-count-pill" aria-hidden="true">
            {count}
          </span>
        )}
      </div>
      <div className="thread-title-actions">
        {actions}
        <button
          className="icon-button compact"
          disabled={refreshDisabled}
          onClick={onRefresh}
          title="刷新任务"
        >
          <RefreshCw size={15} />
        </button>
      </div>
    </header>
  );
}

export function WorkspaceSearchInput({
  value,
  onChange,
  placeholder,
  disabled = false,
}: {
  value: string;
  onChange: (value: string) => void;
  placeholder: string;
  disabled?: boolean;
}) {
  return (
    <label className="search-box">
      <Search size={15} />
      <input
        value={value}
        disabled={disabled}
        onChange={event => onChange(event.target.value)}
        placeholder={placeholder}
      />
    </label>
  );
}

export function WorkspaceEmptyState({
  agent,
  title,
  subtitle,
}: {
  agent: AgentBrandId;
  title: string;
  subtitle: string;
}) {
  return (
    <div className="empty-state">
      <span className="empty-brand-badge" data-agent={agent} aria-hidden="true">
        <AgentBrandIcon agent={agent} size={26} />
      </span>
      <strong>{title}</strong>
      <small>{subtitle}</small>
    </div>
  );
}

export function WorkspaceStageHeader({
  title,
  subtitle,
  onBack,
  badges,
  actions,
}: {
  title: string;
  subtitle?: string | null;
  onBack: () => void;
  badges?: ReactNode;
  actions?: ReactNode;
}) {
  return (
    <header className="conversation-header" data-timeline-id="session">
      <button className="icon-button mobile-back" title="返回任务列表" onClick={onBack}>
        <ChevronLeft size={18} />
      </button>
      <div className="conversation-title-copy">
        <h1 className="conversation-title">{title}</h1>
        {subtitle && (
          <span className="conversation-subtitle" title={subtitle}>
            {subtitle}
          </span>
        )}
      </div>
      <div className="conversation-header-actions">
        {badges}
        {actions}
      </div>
    </header>
  );
}

export function WorkspaceInlineRename({
  ariaLabel,
  value,
  onChange,
  busy = false,
  onSubmit,
  onCancel,
}: {
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
        onSubmit();
      }}
    >
      <input
        aria-label={ariaLabel}
        maxLength={200}
        value={value}
        onChange={event => onChange(event.target.value)}
        autoFocus
      />
      <button
        className="icon-button"
        title="保存名称"
        disabled={busy || !value.trim()}
      >
        <Check size={17} />
      </button>
      <button
        className="icon-button"
        type="button"
        title="取消改名"
        onClick={onCancel}
      >
        <X size={17} />
      </button>
    </form>
  );
}

export function ProviderSessionRowBody({
  title,
  idText,
  cwd,
  running = false,
  statusText,
  storageSize,
}: {
  title: string;
  idText?: string | null;
  cwd?: string | null;
  running?: boolean;
  statusText: string;
  storageSize?: SessionStorageSize | null;
}) {
  return (
    <>
      <strong>{title}</strong>
      {idText && <span>{idText}</span>}
      {cwd && <span className="provider-session-path">{cwd}</span>}
      <span className="session-status-row">
        {running ? <RunningIndicator /> : <small>{statusText}</small>}
        <SessionSize size={storageSize} />
      </span>
    </>
  );
}
