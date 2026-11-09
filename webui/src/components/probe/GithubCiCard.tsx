import {
  Bell,
  Bot,
  CheckCircle2,
  ExternalLink,
  FolderGit2,
  GitBranch,
  Github,
  Plus,
  RefreshCw,
  Search,
} from "lucide-react";
import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { getGithubCiStatus } from "../../lib/api";
import { visiblePollInterval } from "../../lib/query/shared";
import type { GithubCiRepository, GithubCiRun } from "../../types";
import type { ProbeSettingsDraft } from "../../lib/probeUi";
import { Panel } from "../common/Panel";
import type { ProbeSaveStatus } from "./ProbeCards";

const queryKey = ["github-ci"] as const;

export type GithubCiRepoOption = {
  key: string;
  id: string;
  fullName: string;
};

export function ciRunStatus(run: GithubCiRun) {
  const state = run.status === "completed" ? run.conclusion ?? "completed" : run.status;
  const labels: Record<string, string> = {
    success: "成功", failure: "失败", timed_out: "超时", cancelled: "已取消", skipped: "已跳过",
    in_progress: "运行中", queued: "排队中", waiting: "等待中", requested: "等待中", pending: "等待中",
    action_required: "待处理", startup_failure: "启动失败", neutral: "已完成", completed: "已完成", stale: "已过期",
  };
  const tone = state === "success" ? "success"
    : ["failure", "timed_out", "startup_failure"].includes(state) ? "danger"
    : run.status !== "completed" ? "warning" : "neutral";
  return { label: labels[state] ?? state, tone };
}

export function visibleCiRuns(
  runs: GithubCiRun[],
  repositories: string | readonly string[],
  filterBots = false,
) {
  const list = typeof repositories === "string" ? [repositories] : repositories;
  const normalized = list.map(item => item.trim().toLowerCase()).filter(Boolean);
  return runs
    .filter(
      run =>
        (!filterBots || !run.is_bot) &&
        (normalized.length === 0 ||
          normalized.some(
            repo =>
              String(run.repository_id).toLowerCase() === repo ||
              run.repository_name.toLowerCase() === repo,
          )),
    )
    .slice(0, 20);
}

export function buildRepositoryCatalog(
  repositories: readonly GithubCiRepository[] = [],
  runs: readonly GithubCiRun[] = [],
  configured: readonly string[] = [],
): GithubCiRepoOption[] {
  const catalog: GithubCiRepoOption[] = [];
  const seenNames = new Set<string>();
  const seenIds = new Set<string>();
  const pushOption = (id: string, fullName: string) => {
    const trimmedName = fullName.trim();
    const trimmedId = id.trim();
    if (!trimmedName) return;
    const lowerName = trimmedName.toLowerCase();
    const lowerId = trimmedId.toLowerCase();
    if (seenNames.has(lowerName) || (lowerId && seenIds.has(lowerId))) return;
    seenNames.add(lowerName);
    if (lowerId) seenIds.add(lowerId);
    catalog.push({ key: trimmedName, id: trimmedId || trimmedName, fullName: trimmedName });
  };
  for (const repo of repositories) {
    if (!repo.archived) {
      pushOption(String(repo.id), repo.full_name);
    }
  }
  for (const run of runs) {
    pushOption(String(run.repository_id), run.repository_name);
  }
  for (const entry of configured) {
    const trimmed = entry.trim();
    if (trimmed) {
      pushOption(trimmed, trimmed);
    }
  }
  return catalog;
}

export function isRepositorySelected(
  option: GithubCiRepoOption,
  configured: readonly string[],
): boolean {
  const lowerName = option.fullName.toLowerCase();
  const lowerId = option.id.toLowerCase();
  return configured.some(item => {
    const trimmed = item.trim().toLowerCase();
    return Boolean(trimmed) && (trimmed === lowerName || trimmed === lowerId);
  });
}

