import { LockKeyholeOpen, Search } from "lucide-react";
import { useState } from "react";
import { OPS_PANEL_TITLES } from "../../lib/domain/runtimeViewModel";
import { useCodexAccountScope } from "../../lib/query/ops";
import type { RuntimeCapabilityMatrix } from "../../lib/query/system";
import type { CodexAccountScopeItem } from "../../types";
import { Panel } from "../common/Panel";
import { CleanupPagination } from "./UnifiedThreadCleanup";

const REASON_LABELS: Record<CodexAccountScopeItem["reason"], string> = {
  format: "格式不支持",
  busy: "Codex 使用中",
  changed: "文件已变化",
  original_missing: "缺少原始会话",
  failed: "失败",
};

export function CodexAccountScope({ capabilities }: { capabilities: RuntimeCapabilityMatrix }) {
  const supported = capabilities.codexAccountScope === true;
  const [armed, setArmed] = useState(false);
  const [page, setPage] = useState(0);
  const { check, clear, auto } = useCodexAccountScope(supported);
  if (!supported) return null;

  const busy = check.isPending || clear.isPending;
  const scoped = check.data?.scopedThreads;
  const result = clear.data;
  const error = clear.error ?? check.error ?? auto.error;
  const stage = clear.isPending
    ? { label: "解除中", tone: "warning" }
    : check.isPending
      ? { label: "检查中", tone: "warning" }
      : armed
        ? { label: "等待确认", tone: "danger" }
        : scoped === undefined
          ? { label: "待检查", tone: "muted" }
          : scoped > 0
            ? { label: `${scoped} 个会话`, tone: "warning" }
            : { label: "无隔离会话", tone: "success" };

  return (
    <Panel
      title={OPS_PANEL_TITLES.codexAccountScope}
      icon={<LockKeyholeOpen size={18} />}
      className="wide-panel"
      action={
        <span role="status" className={`status-chip tone-${stage.tone}`}>
          {stage.label}
        </span>
      }
    >
      <label className="toggle-row">
        <span>回合结束后自动解除</span>
        {!auto.hookInstalled && <span className="status-chip tone-muted">未安装 Codex Hook</span>}
        <input
          type="checkbox"
          checked={auto.enabled}
          disabled={!auto.hookInstalled || auto.pending}
          onChange={event => auto.set(event.target.checked)}
        />
      </label>

      {result && (
        <div className="unified-cleanup-result" role="status">
          <div className="unified-result-summary">
            <span>
              已解除 <strong>{result.clearedThreads}</strong> 个会话
            </span>
            {result.remainingFiles > 0 && <span>剩余 {result.remainingFiles} 个文件</span>}
          </div>
          {result.items.slice(page * 100, (page + 1) * 100).map(item => (
            <p className="cleanup-result-item" key={item.threadId}>
              {item.threadId}：{REASON_LABELS[item.reason]}
              {item.detail ? `（${item.detail}）` : ""}
            </p>
          ))}
          {result.items.length > 100 && (
            <CleanupPagination page={page} count={result.items.length} onChange={setPage} label="解除结果" />
          )}
        </div>
      )}

      {error && (
        <div role="alert" className="form-error cleanup-error">
          {error.message}
        </div>
      )}

      <div className="button-row ops-action-row cleanup-actions">
        <button
          className="secondary-button"
          disabled={busy}
          onClick={() => {
            setArmed(false);
            setPage(0);
            clear.reset();
            check.mutate();
          }}
        >
          <Search size={16} />
          检查
        </button>
        {!armed ? (
          <button className="danger-button soft" disabled={busy || !scoped} onClick={() => setArmed(true)}>
            <LockKeyholeOpen size={17} />
            解除
          </button>
        ) : (
          <>
            <button
              className="danger-button"
              onClick={() => {
                setArmed(false);
                setPage(0);
                clear.mutate();
              }}
            >
              <LockKeyholeOpen size={17} />
              确认解除
            </button>
            <button className="secondary-button" onClick={() => setArmed(false)}>
              取消
            </button>
          </>
        )}
      </div>
    </Panel>
  );
}
