import { Check, ChevronDown, Cloud, Monitor, Pencil, Plus, Power, Trash2 } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import {
  remoteRemove,
  remoteSave,
  remoteSelect,
  useConnection,
  type RemoteConnectionCredentials,
  type RemoteMachineSummary,
} from "../../lib/query/connection";

function resolvedMachines(connection: ReturnType<typeof useConnection>): RemoteMachineSummary[] {
  if (connection.machines && connection.machines.length > 0) return connection.machines;
  if (connection.configured && connection.baseUrl) {
    return [
      {
        id: connection.activeMachineId ?? "m-legacy",
        name: connection.activeMachineName ?? "远程机器",
        baseUrl: connection.baseUrl,
      },
    ];
  }
  return [];
}

function resolveActiveMachine(
  connection: ReturnType<typeof useConnection>,
  machines: RemoteMachineSummary[],
): RemoteMachineSummary | null {
  return (
    machines.find(m => m.id === connection.activeMachineId) ??
    machines.find(m => m.baseUrl === connection.baseUrl) ??
    machines[0] ??
    null
  );
}

export function MachineButton({
  collapsed = false,
  onConfigure,
}: {
  collapsed?: boolean;
  onConfigure: () => void;
}) {
  const connection = useConnection();
  const [error, setError] = useState("");
  const machines = useMemo(() => resolvedMachines(connection), [connection]);
  const activeMachine = useMemo(() => resolveActiveMachine(connection, machines), [connection, machines]);
  const multiMode = machines.length >= 2;
  const currentLabel =
    connection.target === "local"
      ? "本机"
      : activeMachine?.name || connection.activeMachineName || "远程机器";
  const selectValue =
    connection.target === "local"
      ? "local"
      : multiMode && activeMachine
        ? `machine:${activeMachine.id}`
        : "remote";

  const runSelect = async (target: "local" | "remote", machineId?: string) => {
    setError("");
    try {
      await remoteSelect(target, machineId);
    } catch (err) {
      setError((err as Error).message);
    }
  };

  return (
    <div className="machine-control">
      <label className={`machine-button ${collapsed ? "compact" : ""}`} title={currentLabel}>
        {connection.target === "local" ? <Monitor size={16} /> : <Cloud size={16} />}
        {!collapsed && <span>{currentLabel}</span>}
        {!collapsed && (
          <i
            className={`machine-status-dot${connection.target === "remote" ? " is-remote" : ""}`}
            aria-hidden="true"
          />
        )}
        {!collapsed && <ChevronDown size={13} className="machine-chevron" aria-hidden="true" />}
        <select
          aria-label="当前机器"
          value={selectValue}
          disabled={Boolean(connection.writes || connection.changing)}
          onChange={event => {
            const value = event.target.value;
            if (value === "__configure__" || (value === "remote" && !connection.configured)) {
              onConfigure();
            } else if (value === "local" || value === "remote") {
              void runSelect(value);
            } else if (value.startsWith("machine:")) {
              void runSelect("remote", value.slice("machine:".length));
            }
          }}
        >
          <option value="local">本机</option>
          {!multiMode && (
            <option value="remote">
              {machines[0]?.name || connection.activeMachineName || "远程机器"}
            </option>
          )}
          {multiMode &&
            machines.map(machine => (
              <option key={machine.id} value={`machine:${machine.id}`}>
                {machine.name}
              </option>
            ))}
          {connection.configured && <option value="__configure__">管理远程机器…</option>}
        </select>
      </label>
      {error && (
        <span role="alert" className="form-error">
          {error}
        </span>
      )}
    </div>
  );
}