export function toggleRepositorySelection(
  option: GithubCiRepoOption,
  configured: readonly string[],
): string[] {
  const lowerName = option.fullName.toLowerCase();
  const lowerId = option.id.toLowerCase();
  if (isRepositorySelected(option, configured)) {
    return configured.filter(item => {
      const trimmed = item.trim().toLowerCase();
      return Boolean(trimmed) && trimmed !== lowerName && trimmed !== lowerId;
    });
  }
  return [...configured.map(item => item.trim()).filter(Boolean), option.fullName];
}

export function GithubCiCard({ draft, setDraft, saving, saveStatus, onSave }: {
  draft: ProbeSettingsDraft | null;
  setDraft: (update: (current: ProbeSettingsDraft | null) => ProbeSettingsDraft | null) => void;
  saving: boolean;
  saveStatus: ProbeSaveStatus;
  onSave: () => void;
}) {
  const [localRepositories, setLocalRepositories] = useState<string[]>([]);
  const [localFilterBots, setLocalFilterBots] = useState(true);
  const [repoQuery, setRepoQuery] = useState("");
  const qc = useQueryClient();
  const query = useQuery({
    queryKey,
    queryFn: () => getGithubCiStatus(),
    refetchInterval: query => visiblePollInterval(query.state.data?.refreshing ? 2000 : 15000)(),
  });
  const refresh = useMutation({
    mutationFn: () => getGithubCiStatus(true),
    onSuccess: data => qc.setQueryData(queryKey, data),
  });
  const data = query.data;
  const notifyEnabled = draft?.notifications.notify_github_ci ?? false;
  const filterBots = draft ? draft.notifications.github_ci_filter_bots : localFilterBots;
  const configuredRepositories = draft
    ? draft.notifications.github_ci_repositories
    : localRepositories;
  const catalog = buildRepositoryCatalog(data?.repositories, data?.runs, configuredRepositories);
  const activeFilterTokens = configuredRepositories.flatMap(entry => {
    const trimmed = entry.trim();
    if (!trimmed) return [];
    const matched = catalog.find(
      opt =>
        opt.fullName.toLowerCase() === trimmed.toLowerCase() ||
        opt.id.toLowerCase() === trimmed.toLowerCase(),
    );
    return matched ? [matched.fullName, matched.id] : [trimmed];
  });
  const runs = visibleCiRuns(data?.runs ?? [], activeFilterTokens, filterBots);
  const busy = refresh.isPending || data?.refreshing || query.isLoading;
  const error = refresh.error?.message ?? query.error?.message ?? data?.error;
  const normalizedQuery = repoQuery.trim().toLowerCase();
  const filteredCatalog = normalizedQuery
    ? catalog.filter(opt => opt.fullName.toLowerCase().includes(normalizedQuery))
    : catalog;
  const selectedCount =
    catalog.filter(opt => isRepositorySelected(opt, configuredRepositories)).length ||
    configuredRepositories.length;

  const updateRepositories = (nextRepositories: string[]) => {
    setLocalRepositories(nextRepositories);
    setDraft(current =>
      current
        ? {
            ...current,
            notifications: {
              ...current.notifications,
              github_ci_repositories: nextRepositories,
            },
          }
        : current,
    );
  };

  const handleAddRepository = () => {
    const trimmed = repoQuery.trim();
    if (!trimmed) return;
    const matched = catalog.find(
      opt =>
        opt.fullName.toLowerCase() === trimmed.toLowerCase() ||
        opt.id.toLowerCase() === trimmed.toLowerCase(),
    );
    const targetName = matched ? matched.fullName : trimmed;
    const alreadySelected = configuredRepositories.some(item => {
      const lower = item.trim().toLowerCase();
      return (
        lower === targetName.toLowerCase() ||
        Boolean(matched && lower === matched.id.toLowerCase())
      );
    });
    if (!alreadySelected) {
      updateRepositories([...configuredRepositories, targetName]);
    }
    setRepoQuery("");
  };

  const headerAction = (
    <div className="github-ci-header-actions">
      <span
        className={`status-chip github-ci-account tone-${data?.account ? "info" : "neutral"}`}
        title={data?.account?.login}
      >
        {data?.account
          ? `@${data.account.login}`
          : busy
            ? "读取账号中"
            : !notifyEnabled && !error
              ? "待启用扫描"
              : "未登录"}
      </span>
      <span className="status-chip tone-neutral github-ci-count">{runs.length} 次运行</span>
      <button
        type="button"
        className="icon-button"
        title="刷新 GitHub CI"
        aria-label="刷新 GitHub CI"
        disabled={busy}
        onClick={() => refresh.mutate()}
      >
        <RefreshCw size={16} className={busy ? "spinning" : undefined} />
      </button>
      <button
        type="button"
        className="primary-button github-ci-save-button"
        disabled={!draft || saving}
        onClick={onSave}
      >
        <CheckCircle2 size={16} />
        {saving ? "保存中" : "保存"}
      </button>
    </div>
  );

  return (
    <Panel
      title="GitHub CI"
      icon={<Github size={18} />}
      action={headerAction}
      className="wide-panel github-ci-panel"
    >
      <div className="github-ci-switches">
        <button
          type="button"
          className={`github-ci-switch-card ${notifyEnabled ? "is-enabled" : ""}`}
          aria-pressed={notifyEnabled}
          disabled={!draft || saving}
          onClick={() => {
            const next = !notifyEnabled;
            setDraft(current =>
              current
                ? {
                    ...current,
                    notifications: { ...current.notifications, notify_github_ci: next },
                  }
                : current,
            );
          }}
        >
          <span className="github-ci-switch-head">
            <span className="github-ci-switch-icon" aria-hidden="true">
              <Bell size={16} />
            </span>
            <strong>CI 监控通知</strong>
          </span>
          <span className="github-ci-switch-state">
            <span className={`status-chip tone-${notifyEnabled ? "success" : "neutral"}`}>
              {notifyEnabled ? "已开启" : "已关闭"}
            </span>
            <span className="github-ci-switch-track" aria-hidden="true">
              <span className="github-ci-switch-thumb" />
            </span>
          </span>
        </button>

        <button
          type="button"
          className={`github-ci-switch-card ${filterBots ? "is-enabled" : ""}`}
          aria-pressed={filterBots}
          disabled={saving}
          onClick={() => {
            const next = !filterBots;
            setLocalFilterBots(next);
            setDraft(current =>
              current
                ? {
                    ...current,
                    notifications: { ...current.notifications, github_ci_filter_bots: next },
                  }
                : current,
            );
          }}
        >
          <span className="github-ci-switch-head">
            <span className="github-ci-switch-icon" aria-hidden="true">
              <Bot size={16} />
            </span>
            <strong>过滤 Bot</strong>
          </span>
          <span className="github-ci-switch-state">
            <span className={`status-chip tone-${filterBots ? "info" : "neutral"}`}>
              {filterBots ? "已开启" : "已关闭"}
            </span>
            <span className="github-ci-switch-track" aria-hidden="true">
              <span className="github-ci-switch-thumb" />
            </span>
          </span>
        </button>
      </div>

      <div className="github-ci-repo-section" role="group" aria-label="筛选 CI 仓库">
        <div className="github-ci-repo-header">
          <div className="github-ci-repo-heading">
            <FolderGit2 size={15} aria-hidden="true" />
            <strong>监控仓库范围</strong>
            <span
              className={`status-chip tone-${configuredRepositories.length > 0 ? "info" : "neutral"}`}
            >
              {configuredRepositories.length > 0 ? `只监控 ${selectedCount} 个仓库` : "监控全部仓库"}
            </span>
          </div>
          <div className="github-ci-repo-controls">
            <div className="github-ci-repo-search">
              <Search size={14} aria-hidden="true" />
              <input
                type="search"
                aria-label="搜索或添加监控仓库"
                placeholder="搜索或输入 owner/repo"
                value={repoQuery}
                disabled={saving}
                onChange={event => setRepoQuery(event.target.value)}
                onKeyDown={event => {
                  if (event.key === "Enter") {
                    event.preventDefault();
                    handleAddRepository();
                  }
                }}
              />
              {repoQuery.trim() && (
                <button
                  type="button"
                  className="secondary-button github-ci-repo-add"
                  disabled={saving}
                  onClick={handleAddRepository}
                >
                  <Plus size={14} />
                  添加
                </button>
              )}
            </div>
          </div>
        </div>

        <div className="github-ci-repo-grid">
          <button
            type="button"
            className={`github-ci-repo-chip is-all ${configuredRepositories.length === 0 ? "is-selected" : ""}`}
            aria-pressed={configuredRepositories.length === 0}
            disabled={saving}
            onClick={() => updateRepositories([])}
          >
            <span className="github-ci-repo-indicator" aria-hidden="true">
              {configuredRepositories.length === 0 ? "✓" : "○"}
            </span>
            <span className="github-ci-repo-name">全部仓库</span>
          </button>

          {filteredCatalog.map(option => {
            const selected = isRepositorySelected(option, configuredRepositories);
            return (
              <button
                key={option.key}
                type="button"
                className={`github-ci-repo-chip ${selected ? "is-selected" : ""}`}
                aria-pressed={selected}
                disabled={saving}
                onClick={() =>
                  updateRepositories(toggleRepositorySelection(option, configuredRepositories))
                }
                title={option.fullName}
              >
                <span className="github-ci-repo-indicator" aria-hidden="true">
                  {selected ? "✓" : "+"}
                </span>
                <span className="github-ci-repo-name">{option.fullName}</span>
              </button>
            );
          })}
        </div>
      </div>

      {error && <div className="form-error" role="alert">{error}</div>}
      {saveStatus && (
        <div className={saveStatus.tone === "success" ? "form-success" : "form-error"}>
          {saveStatus.message}
        </div>
      )}

      <div className="github-ci-runs" aria-label="CI 运行列表">
        {runs.map(run => {
          const status = ciRunStatus(run);
          return (
            <div
              className="github-ci-run"
              key={`${run.repository_id}:${run.id}:${run.run_attempt}`}
            >
              <div className="github-ci-run-main">
                <div className="github-ci-run-title">
                  <span
                    className={`github-ci-status-dot tone-${status.tone}`}
                    aria-hidden="true"
                  />
                  <a
                    href={run.html_url}
                    target="_blank"
                    rel="noopener noreferrer"
                    title={run.name ?? "工作流"}
                  >
                    <span>{run.name ?? "工作流"}</span>
                    <ExternalLink size={13} />
                  </a>
                  {run.is_bot && (
                    <span className="status-chip tone-neutral github-ci-bot-badge">Bot</span>
                  )}
                </div>
                <span className={`status-chip tone-${status.tone}`}>{status.label}</span>
              </div>
              <div className="github-ci-run-meta">
                <span className="github-ci-meta-repo" title={run.repository_name}>
                  {run.repository_name}
                </span>
                <span className="github-ci-meta-branch" title={run.head_branch ?? ""}>
                  <GitBranch size={12} aria-hidden="true" />
                  {run.head_branch ?? "—"}
                </span>
                <code className="github-ci-meta-sha">{run.head_sha.slice(0, 7)}</code>
                <time dateTime={run.updated_at}>
                  {new Date(run.updated_at).toLocaleString(undefined, {
                    month: "2-digit",
                    day: "2-digit",
                    hour: "2-digit",
                    minute: "2-digit",
                  })}
                </time>
              </div>
            </div>
          );
        })}
        {!runs.length && (
          <div className="empty-state compact">
            {busy ? "正在读取 CI" : "暂无工作流运行"}
          </div>
        )}
      </div>
    </Panel>
  );
}
