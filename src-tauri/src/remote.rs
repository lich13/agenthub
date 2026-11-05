#![allow(non_snake_case)]
mod install;
mod ssh;
mod ssh_config;
use agenthub_core::services::{
    commands::{is_allowed_rpc_command, is_mutating_rpc_command},
    remote::{credentials::CredentialAccess, *},
    system::{HostSurface, SystemCapabilitiesResponse, API_VERSION},
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Preferences {
    target: MachineTarget,
    revision: u64,
    active_machine_id: Option<String>,
    #[serde(default)]
    machines: Vec<RemoteMachineSummary>,
}
impl Preferences {
    fn active(&self) -> Option<&RemoteMachineSummary> {
        self.machines
            .iter()
            .find(|m| Some(&m.id) == self.active_machine_id.as_ref())
    }
}
#[derive(Default)]
struct ConnectionState {
    prefs: Preferences,
    writes: usize,
    changing: bool,
    client: Option<Arc<ssh::Client>>,
}
#[derive(Clone)]
pub struct RemoteConnections {
    state: Arc<Mutex<ConnectionState>>,
    path: PathBuf,
    credentials: CredentialAccess,
    legacy: Arc<Mutex<Option<Value>>>,
}
impl RemoteConnections {
    pub fn load(data_dir: &Path) -> Result<Self, String> {
        fs::create_dir_all(data_dir).map_err(|_| "无法创建连接配置目录")?;
        let path = data_dir.join("remote-connection.json");
        let raw = if path.exists() {
            Some(
                serde_json::from_slice::<Value>(&fs::read(&path).map_err(|_| "无法读取连接配置")?)
                    .map_err(|_| "连接配置损坏")?,
            )
        } else {
            None
        };
        let old = raw.as_ref().is_some_and(|v| {
            v.get("baseUrl").is_some()
                || v["machines"]
                    .as_array()
                    .is_some_and(|ms| ms.iter().any(|m| m.get("baseUrl").is_some()))
        });
        let prefs = if old {
            Preferences::default()
        } else {
            raw.as_ref()
                .map(|v| serde_json::from_value(v.clone()))
                .transpose()
                .map_err(|_| "连接配置损坏")?
                .unwrap_or_default()
        };
        Ok(Self {
            state: Arc::new(Mutex::new(ConnectionState {
                prefs,
                ..Default::default()
            })),
            path,
            credentials: CredentialAccess::default(),
            legacy: Arc::new(Mutex::new(if old { raw } else { None })),
        })
    }
    pub(crate) fn disconnect(&self) {
        if let Some(client) = self.state.lock().expect("connection").client.take() {
            client.close();
        }
    }
    pub(crate) fn credential_access(&self) -> CredentialAccess {
        self.credentials.clone()
    }
    pub(crate) fn require_local(&self, revision: u64) -> Result<(), String> {
        let s = self.state.lock().expect("connection");
        if s.changing || s.prefs.revision != revision || s.prefs.target != MachineTarget::Local {
            return Err("连接已变化，操作已取消".into());
        }
        Ok(())
    }
    pub(crate) fn begin_local_write(&self, revision: u64) -> Result<LocalWriteGuard, String> {
        let mut s = self.state.lock().expect("connection");
        if s.changing || s.prefs.revision != revision || s.prefs.target != MachineTarget::Local {
            return Err("连接已变化，操作已取消".into());
        }
        s.writes += 1;
        Ok(LocalWriteGuard(self.state.clone()))
    }
    fn begin_change(&self, revision: u64) -> Result<ChangeGuard, String> {
        let mut s = self.state.lock().expect("connection");
        if s.prefs.revision != revision {
            return Err("连接已变化，请重试".into());
        }
        if s.changing || s.writes > 0 {
            return Err("操作进行中，请稍后切换".into());
        }
        s.changing = true;
        Ok(ChangeGuard(self.state.clone()))
    }
    fn view(&self) -> RemoteConnectionView {
        let s = self.state.lock().expect("connection");
        let active = s.prefs.active();
        RemoteConnectionView {
            target: s.prefs.target,
            revision: s.prefs.revision,
            ssh_host: active.map(|m| m.ssh_host.clone()),
            configured: !s.prefs.machines.is_empty(),
            active_machine_id: active.map(|m| m.id.clone()),
            active_machine_name: active.map(|m| m.name.clone()),
            machines: s.prefs.machines.clone(),
        }
    }
    fn persist(&self, prefs: &Preferences) -> Result<(), String> {
        let temp = self.path.with_extension("json.pending");
        let result = (|| -> anyhow::Result<()> {
            let mut opts = fs::OpenOptions::new();
            opts.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                opts.mode(0o600);
            }
            let mut f = opts.open(&temp)?;
            f.write_all(&serde_json::to_vec(prefs)?)?;
            f.sync_all()?;
            fs::rename(&temp, &self.path)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        result.map_err(|_| "保存连接配置失败".into())
    }
    async fn retire_legacy(&self) -> Result<(), String> {
        let old = self.legacy.lock().expect("legacy").clone();
        let Some(old) = old else {
            return Ok(());
        };
        self.credentials
            .transaction(
                agenthub_core::resource_limits::CREDENTIAL_WAIT,
                move || -> Result<(), String> {
                    let mut accounts = Vec::new();
                    if let Some(base) = old["baseUrl"].as_str() {
                        accounts.push(base.to_string());
                        accounts.push("machine:m-legacy".into());
                    }
                    if let Some(ms) = old["machines"].as_array() {
                        for m in ms {
                            if let Some(id) = m["id"].as_str() {
                                accounts.push(format!("machine:{id}"));
                            }
                            if let Some(base) = m["baseUrl"].as_str() {
                                accounts.push(base.into());
                            }
                        }
                    }
                    accounts.sort();
                    accounts.dedup();
                    for account in accounts {
                        let entry = keyring::Entry::new("com.lich13.agenthub.remote", &account)
                            .map_err(|_| "无法清理旧远程凭据")?;
                        match entry.delete_credential() {
                            Ok(()) | Err(keyring::Error::NoEntry) => {}
                            Err(_) => {
                                return Err("无法清理旧远程凭据，请检查钥匙串授权后重试".into())
                            }
                        }
                    }
                    Ok(())
                },
            )
            .await
            .map_err(|_| "旧远程凭据清理失败")??;
        self.persist(&self.state.lock().expect("connection").prefs)?;
        *self.legacy.lock().expect("legacy") = None;
        Ok(())
    }
    fn machine(request: &RemoteConnectionCredentials) -> Result<RemoteMachineSummary, String> {
        let host = request.ssh_host.trim();
        if !ssh_config::valid_alias(host) {
            return Err("请选择有效的 SSH Host".into());
        }
        let name = request.name.as_deref().unwrap_or(host).trim();
        if name.is_empty() || name.chars().count() > 64 || name.chars().any(char::is_control) {
            return Err("机器名称无效".into());
        }
        Ok(RemoteMachineSummary {
            id: request.id.clone().unwrap_or_else(|| {
                format!(
                    "ssh-{:x}",
                    SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_nanos()
                )
            }),
            name: name.into(),
            ssh_host: host.into(),
            use_sudo: request.use_sudo,
        })
    }
    fn commit(
        &self,
        mut prefs: Preferences,
        client: Option<Arc<ssh::Client>>,
    ) -> Result<RemoteConnectionView, String> {
        prefs.revision = prefs.revision.wrapping_add(1);
        self.persist(&prefs)?;
        {
            let mut s = self.state.lock().expect("connection");
            if let Some(previous) = s.client.take() {
                if !client
                    .as_ref()
                    .is_some_and(|next| Arc::ptr_eq(&previous, next))
                {
                    previous.close();
                }
            }
            s.prefs = prefs;
            s.client = client;
        }
        Ok(self.view())
    }
    async fn connect(
        machine: &RemoteMachineSummary,
    ) -> Result<(Arc<ssh::Client>, SystemCapabilitiesResponse), String> {
        let client = ssh::Client::connect(&machine.ssh_host, machine.use_sudo)?;
        let value = client
            .call("system.capabilities", json!({}))
            .await
            .map_err(|_| {
                "无法连接 AgentHub；请先在终端完成 SSH 认证，并确认 sudo 权限与服务安装状态"
                    .to_string()
            })?;
        let caps: SystemCapabilitiesResponse =
            serde_json::from_value(value).map_err(|_| "远程协议不兼容，请更新服务")?;
        if caps.api_version != API_VERSION || caps.host_surface != HostSurface::LinuxServerApi {
            return Err("远程协议不兼容，请更新服务".into());
        }
        Ok((client, caps))
    }
}
struct ChangeGuard(Arc<Mutex<ConnectionState>>);
impl Drop for ChangeGuard {
    fn drop(&mut self) {
        self.0.lock().expect("connection").changing = false;
    }
}
pub(crate) struct LocalWriteGuard(Arc<Mutex<ConnectionState>>);
impl Drop for LocalWriteGuard {
    fn drop(&mut self) {
        self.0.lock().expect("connection").writes -= 1;
    }
}
#[tauri::command(rename = "remote.get")]
pub async fn remoteGet(
    state: tauri::State<'_, RemoteConnections>,
) -> Result<RemoteConnectionView, String> {
    state.retire_legacy().await?;
    let selected = {
        let s = state.state.lock().expect("connection");
        (s.prefs.target == MachineTarget::Remote && s.client.is_none())
            .then(|| (s.prefs.revision, s.prefs.active().cloned()))
    };
    if let Some((revision, Some(machine))) = selected {
        let _guard = state.begin_change(revision)?;
        if let Ok((client, _)) = RemoteConnections::connect(&machine).await {
            state.state.lock().expect("connection").client = Some(client);
        }
    }
    Ok(state.view())
}
#[tauri::command(rename = "remote.sshHosts")]
pub async fn remoteSshHosts() -> Result<SshHostList, String> {
    tokio::task::spawn_blocking(ssh_config::list)
        .await
        .map_err(|_| "SSH 配置读取失败")?
}
#[tauri::command(rename = "remote.verify")]
pub async fn remoteVerify(
    state: tauri::State<'_, RemoteConnections>,
    request: RemoteConnectionCredentials,
) -> Result<SystemCapabilitiesResponse, String> {
    let _guard = state.begin_change(request.revision)?;
    let machine = RemoteConnections::machine(&request)?;
    Ok(RemoteConnections::connect(&machine).await?.1)
}
#[tauri::command(rename = "remote.save")]
pub async fn remoteSave(
    state: tauri::State<'_, RemoteConnections>,
    request: RemoteConnectionCredentials,
) -> Result<RemoteConnectionView, String> {
    let _guard = state.begin_change(request.revision)?;
    let machine = RemoteConnections::machine(&request)?;
    let (client, _) = RemoteConnections::connect(&machine).await?;
    save(&state, machine, client)
}
fn save(
    state: &RemoteConnections,
    machine: RemoteMachineSummary,
    client: Arc<ssh::Client>,
) -> Result<RemoteConnectionView, String> {
    let mut prefs = state.state.lock().expect("connection").prefs.clone();
    prefs.machines.retain(|m| m.id != machine.id);
    prefs.active_machine_id = Some(machine.id.clone());
    prefs.machines.push(machine);
    prefs.target = MachineTarget::Remote;
    state.commit(prefs, Some(client))
}
#[tauri::command(rename = "remote.install")]
pub async fn remoteInstall(
    state: tauri::State<'_, RemoteConnections>,
    request: RemoteConnectionCredentials,
) -> Result<RemoteConnectionView, String> {
    let _guard = state.begin_change(request.revision)?;
    let machine = RemoteConnections::machine(&request)?;
    install::run(&machine.ssh_host, machine.use_sudo).await?;
    let (client, _) = RemoteConnections::connect(&machine).await?;
    save(&state, machine, client)
}
#[tauri::command(rename = "remote.remove")]
pub fn remoteRemove(
    state: tauri::State<'_, RemoteConnections>,
    request: RemoteRevisionRequest,
) -> Result<RemoteConnectionView, String> {
    let _guard = state.begin_change(request.revision)?;
    let mut prefs = state.state.lock().expect("connection").prefs.clone();
    let id = request
        .machine_id
        .or_else(|| prefs.active_machine_id.clone());
    prefs.machines.retain(|m| Some(&m.id) != id.as_ref());
    if prefs.active_machine_id == id {
        prefs.active_machine_id = None;
        prefs.target = MachineTarget::Local;
    }
    let client = if prefs.target == MachineTarget::Remote {
        state.state.lock().expect("connection").client.clone()
    } else {
        None
    };
    state.commit(prefs, client)
}
#[tauri::command(rename = "remote.select")]
pub async fn remoteSelect(
    state: tauri::State<'_, RemoteConnections>,
    request: RemoteSelectionRequest,
) -> Result<RemoteConnectionView, String> {
    let _guard = state.begin_change(request.revision)?;
    let mut prefs = state.state.lock().expect("connection").prefs.clone();
    prefs.target = request.target;
    if let Some(id) = request.machine_id {
        prefs.active_machine_id = Some(id);
    }
    let client = if request.target == MachineTarget::Remote {
        let machine = prefs.active().ok_or("请先选择远程机器")?;
        Some(RemoteConnections::connect(machine).await?.0)
    } else {
        None
    };
    state.commit(prefs, client)
}
#[tauri::command(rename = "remote.invoke")]
pub async fn remoteInvoke(
    state: tauri::State<'_, RemoteConnections>,
    request: RemoteInvokeRequest,
) -> Result<Value, String> {
    if !is_allowed_rpc_command(&request.command) {
        return Err("不支持的远程操作".into());
    }
    let (client, guard) = {
        let mut s = state.state.lock().expect("connection");
        if s.changing
            || s.prefs.revision != request.revision
            || s.prefs.target != MachineTarget::Remote
        {
            return Err("连接已变化，操作已取消".into());
        }
        let client = s.client.clone().ok_or("请重新连接远程机器")?;
        let guard = if is_mutating_rpc_command(&request.command) {
            s.writes += 1;
            Some(LocalWriteGuard(state.state.clone()))
        } else {
            None
        };
        (client, guard)
    };
    let value = client.call(&request.command, request.args).await;
    drop(guard);
    if state.state.lock().expect("connection").prefs.revision != request.revision {
        return Err("连接已变化，已丢弃旧响应".into());
    }
    value
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn saved_ssh_preferences_and_local_write_guard() {
        let root = tempfile::tempdir().unwrap();
        let connection = RemoteConnections::load(root.path()).unwrap();
        let guard = connection.begin_local_write(0).unwrap();
        assert!(connection.begin_change(0).is_err());
        drop(guard);
        let mut prefs = Preferences::default();
        prefs.machines.push(RemoteMachineSummary {
            id: "fixture".into(),
            name: "Fixture".into(),
            ssh_host: "fixture-host".into(),
            use_sudo: true,
        });
        connection.commit(prefs, None).unwrap();
        let restored = RemoteConnections::load(root.path()).unwrap();
        assert_eq!(restored.view().machines[0].ssh_host, "fixture-host");
        assert!(restored.require_local(0).is_err());
    }
}