export function RemoteConnectionSettings() {
  const connection = useConnection();
  const machines = useMemo(() => resolvedMachines(connection), [connection]);
  const activeMachine = useMemo(() => resolveActiveMachine(connection, machines), [connection, machines]);

  const [mode, setMode] = useState<"edit" | "create">(() => (machines.length > 0 ? "edit" : "create"));
  const [editingId, setEditingId] = useState<string | null>(() => activeMachine?.id ?? null);
  const [name, setName] = useState(() => activeMachine?.name ?? "");
  const [baseUrl, setBaseUrl] = useState(() => activeMachine?.baseUrl ?? connection.baseUrl ?? "");
  const [apiKey, setApiKey] = useState("");
  const [error, setError] = useState("");
  const [saved, setSaved] = useState(false);
  const busy = Boolean(connection.writes || connection.changing);

  useEffect(() => {
    if (machines.length === 0) {
      setMode("create");
      setEditingId(null);
      return;
    }
    if (mode === "edit") {
      const target = machines.find(m => m.id === editingId) ?? activeMachine ?? machines[0];
      if (target && target.id !== editingId) {
        setEditingId(target.id);
        setName(target.name === "远程机器" ? "" : target.name);
        setBaseUrl(target.baseUrl);
      }
    }
  }, [activeMachine, editingId, machines, mode]);

  const applyDraft = (nextMode: "create" | "edit", machine?: RemoteMachineSummary | null) => {
    setMode(nextMode);
    setEditingId(machine?.id ?? null);
    setName(machine?.name ?? "");
    setBaseUrl(machine?.baseUrl ?? "");
    setApiKey("");
    setError("");
    setSaved(false);
  };

  const runAction = async (work: () => Promise<void>) => {
    setError("");
    try {
      await work();
    } catch (err) {
      setError((err as Error).message);
    }
  };

  const isEditingExisting = mode === "edit" && Boolean(editingId) && machines.some(m => m.id === editingId);
  const canSubmit = !busy && Boolean(baseUrl.trim()) && (Boolean(apiKey.trim()) || isEditingExisting);

  return (
    <div className="remote-settings-stack">
      {machines.length > 0 && (
        <section className="panel remote-machines-panel">
          <header className="remote-machines-header">
            <div className="remote-machines-heading">
              <Cloud size={18} />
              <strong>远程机器列表</strong>
            </div>
            <button
              type="button"
              className="secondary-button"
              disabled={busy}
              onClick={() => applyDraft("create", null)}
            >
              <Plus size={16} />新增远程机器
            </button>
          </header>
          <div className="remote-machine-list">
            {machines.map(machine => {
              const isConnected =
                connection.target === "remote" &&
                (connection.activeMachineId
                  ? connection.activeMachineId === machine.id
                  : connection.baseUrl === machine.baseUrl);
              const isSelectedForEdit = mode === "edit" && editingId === machine.id;
              return (
                <article
                  key={machine.id}
                  className={`remote-machine-card ${isConnected ? "is-connected" : ""} ${isSelectedForEdit ? "is-editing" : ""}`}
                >
                  <div className="remote-machine-card-main">
                    <div className="remote-machine-title-row">
                      <strong>{machine.name}</strong>
                      <span className={`status-chip ${isConnected ? "tone-success" : "tone-neutral"}`}>
                        {isConnected ? "当前连接" : "已保存"}
                      </span>
                    </div>
                    <code>{machine.baseUrl}</code>
                  </div>
                  <div className="button-row remote-machine-actions">
                    <button
                      type="button"
                      className="secondary-button"
                      disabled={busy}
                      onClick={() =>
                        void runAction(() =>
                          isConnected
                            ? remoteSelect("local").then(() => undefined)
                            : remoteSelect("remote", machines.length >= 2 ? machine.id : undefined).then(
                                () => undefined,
                              ),
                        )
                      }
                    >
                      {isConnected ? (
                        <>
                          <Power size={15} />切回本机
                        </>
                      ) : (
                        <>
                          <Check size={15} />连接此机器
                        </>
                      )}
                    </button>
                    <button
                      type="button"
                      className="secondary-button"
                      disabled={busy}
                      onClick={() => applyDraft("edit", machine)}
                    >
                      <Pencil size={15} />编辑
                    </button>
                    <button
                      type="button"
                      className="danger-button soft"
                      disabled={busy}
                      onClick={() =>
                        void runAction(async () => {
                          setSaved(false);
                          const view = await remoteRemove(machines.length >= 2 ? machine.id : undefined);
                          setApiKey("");
                          if (!view.configured) applyDraft("create", null);
                        })
                      }
                    >
                      <Trash2 size={15} />移除连接
                    </button>
                  </div>
                </article>
              );
            })}
          </div>
        </section>
      )}

      <form
        className="panel remote-connection-form"
        onSubmit={event => {
          event.preventDefault();
          setSaved(false);
          void runAction(async () => {
            const payload: RemoteConnectionCredentials = {
              revision: connection.revision,
              baseUrl,
              ...(apiKey ? { apiKey } : {}),
              ...(isEditingExisting && editingId ? { id: editingId } : {}),
              ...(name.trim() ? { name: name.trim() } : {}),
            };
            const view = await remoteSave(payload);
            setApiKey("");
            setSaved(true);
            setMode("edit");
            if (view.activeMachineId) setEditingId(view.activeMachineId);
            if (view.activeMachineName && !name.trim()) setName(view.activeMachineName);
          });
        }}
      >
        <strong>{isEditingExisting ? "编辑远程机器" : "新增远程机器"}</strong>
        <label>
          机器名称
          <input
            type="text"
            maxLength={64}
            value={name}
            onChange={event => setName(event.target.value)}
            autoComplete="off"
            disabled={busy}
            placeholder="例如：主力云端工作站（留空默认取域名）"
          />
        </label>
        <label>
          HTTPS 地址
          <input
            type="url"
            required
            value={baseUrl}
            onChange={event => setBaseUrl(event.target.value)}
            autoComplete="off"
            disabled={busy}
            placeholder="https://api.example.com/agenthub/"
          />
        </label>
        <label>
          管理员 API Key
          <input
            type="password"
            required={!isEditingExisting}
            value={apiKey}
            onChange={event => setApiKey(event.target.value)}
            autoComplete="off"
            disabled={busy}
            placeholder={isEditingExisting ? "留空保持当前 API Key 不变" : "nhk_..."}
          />
        </label>
        <div className="button-row">
          <button className="primary-button" disabled={!canSubmit}>
            {connection.changing ? "连接中…" : "验证并保存"}
          </button>
          {mode === "create" && machines.length > 0 && (
            <button
              type="button"
              className="secondary-button"
              disabled={busy}
              onClick={() => activeMachine && applyDraft("edit", activeMachine)}
            >
              取消新增
            </button>
          )}
        </div>
        {(saved || connection.notice) && (
          <span role="status" className="form-success">
            已保存
          </span>
        )}
        {error && (
          <div className="form-error" role="alert">
            {error}
          </div>
        )}
      </form>
    </div>
  );
}
