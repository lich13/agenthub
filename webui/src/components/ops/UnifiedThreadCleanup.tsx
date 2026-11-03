import { ChevronDown, ChevronRight, Database, Search, Sparkles, Trash2 } from "lucide-react";
import { useState } from "react";
import {
  OPS_PANEL_TITLES,
  UNIFIED_CLEANUP_SCOPES,
  canStartHiddenThreadDelete,
  collectUnifiedCleanupTargets,
  formatCleanupBytes,
  mergeHiddenScanIntoUnifiedPlan,
  unifiedCleanupStageView,
  unifiedCleanupUnsuccessfulItems,
  unifiedPlanAfterExecute,
} from "../../lib/domain/runtimeViewModel";
import { useUnifiedCleanupActions } from "../../lib/query/ops";
import type { RuntimeCapabilityMatrix } from "../../lib/query/system";
import type {
  HiddenScanProgress,
  UnifiedCleanupPlan,
  UnifiedCleanupResult,
  UnifiedCleanupScopePlan,
} from "../../types";
import { AgentBrandIcon } from "../common/AgentBrandIcon";
import { Metric, Panel } from "../common/Panel";

export function CleanupPagination(props: {
  page: number;
  count: number;
  onChange: (page: number) => void;
  label: string;
}) {
  const pages = Math.ceil(props.count / 100);
  return (
    <nav className="button-row cleanup-pagination" aria-label={`${props.label}分页`}>
      <button className="secondary-button" disabled={props.page === 0} onClick={() => props.onChange(props.page - 1)}>
        上一页
      </button>
      <span>
        {props.page + 1} / {pages}
      </span>
      <button
        className="secondary-button"
        disabled={props.page + 1 >= pages}
        onClick={() => props.onChange(props.page + 1)}
      >
        下一页
      </button>
    </nav>
  );
}

function cleanupErrorMessage(prefix: string, error: unknown): string {
  return `${prefix}: ${error instanceof Error ? error.message : String(error)}`;
}

function defaultSelectedScopes(plan: UnifiedCleanupPlan): string[] {
  return plan.scopes.filter(s => s.supported && s.scanned && s.deletableCount > 0).map(s => s.scope);
}

function firstScopeWithCandidates(plan: UnifiedCleanupPlan): string | null {
  return plan.scopes.find(scope => scope.candidates.length > 0)?.scope ?? null;
}

