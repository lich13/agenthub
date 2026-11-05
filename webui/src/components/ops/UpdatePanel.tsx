import { Download, RefreshCw, Trash2 } from "lucide-react";
import { Metric, Panel } from "../common/Panel";
import { RunningIndicator } from "../common/RunningIndicator";
import { displayVersion, opsUpdateActionView, updatePanelTitle } from "../../lib/domain/runtimeViewModel";
import type { RuntimeCapabilityMatrix } from "../../lib/domain/capabilities";
import type { UnifiedUpdateAction } from "../../lib/api/updates";
import type { JobRecord, UpdateStatus } from "../../types";

export function UpdatePanel({ capabilities, status, loading, action, busy, finished, job, error, onAction }: {
  capabilities: RuntimeCapabilityMatrix;
  status?: UpdateStatus;
  loading: boolean;
  action?: UnifiedUpdateAction;
  busy: boolean;
  finished: boolean;
  job?: JobRecord;
  error?: string;
  onAction: (action: UnifiedUpdateAction) => void;
}) {
  const failure = error || (job?.status === "failed" ? job.error || "更新操作失败" : undefined)
    || (!busy && status?.state === "failed" ? "检查更新失败，请重试" : undefined);
  const working = busy || status?.state === "checking" || status?.state === "installing";
  const progress = action === "prune" ? "正在清理更新备份…"
    : action === "install" || status?.state === "installing"
      ? "正在下载并安装…"
      : "正在检查更新…";
  const feedback = finished && !failure && !working
    ? action === "prune" ? "更新备份已清理"
      : action === "install" ? "更新完成"
        : status?.update_available === false && /^v?\d+\.\d+\.\d+(?:[-+].*)?$/i.test(status.latest_version ?? "")
          && displayVersion(status.latest_version) === displayVersion(status.current_version) ? "已是最新版本" : null
    : null;
  return (
    <Panel title={updatePanelTitle(capabilities)} icon={<RefreshCw size={18} />} className="update-panel">
      <div className="update-panel-row">
        <Metric label="当前版本" value={status ? displayVersion(status.current_version) : loading ? "读取中…" : "未知"} />
        {status?.update_available && status.latest_version && (
          <Metric label="可用版本" value={displayVersion(status.latest_version)} tone="info" />
        )}
        <div className="button-row">
          {opsUpdateActionView(error ? undefined : status, capabilities).map((item) => (
            <button key={item.action}
              className={item.tone === "primary" ? "primary-button" : item.tone === "danger" ? "danger-button soft" : "secondary-button"}
              disabled={working || item.disabled} onClick={() => onAction(item.action)}>
              {item.action === "check" ? <RefreshCw size={17} /> : item.action === "install" ? <Download size={17} /> : <Trash2 size={17} />}
              {item.label}
            </button>
          ))}
        </div>
      </div>
      {failure && <div role="alert" className="form-error update-feedback">{failure}</div>}
      {working && !failure && <div role="status" className="update-feedback"><RunningIndicator />{progress}</div>}
      {feedback && <div role="status" className="form-success update-feedback">{feedback}</div>}
    </Panel>
  );
}
