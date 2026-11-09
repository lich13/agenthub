import { CheckCircle2, ChevronDown, ExternalLink, Github, RefreshCw } from "lucide-react";
import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { getGithubCiStatus } from "../../lib/api";
import { visiblePollInterval } from "../../lib/query/shared";
import type { GithubCiRun } from "../../types";
import type { ProbeSettingsDraft } from "../../lib/probeUi";
import { Panel } from "../common/Panel";
import type { ProbeSaveStatus } from "./ProbeCards";

const queryKey = ["github-ci"] as const;

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

export function visibleCiRuns(runs: GithubCiRun[], repository: string, filterBots = false) {
  const normalizedRepo = repository.trim().toLowerCase();
  return runs
    .filter(
      run =>
        (!filterBots || !run.is_bot) &&
        (!normalizedRepo ||
          String(run.repository_id) === normalizedRepo ||
          run.repository_name.toLowerCase() === normalizedRepo),
    )
    .slice(0, 20);
}

export function GithubCiCard({ draft, setDraft, saving, saveStatus, onSave }: {
  draft: ProbeSettingsDraft | null;
  setDraft: (update: (current: ProbeSettingsDraft | null) => ProbeSettingsDraft | null) => void;
  saving: boolean;
  saveStatus: ProbeSaveStatus;
  onSave: () => void;
}) {
  const [repository, setRepository] = useState("");
  const [localFilterBots, setLocalFilterBots] = useState(true);
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
  const configuredRepository = (draft ? draft.notifications.github_ci_repositories[0] ?? "" : repository).trim();
  const matchedRepository = data?.repositories.find(
    repo =>
      String(repo.id) === configuredRepository ||
      repo.full_name.toLowerCase() === configuredRepository.toLowerCase(),
  );
  const selectedRepository = matchedRepository ? String(matchedRepository.id) : "";
  const activeRepositoryFilter = draft ? selectedRepository || configuredRepository : selectedRepository;
  const filterBots = draft ? draft.notifications.github_ci_filter_bots : localFilterBots;
  const runs = visibleCiRuns(data?.runs ?? [], activeRepositoryFilter, filterBots);
  const busy = refresh.isPending || data?.refreshing || query.isLoading;
  const error = refresh.error?.message ?? query.error?.message ?? data?.error;

  return (
    <Panel title="GitHub CI" icon={<Github size={18} />} className="wide-panel">
      <div className="github-ci-toolbar">
        <div className="github-ci-account" title={data?.account?.login}>
          {data?.account ? `@${data.account.login}` : busy ? "读取账号中" : "未登录"}
        </div>
        <span className="github-ci-count">{runs.length} 次运行</span>
        <label className="toggle-row github-ci-toggle">
          <span>监控</span>
          <input type="checkbox" checked={draft?.notifications.notify_github_ci ?? false} disabled={!draft || saving}
            onChange={event => {
              const enabled = event.target.checked;
              setDraft(current => current ? {
                ...current,
                notifications: { ...current.notifications, notify_github_ci: enabled },
              } : current);
            }} />
        </label>
        <button
          type="button"
          className="secondary-button"
          aria-pressed={filterBots}
          disabled={saving}
          onClick={() => {
            const next = !filterBots;
            setLocalFilterBots(next);
            setDraft(current => current ? {
              ...current,
              notifications: { ...current.notifications, github_ci_filter_bots: next },
            } : current);
          }}
        >
          过滤 Bot
        </button>
        <button className="primary-button" disabled={!draft || saving} onClick={onSave}>
          <CheckCircle2 size={16} />{saving ? "保存中" : "保存"}
        </button>
        <button className="icon-button" title="刷新 GitHub CI" aria-label="刷新 GitHub CI"
          disabled={busy} onClick={() => refresh.mutate()}>
          <RefreshCw size={17} className={busy ? "spinning" : undefined} />
        </button>
      </div>
      <div className="github-ci-filter field-label">
        <select
          aria-label="筛选 CI 仓库"
          value={selectedRepository}
          onChange={event => {
            const value = event.target.value;
            setRepository(value);
            const repo = data?.repositories.find(item => String(item.id) === value);
            const nextRepositories = value ? [repo?.full_name ?? value] : [];
            setDraft(current => current ? {
              ...current,
              notifications: { ...current.notifications, github_ci_repositories: nextRepositories },
            } : current);
          }}
        >
          <option value="">全部仓库</option>
          {data?.repositories.map(repo => <option key={repo.id} value={String(repo.id)}>{repo.full_name}</option>)}
        </select>
        <ChevronDown size={16} aria-hidden="true" />
      </div>
      {error && <div className="form-error" role="alert">{error}</div>}
      {saveStatus && <div className={saveStatus.tone === "success" ? "form-success" : "form-error"}>{saveStatus.message}</div>}
      <div className="github-ci-runs" aria-label="CI 运行列表">
        {runs.map(run => {
          const status = ciRunStatus(run);
          return (
            <div className="github-ci-run" key={`${run.repository_id}:${run.id}:${run.run_attempt}`}>
              <div className="github-ci-run-main">
                <a href={run.html_url} target="_blank" rel="noopener noreferrer" title={run.name ?? "工作流"}>
                  {run.name ?? "工作流"}<ExternalLink size={14} />
                </a>
                <span className={`status-chip tone-${status.tone}`}>{status.label}</span>
              </div>
              <div className="github-ci-run-meta">
                <span title={run.repository_name}>{run.repository_name}</span>
                <span title={run.head_branch ?? ""}>{run.head_branch ?? "—"}</span>
                <span>{run.head_sha.slice(0, 7)}</span>
                <time dateTime={run.updated_at}>
                  {new Date(run.updated_at).toLocaleString(undefined, {
                    month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit",
                  })}
                </time>
              </div>
            </div>
          );
        })}
        {!runs.length && <div className="empty-state compact">{busy ? "正在读取 CI" : "暂无工作流运行"}</div>}
      </div>
    </Panel>
  );
}