export function UnifiedThreadCleanup({ capabilities }: { capabilities: RuntimeCapabilityMatrix }) {
  const [plan, setPlan] = useState<UnifiedCleanupPlan | null>(null);
  const [hiddenScan, setHiddenScan] = useState<HiddenScanProgress | null>(null);
  const [result, setResult] = useState<UnifiedCleanupResult | null>(null);
  const [selectedScopes, setSelectedScopes] = useState<string[]>([]);
  const [armed, setArmed] = useState(false);
  const [expandedScope, setExpandedScope] = useState<string | null>(null);
  const [candidatePage, setCandidatePage] = useState(0);
  const [resultPage, setResultPage] = useState(0);

  const actions = useUnifiedCleanupActions({
    capabilities,
    onUnifiedDryRun: nextPlan => {
      setPlan(previous => {
        if (previous && !nextPlan.scopes.some(s => s.scope === "codex_hidden" && s.scanned)) {
          const prevHidden = previous.scopes.find(s => s.scope === "codex_hidden" && s.scanned);
          if (prevHidden) {
            const scopes = nextPlan.scopes.map(s => (s.scope === "codex_hidden" ? prevHidden : s));
            const merged: UnifiedCleanupPlan = {
              totalCandidates: scopes.reduce((sum, item) => sum + item.candidateCount, 0),
              totalDeletable: scopes.reduce((sum, item) => sum + item.deletableCount, 0),
              totalEstimatedBytes: scopes.reduce((sum, item) => sum + item.estimatedBytes, 0),
              scopes,
            };
            setSelectedScopes(defaultSelectedScopes(merged));
            setExpandedScope(current => current ?? firstScopeWithCandidates(merged));
            return merged;
          }
        }
        setSelectedScopes(defaultSelectedScopes(nextPlan));
        setExpandedScope(firstScopeWithCandidates(nextPlan));
        return nextPlan;
      });
      setResult(null);
      setArmed(false);
      setCandidatePage(0);
      setResultPage(0);
    },
    onHiddenDryRun: hiddenPlan => {
      setHiddenScan(hiddenPlan.scan ?? null);
      setResult(null);
      setArmed(false);
      setPlan(current => {
        const merged = mergeHiddenScanIntoUnifiedPlan(current, hiddenPlan);
        if (canStartHiddenThreadDelete(hiddenPlan)) {
          const hiddenScope = merged.scopes.find(item => item.scope === "codex_hidden");
          if (hiddenScope && hiddenScope.deletableCount > 0) {
            setSelectedScopes(prev => (prev.includes("codex_hidden") ? prev : [...prev, "codex_hidden"]));
            setExpandedScope(prev => prev ?? "codex_hidden");
          }
        } else {
          setSelectedScopes(prev => prev.filter(scope => scope !== "codex_hidden"));
        }
        return merged;
      });
    },
    onUnifiedExecute: nextResult => {
      setArmed(false);
      setResult(nextResult);
      setResultPage(0);
      setCandidatePage(0);
      setPlan(current => {
        const updated = unifiedPlanAfterExecute(current, nextResult);
        if (updated) {
          setSelectedScopes(defaultSelectedScopes(updated));
          setExpandedScope(firstScopeWithCandidates(updated));
        }
        return updated;
      });
    },
  });

  if (!capabilities.threadCleanup) return null;

  const { unifiedDryRun, hiddenDryRun, unifiedExecute } = actions;
  const dryRunPending = unifiedDryRun.isPending || hiddenDryRun.isPending;
  const busy = dryRunPending || unifiedExecute.isPending;

  const scopeMap = new Map<string, UnifiedCleanupScopePlan>((plan?.scopes ?? []).map(s => [s.scope, s]));
  const availableScopesCount = UNIFIED_CLEANUP_SCOPES.filter(
    meta => scopeMap.get(meta.scope)?.supported !== false,
  ).length;
  const activeSelectedScopes = selectedScopes.filter(scope => {
    const item = scopeMap.get(scope);
    return Boolean(item && item.supported && item.scanned && item.deletableCount > 0);
  });
  const selectedDeletableCount = activeSelectedScopes.reduce(
    (sum, scope) => sum + (scopeMap.get(scope)?.deletableCount ?? 0),
    0,
  );
  const selectedEstimatedBytes = activeSelectedScopes.reduce(
    (sum, scope) => sum + (scopeMap.get(scope)?.estimatedBytes ?? 0),
    0,
  );
  const selectedTargets = collectUnifiedCleanupTargets(plan, activeSelectedScopes);
  const stage = unifiedCleanupStageView({
    hasPlan: Boolean(plan),
    dryRunPending,
    armed,
    executePending: unifiedExecute.isPending,
    executableCount: selectedDeletableCount,
    hiddenScan,
  });

  const canArm = Boolean(plan) && !busy && selectedDeletableCount > 0 && activeSelectedScopes.length > 0;
  const activeExpandedScope = expandedScope ? scopeMap.get(expandedScope) ?? null : null;
  const visibleCandidates =
    activeExpandedScope?.candidates.slice(candidatePage * 100, (candidatePage + 1) * 100) ?? [];
  const unsuccessfulItems = unifiedCleanupUnsuccessfulItems(result);
  const visibleUnsuccessful = unsuccessfulItems.slice(resultPage * 100, (resultPage + 1) * 100);
  const unifiedCleanupError = unifiedExecute.error
    ? cleanupErrorMessage("全平台清理失败", unifiedExecute.error)
    : unifiedDryRun.error
      ? cleanupErrorMessage("全平台扫描失败", unifiedDryRun.error)
      : null;
  const hiddenCleanupError = hiddenDryRun.error
    ? cleanupErrorMessage("隐藏线程扫描失败", hiddenDryRun.error)
    : null;
  const cleanupError = unifiedCleanupError ?? hiddenCleanupError;

  const resetPreviewState = () => {
    unifiedExecute.reset();
    setResult(null);
    setArmed(false);
    setCandidatePage(0);
    setResultPage(0);
  };
  const runFullPreview = () => {
    resetPreviewState();
    unifiedDryRun.reset();
    hiddenDryRun.reset();
    setHiddenScan(null);
    unifiedDryRun.mutate(undefined);
    if (capabilities.hiddenCleanupScan === true) hiddenDryRun.mutate();
  };
  const runHiddenPreview = () => {
    resetPreviewState();
    hiddenDryRun.reset();
    hiddenDryRun.mutate();
  };

  return (
    <Panel
      title={OPS_PANEL_TITLES.unifiedCleanup}
      icon={<Sparkles size={18} />}
      className="wide-panel unified-cleanup-panel"
    >
      <div className="cleanup-panel-head unified-cleanup-head">
        <span role="status" className={`status-chip ${stage.tone ? `tone-${stage.tone}` : "tone-muted"}`}>
          {stage.label}
        </span>
      </div>

      <div className="archive-plan unified-cleanup-metrics">
        <Metric
          label="可清理会话"
          value={plan ? `${selectedDeletableCount} / ${plan.totalDeletable}` : "待扫描预览"}
          tone={selectedDeletableCount > 0 ? "warning" : undefined}
        />
        <Metric
          label="预计释放空间"
          value={plan ? formatCleanupBytes(selectedEstimatedBytes) : "待扫描预览"}
        />
        <Metric
          label="已选平台范围"
          value={plan ? `${activeSelectedScopes.length} / ${availableScopesCount}` : "待扫描预览"}
        />
      </div>

      <div className="unified-cleanup-grid">
        {UNIFIED_CLEANUP_SCOPES.map(meta => {
          const scopePlan = scopeMap.get(meta.scope);
          const supported = scopePlan?.supported !== false;
          const scanned = Boolean(scopePlan?.scanned);
          const deletable = scopePlan?.deletableCount ?? 0;
          const candidatesCount = scopePlan?.candidateCount ?? 0;
          const bytes = scopePlan?.estimatedBytes ?? 0;
          const canSelect = Boolean(plan && supported && scanned && deletable > 0);
          const isSelected = activeSelectedScopes.includes(meta.scope);
          const isExpanded = expandedScope === meta.scope && candidatesCount > 0;
          const barPct =
            plan && plan.totalEstimatedBytes > 0
              ? Math.max(6, Math.min(100, Math.round((bytes / plan.totalEstimatedBytes) * 100)))
              : 0;
          const deletableSummary =
            deletable > 0
              ? `${deletable} 项可清理${bytes > 0 ? ` · ${formatCleanupBytes(bytes)}` : ""}`
              : "无可清理项";
          const skipSuffix =
            candidatesCount > deletable ? `（${candidatesCount - deletable} 项占用跳过）` : "";
          const statusMeta = !supported
            ? (scopePlan?.unavailableReason ?? "当前运行环境不支持")
            : !plan
              ? "点击下方全平台扫描预览"
              : !scanned
                ? meta.scope === "codex_hidden" && hiddenDryRun.isPending
                  ? "正在深度扫描隐藏线程…"
                  : "未深度扫描"
                : `${deletableSummary}${skipSuffix}`;

          return (
            <div
              key={meta.scope}
              className={`unified-scope-card${isSelected ? " is-selected" : ""}${!supported ? " is-unsupported" : ""}`}
              data-agent={meta.provider}
            >
              <div className="unified-scope-top">
                <span className="agent-brand-badge" data-agent={meta.provider} aria-hidden="true">
                  <AgentBrandIcon agent={meta.provider} size={17} />
                </span>
                <div className="unified-scope-copy">
                  <div className="unified-scope-title-row">
                    <strong>{meta.agentTitle}</strong>
                    <span className="unified-scope-badge">{meta.scopeBadge}</span>
                  </div>
                  <small>{meta.description}</small>
                </div>
                <input
                  type="checkbox"
                  className="unified-scope-checkbox"
                  aria-label={`选择 ${meta.label}`}
                  checked={isSelected}
                  disabled={!canSelect || busy}
                  onChange={() => {
                    setArmed(false);
                    setSelectedScopes(cur =>
                      cur.includes(meta.scope) ? cur.filter(s => s !== meta.scope) : [...cur, meta.scope],
                    );
                  }}
                />
              </div>

              {plan && scanned && deletable > 0 && plan.totalEstimatedBytes > 0 && (
                <div className="unified-scope-bar" aria-hidden="true">
                  <span style={{ width: `${barPct}%` }} />
                </div>
              )}

              <div className="unified-scope-foot">
                <span className={`unified-scope-meta${scanned && deletable > 0 ? " has-items" : ""}`}>
                  {statusMeta}
                </span>
                {candidatesCount > 0 && (
                  <button
                    type="button"
                    className="unified-scope-detail-toggle"
                    aria-expanded={isExpanded}
                    onClick={() => {
                      setCandidatePage(0);
                      setExpandedScope(isExpanded ? null : meta.scope);
                    }}
                  >
                    {isExpanded ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
                    <span>{isExpanded ? "收起明细" : `明细 (${candidatesCount})`}</span>
                  </button>
                )}
              </div>

              {scopePlan?.issues.map(issue => (
                <p key={issue} className="unified-scope-issue">
                  {issue}
                </p>
              ))}
            </div>
          );
        })}
      </div>

      {activeExpandedScope && activeExpandedScope.candidates.length > 0 && (
        <div className="unified-candidates-drawer">
          <div className="unified-candidates-head">
            <strong>{activeExpandedScope.label}候选明细</strong>
            <small>共 {activeExpandedScope.candidates.length} 项</small>
          </div>
          <ul className="hidden-cleanup-candidates">
            {visibleCandidates.map(item => (
              <li key={item.id}>
                <strong>{item.title || item.id}</strong>
                <span>
                  {item.id}
                  {item.estimatedBytes > 0 ? ` · ${formatCleanupBytes(item.estimatedBytes)}` : ""}
                  {item.blockedReason ? ` · 已跳过：${item.blockedReason}` : ""}
                </span>
              </li>
            ))}
          </ul>
          {activeExpandedScope.candidates.length > 100 && (
            <CleanupPagination
              page={candidatePage}
              count={activeExpandedScope.candidates.length}
              onChange={setCandidatePage}
              label="候选"
            />
          )}
        </div>
      )}

      {plan && !dryRunPending && plan.totalDeletable === 0 && !result && (
        <p className="unified-cleanup-empty" role="status">
          全平台当前无可清理会话
        </p>
      )}

      {result && (
        <div className="unified-cleanup-result" role="status">
          <div className="unified-result-summary">
            <span>
              已删除 <strong>{result.deletedCount}</strong> 项
            </span>
            <span>
              实际回收 <strong>{formatCleanupBytes(result.deletedBytes)}</strong>
            </span>
            <span>
              已跳过 {result.skippedCount} · 失败 {result.failedCount}
              {result.stoppedEarly ? "（检测到失败或残留已停止后续删除）" : ""}
            </span>
          </div>
          {visibleUnsuccessful.map(entry => (
            <p className="cleanup-result-item" key={entry.key}>
              {entry.text}
              {entry.residuals.map(path => (
                <code className="delete-path" key={path}>
                  {path}
                </code>
              ))}
            </p>
          ))}
          {unsuccessfulItems.length > 100 && (
            <CleanupPagination
              page={resultPage}
              count={unsuccessfulItems.length}
              onChange={setResultPage}
              label="清理结果"
            />
          )}
        </div>
      )}

      {cleanupError && (
        <div role="alert" className="form-error cleanup-error">
          {cleanupError}
        </div>
      )}

      <div className="button-row ops-action-row cleanup-actions unified-cleanup-actions">
        <button className="secondary-button" disabled={busy} onClick={runFullPreview}>
          <Database size={17} />
          全平台扫描预览
        </button>
        <button className="secondary-button" disabled={busy} onClick={runHiddenPreview}>
          <Search size={16} />
          深度扫描隐藏线程
        </button>
        {hiddenDryRun.isPending && capabilities.hiddenCleanupScan && (
          <button
            className="secondary-button"
            disabled={hiddenScan?.phase === "stopping"}
            onClick={() => {
              setHiddenScan(current => (current ? { ...current, phase: "stopping" } : current));
              void actions.stopHiddenScan();
            }}
          >
            停止扫描
          </button>
        )}
        {!armed ? (
          <button className="danger-button soft" disabled={!canArm} onClick={() => setArmed(true)}>
            <Trash2 size={17} />
            一键清理所选平台
          </button>
        ) : (
          <>
            <button
              className="danger-button"
              disabled={
                unifiedExecute.isPending || activeSelectedScopes.length === 0 || selectedTargets.length === 0
              }
              onClick={() =>
                unifiedExecute.mutate({ scopes: activeSelectedScopes, targets: selectedTargets })
              }
            >
              <Trash2 size={17} />
              确认清理所选
            </button>
            <button
              className="secondary-button"
              disabled={unifiedExecute.isPending}
              onClick={() => setArmed(false)}
            >
              取消
            </button>
          </>
        )}
      </div>
    </Panel>
  );
}
