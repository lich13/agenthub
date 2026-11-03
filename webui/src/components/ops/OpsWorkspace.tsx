import { Monitor, Moon, RefreshCw, SlidersHorizontal, Sun, TerminalSquare } from "lucide-react";
import { useState, type KeyboardEvent, type ReactNode } from "react";
import {
  OPS_PANEL_TITLES,
  opsWorkspaceView,
} from "../../lib/domain/runtimeViewModel";
import {
  APPEARANCE_OPTIONS,
  applyThemeMode,
  readStoredThemeMode,
  writeStoredThemeMode,
  type ThemeMode,
} from "../../lib/theme";
import { machineScope } from "../../lib/query/connection";
import { isTerminalJob, useStartedJob } from "../../lib/query/jobs";
import { useOpsActions, useOpsQueries } from "../../lib/query/ops";
import type { RuntimeCapabilityMatrix } from "../../lib/query/system";
import { Panel } from "../common/Panel";
import { JobList } from "../jobs/JobList";
import { CleanupPagination, UnifiedThreadCleanup } from "./UnifiedThreadCleanup";
import { UpdatePanel } from "./UpdatePanel";

export { CleanupPagination };

const APPEARANCE_ICONS: Record<ThemeMode, ReactNode> = {
  system: <Monitor size={16} />,
  dark: <Moon size={16} />,
  light: <Sun size={16} />,
};

export function OpsWorkspace({ capabilities }: { capabilities: RuntimeCapabilityMatrix }) {
  const [historyOpen, setHistoryOpen] = useState(false);
  const [themeMode, setThemeMode] = useState<ThemeMode>(() => readStoredThemeMode());
  const { update, jobs } = useOpsQueries({ historyOpen });
  const opsActions = useOpsActions({ capabilities });
  const jobMutation = opsActions.updateJob;
  const startedJob = useStartedJob(jobMutation.data?.job_id);
  const jobBusy =
    jobMutation.isPending ||
    Boolean(jobMutation.data?.job_id && !isTerminalJob(startedJob.data?.status));
  const opsView = opsWorkspaceView({
    update: update.data,
    capabilities,
  });

  const handleThemeChange = (next: ThemeMode) => {
    const normalized = writeStoredThemeMode(next);
    applyThemeMode(normalized);
    setThemeMode(normalized);
  };

  const handleAppearanceKeyDown = (event: KeyboardEvent<HTMLButtonElement>, index: number) => {
    let nextIndex = -1;
    if (event.key === "ArrowRight" || event.key === "ArrowDown") {
      nextIndex = (index + 1) % APPEARANCE_OPTIONS.length;
    } else if (event.key === "ArrowLeft" || event.key === "ArrowUp") {
      nextIndex = (index - 1 + APPEARANCE_OPTIONS.length) % APPEARANCE_OPTIONS.length;
    }
    if (nextIndex < 0) return;
    event.preventDefault();
    const nextOption = APPEARANCE_OPTIONS[nextIndex];
    handleThemeChange(nextOption.id);
    const buttons = event.currentTarget.parentElement?.querySelectorAll<HTMLButtonElement>('[role="radio"]');
    buttons?.[nextIndex]?.focus();
  };

  return (
    <div className="ops-grid ws-control-stage" data-update-actions={opsView.updateActions.length}>
      <header className="ops-header probe-header">
        <div className="workspace-title-brand">
          <SlidersHorizontal size={18} />
          <h1>设置与维护</h1>
        </div>
        <div className="button-row">
          <button
            className="icon-button"
            title="刷新版本与状态"
            disabled={update.isFetching}
            onClick={() => {
              void update.refetch();
              if (historyOpen) void jobs.refetch();
            }}
          >
            <RefreshCw size={17} />
          </button>
        </div>
      </header>
      <section className="appearance-section">
        <strong className="appearance-heading">外观</strong>
        <div className="appearance-segmented" role="radiogroup" aria-label="外观">
          {APPEARANCE_OPTIONS.map((option, index) => (
            <button
              key={option.id}
              type="button"
              role="radio"
              tabIndex={themeMode === option.id ? 0 : -1}
              aria-checked={themeMode === option.id}
              className={`appearance-option ${themeMode === option.id ? "is-active" : ""}`}
              onClick={() => handleThemeChange(option.id)}
              onKeyDown={event => handleAppearanceKeyDown(event, index)}
            >
              {APPEARANCE_ICONS[option.id]}
              <span>{option.label}</span>
            </button>
          ))}
        </div>
      </section>
      <UpdatePanel
        capabilities={capabilities}
        status={update.data}
        loading={update.isPending}
        action={jobMutation.variables?.action}
        busy={jobBusy}
        finished={jobMutation.isSuccess && startedJob.data?.status === "succeeded"}
        job={startedJob.data}
        error={update.error?.message ?? jobMutation.error?.message ?? startedJob.error?.message}
        onAction={action => jobMutation.mutate({ action })}
      />
      <UnifiedThreadCleanup key={machineScope()} capabilities={capabilities} />
      <details
        className="execution-history"
        open={historyOpen}
        onToggle={event => setHistoryOpen(event.currentTarget.open)}
      >
        <summary>
          <span>执行记录</span>
          {jobs.data && jobs.data.length > 0 && (
            <span className="workspace-count-pill">{jobs.data.length}</span>
          )}
        </summary>
        <Panel
          title={OPS_PANEL_TITLES.jobs}
          icon={<TerminalSquare size={18} />}
          className="wide-panel"
        >
          {jobs.error && (
            <div role="alert" className="form-error">
              {jobs.error.message}
            </div>
          )}
          <JobList jobs={jobs.data ?? []} capabilities={capabilities} />
        </Panel>
      </details>
    </div>
  );
}
