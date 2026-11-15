import { Archive, ArchiveRestore, Trash2, X } from "lucide-react";
import type { SessionOperation, SessionSelection } from "../../lib/query/sessions";
import type { ThreadMenuAction } from "./TaskMenu";
import { ConfirmDialog } from "./ConfirmDialog";

const labels: Record<SessionOperation, string> = {
  archive: "归档",
  restore: "恢复",
  delete: "删除",
};

export function buildBatchContextMenuActions(
  batch: SessionSelection,
  operations: readonly SessionOperation[],
): ThreadMenuAction[] {
  const count = batch.selection.length;
  if (count < 2) return [];
  const actions: ThreadMenuAction[] = operations.map(operation => ({
    id: `batch-${operation}`,
    label:
      operation === "delete"
        ? `删除所选 ${count} 项…`
        : `${labels[operation]}所选 ${count} 项`,
    icon:
      operation === "delete" ? (
        <Trash2 size={15} />
      ) : operation === "restore" ? (
        <ArchiveRestore size={15} />
      ) : (
        <Archive size={15} />
      ),
    danger: operation === "delete",
    disabled: batch.busy,
    run: () => batch.prepareBatch(operation),
  }));
  actions.push({
    id: "batch-clear",
    label: "取消选择",
    icon: <X size={15} />,
    disabled: batch.busy,
    run: batch.clear,
  });
  return actions;
}

export function SessionBatchControls({
  batch,
  operations,
  showSelection = true,
}: {
  batch: SessionSelection;
  operations: SessionOperation[];
  showSelection?: boolean;
}) {
  const trigger = batch.returnFocus;
  const allowed = batch.preview?.items.filter(i => i.allowed).length ?? 0;
  const showFloatingBar = showSelection && batch.selecting && batch.selection.length > 0;
  const hasFeedback =
    Boolean(batch.notice) ||
    Boolean(batch.prepare.error) ||
    Boolean(batch.execute.error) ||
    Boolean(batch.result);

  if (!showFloatingBar && !hasFeedback && !batch.preview) return null;

  return (
    <div className="session-batch-controls">
      {showFloatingBar && (
        <div className="finder-selection-bar" onKeyDown={batch.onListKeyDown} role="region" aria-label="已选会话操作">
          <div className="finder-selection-summary">
            <span className="finder-selection-count" role="status">
              已选 {batch.selection.length}
            </span>
            <button
              type="button"
              className="finder-link-button"
              disabled={batch.busy}
              onClick={batch.selectAll}
            >
              全选
            </button>
          </div>
          <div className="finder-selection-actions">
            {operations.map(operation => (
              <button
                key={operation}
                type="button"
                className={operation === "delete" ? "danger-button" : "secondary-button"}
                disabled={!batch.selection.length || batch.busy}
                onClick={event => batch.prepareBatch(operation, event.currentTarget)}
              >
                {labels[operation]}所选线程
              </button>
            ))}
            <button
              type="button"
              className="icon-button compact"
              title="取消多选"
              aria-label="取消多选"
              disabled={batch.execute.isPending}
              onClick={batch.clear}
            >
              <X size={15} />
            </button>
          </div>
        </div>
      )}
      {batch.notice && (
        <p role="alert" className="form-error">
          {batch.notice}
        </p>
      )}
      {batch.prepare.error && (
        <p role="alert" className="form-error">
          {batch.prepare.error.message}
        </p>
      )}
      {batch.execute.error && (
        <p role="alert" className="form-error">
          {batch.execute.error.message}。请刷新并重新预览。
        </p>
      )}
      {batch.result && (
        <div className="batch-result-banner" role="status">
          <p>
            已完成 {batch.result.items.filter(i => i.status === "succeeded").length} 项，未完成{" "}
            {batch.result.items.filter(i => i.status !== "succeeded").length} 项
          </p>
          {batch.result.items
            .filter(i => i.status !== "succeeded")
            .map(item => (
              <div className="form-error" key={item.sessionKey}>
                <p>
                  {item.sessionKey}：{item.message}
                </p>
                {item.deletedIds?.length ? <p>已删除 {item.deletedIds.length} 个线程</p> : null}
                {item.residuals?.map(residual => (
                  <code className="delete-path" key={residual}>
                    {residual}
                  </code>
                ))}
              </div>
            ))}
        </div>
      )}
      {batch.preview && (
        <ConfirmDialog
          labelledBy="batch-confirm-title"
          busy={batch.execute.isPending}
          onCancel={batch.dismissPreview}
          returnFocus={trigger}
        >
          <h2 id="batch-confirm-title">{labels[batch.preview.operation]}所选线程</h2>
          <p>
            已定位 {allowed} 项，定位失败 {batch.preview.items.length - allowed} 项。
            {batch.preview.operation === "delete"
              ? "仅删除下列会话数据，工作目录和配置保留。"
              : "确认后更新所选线程的归档状态。"}
          </p>
          <ul className="batch-preview-items">
            {batch.preview.items.map(item => (
              <li key={item.sessionKey}>
                <strong>{item.title}</strong>
                <small>{item.id}</small>
                {item.descendants?.length ? (
                  <>
                    <span>同时删除 {item.descendants.length} 个后代线程</span>
                    {item.descendants.map(id => (
                      <small key={id}>{id}</small>
                    ))}
                  </>
                ) : null}
                {item.paths.map(path => (
                  <code className="delete-path" key={path}>
                    {path}
                  </code>
                ))}
                <span>{item.bytes.toLocaleString()} 字节</span>
                {item.warnings?.map(warning => (
                  <span className="form-error" key={warning}>
                    {warning}
                  </span>
                ))}
                {!item.allowed && <span className="form-error">{item.reason}</span>}
              </li>
            ))}
          </ul>
          {batch.execute.error && (
            <p role="alert" className="form-error">
              {batch.execute.error.message}
            </p>
          )}
          <div className="button-row">
            <button
              type="button"
              className="secondary-button"
              disabled={batch.execute.isPending}
              onClick={batch.dismissPreview}
            >
              取消
            </button>
            <button
              type="button"
              className={
                batch.preview.operation === "delete" ? "danger-button" : "secondary-button"
              }
              disabled={!allowed || batch.execute.isPending}
              onClick={() => batch.execute.mutate()}
            >
              {batch.execute.isPending
                ? "正在处理…"
                : `确认${labels[batch.preview.operation]} ${allowed} 项`}
            </button>
          </div>
        </ConfirmDialog>
      )}
    </div>
  );
}
