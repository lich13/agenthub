import { ChevronDown, Cloud, Monitor, RefreshCw, Trash2 } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { ConfirmDialog } from "../common/ConfirmDialog";
import {
  remoteInstall, remoteRemove, remoteSave, remoteSelect, remoteSshHosts, remoteSshDelete,
  useConnection, type RemoteMachineSummary,
} from "../../lib/query/connection";

export function MachineButton({ collapsed = false, onConfigure }: { collapsed?: boolean; onConfigure: () => void }) {
  const connection = useConnection();
  const [error, setError] = useState("");
  const busy = Boolean(connection.writes || connection.changing);
  const label = connection.target === "local" ? "本机" : connection.activeMachineName || "远程机器";
  const select = (id: string) => {
    if (id === "configure") { onConfigure(); return; }
    setError("");
    void remoteSelect(id === "local" ? "local" : "remote", id === "local" ? undefined : id)
      .catch(error => setError(error.message));
  };
  return <div className="machine-control">
    <label className={`machine-button ${collapsed ? "compact" : ""}`} title={label}>
      {connection.target === "local" ? <Monitor size={16} /> : <Cloud size={16} />}
      {!collapsed && <span>{label}</span>}
      <select aria-label="当前机器" value={connection.target === "local" ? "local" : connection.activeMachineId ?? "local"}
        disabled={busy} onChange={event => select(event.target.value)}>
        <option value="local">本机</option>
        {connection.machines.map(machine => <option key={machine.id} value={machine.id}>{machine.name}</option>)}
        <option value="configure">管理远程机器…</option>
      </select>
    </label>
    {connection.target === "remote" && <button className="icon-button" aria-label="重新连接当前机器" title="重新连接"
      disabled={busy} onClick={() => select(connection.activeMachineId ?? "")}><RefreshCw size={15} /></button>}
    {error && <span role="alert" className="form-error">{error}</span>}
  </div>;
}

export function RemoteConnectionSettings() {
  const connection = useConnection();
  const [hosts, setHosts] = useState<string[]>([]);
  const [sshHost, setHost] = useState("");
  const [name, setName] = useState("");
  const [useSudo, setSudo] = useState(true);
  const [id, setId] = useState<string>();
  const [error, setError] = useState("");
  const [installing, setInstalling] = useState(false);
  const [notice, setNotice] = useState("");
  const [deleting, setDeleting] = useState<string>();
  const deleteButton = useRef<HTMLButtonElement>(null);
  const busy = Boolean(connection.writes || connection.changing || installing);
  const reload = () => remoteSshHosts().then(rows => {
    setHosts(rows.map(row => row.alias));
    setHost(value => rows.some(row => row.alias === value) ? value : rows[0]?.alias || "");
    setError("");
  }).catch(error => setError(error.message));
  useEffect(() => { void reload(); }, []);
  const edit = (machine: RemoteMachineSummary) => {
    setId(machine.id); setHost(machine.sshHost); setName(machine.name); setSudo(machine.useSudo);
    setError(""); setNotice("");
  };
  const act = async (work: () => Promise<unknown>) => {
    setError(""); setNotice("");
    try { await work(); } catch (error) { setError((error as Error).message); }
  };
  const submit = (install: boolean) => act(async () => {
    const request = { revision: connection.revision, sshHost, name: name || sshHost, useSudo, id };
    setInstalling(install);
    try {
      const view = install ? await remoteInstall(request) : await remoteSave(request);
      setId(view.activeMachineId ?? undefined);
      setNotice("已连接");
    } finally { setInstalling(false); }
  });
  return <div className="remote-settings-stack">
    {connection.machines.length > 0 && <section className="panel remote-machines-panel">
      <header><strong>远程机器</strong></header>
      <div className="remote-machine-list">
        {connection.machines.map(machine => <article className="remote-machine-card" key={machine.id}>
          <div className="remote-machine-card-main"><strong>{machine.name}</strong>{machine.name !== machine.sshHost && <span>{machine.sshHost}</span>}</div>
          <div className="button-row">
            <button className="secondary-button" disabled={busy} onClick={() => void act(() => remoteSelect("remote", machine.id))}>
              {connection.target === "remote" && connection.activeMachineId === machine.id ? "重新连接" : "连接"}
            </button>
            <button className="secondary-button" disabled={busy} onClick={() => edit(machine)}>编辑</button>
            <button className="danger-button soft" disabled={busy} onClick={() => void act(async () => {
              await remoteRemove(machine.id);
              if (id === machine.id) { setId(undefined); setName(""); }
            })}><Trash2 size={15} />移除</button>
          </div>
        </article>)}
      </div>
    </section>}
    <section className="panel">
      <header><strong>{id ? "编辑连接" : "新增 SSH 连接"}</strong></header>
      <fieldset disabled={busy} className="probe-card-stack">
        <div className="form-grid">
          <label className="field-label">SSH 连接
            <span className="remote-host-select"><select value={sshHost} onChange={event => { setHost(event.target.value); setError(""); setNotice(""); }}>
              <option value="">选择已保存的连接</option>
              {[...new Set([...hosts, ...(sshHost ? [sshHost] : [])])].map(host => <option key={host} value={host}>{host}</option>)}
            </select><ChevronDown size={15} aria-hidden="true" /></span>
          </label>
          <label className="field-label">名称<input value={name} onChange={event => setName(event.target.value)} placeholder={sshHost} /></label>
        </div>
        <label className="toggle-row"><span>使用 sudo</span><input type="checkbox" checked={useSudo} onChange={event => setSudo(event.target.checked)} /></label>
        <div className="button-row">
          <button className="primary-button" disabled={!sshHost || busy} onClick={() => void submit(false)}>保存并连接</button>
          <button className="secondary-button" disabled={!sshHost || busy} onClick={() => void submit(true)}>{installing ? "正在安装…" : "安装并连接"}</button>
          <button className="secondary-button" onClick={() => void reload()}><RefreshCw size={15} />刷新 SSH 配置</button>
          <button ref={deleteButton} className="danger-button soft" disabled={!sshHost || busy} onClick={() => { setError(""); setDeleting(sshHost); }}><Trash2 size={15} />删除 SSH 配置</button>
          {id && <button className="secondary-button" onClick={() => { setId(undefined); setName(""); }}>新增连接</button>}
        </div>
      </fieldset>
      {error && <div role="alert" className="form-error">{error}</div>}
      {notice && <div role="status" className="form-success">{notice}</div>}
    </section>
    {deleting && <ConfirmDialog labelledBy="delete-ssh-title" busy={busy} onCancel={() => setDeleting(undefined)} returnFocus={deleteButton}>
      <h2 id="delete-ssh-title">删除 SSH 配置</h2>
      <p>删除“{deleting}”及其在 AgentHub 中的连接记录？</p>
      {error && <div role="alert" className="form-error">{error}</div>}
      <div className="button-row">
        <button className="secondary-button" disabled={busy} onClick={() => setDeleting(undefined)}>取消</button>
        <button className="danger-button" disabled={busy} onClick={() => void act(async () => {
          await remoteSshDelete(deleting);
          setDeleting(undefined); setId(undefined); setName(""); setHost("");
          await reload(); setNotice("SSH 配置已删除");
        })}><Trash2 size={15} />确认删除</button>
      </div>
    </ConfirmDialog>}
  </div>;
}
