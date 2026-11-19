#![allow(non_snake_case)]

#[path = "../../../src-tauri/src/remote/install.rs"]
mod install;
#[path = "../../../src-tauri/src/remote/ssh.rs"]
mod ssh;
#[path = "../../../src-tauri/src/remote/ssh_config.rs"]
mod ssh_config;

use agenthub_core::services::{
    commands::{is_allowed_rpc_command, is_mutating_rpc_command},
    remote::*,
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

type ConnectionAttempt = Arc<tokio::sync::OnceCell<Result<Arc<ssh::Client>, String>>>;

#[derive(Default)]
struct ConnectionState {
    prefs: Preferences,
    writes: usize,
    changing: bool,
    client: Option<Arc<ssh::Client>>,
    connecting: Option<ConnectionAttempt>,
    closed: bool,
}

#[derive(Clone)]
pub struct RemoteConnections {
    state: Arc<Mutex<ConnectionState>>,
    path: PathBuf,
}

impl RemoteConnections {
    pub fn load(data_dir: &Path) -> Result<Self, String> {
        fs::create_dir_all(data_dir).map_err(|_| "无法创建连接配置目录")?;
        let path = data_dir.join("remote-connection.json");
        let prefs = match fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|_| "连接配置损坏")?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Preferences::default(),
            Err(_) => return Err("无法读取连接配置".into()),
        };
        Ok(Self {
            state: Arc::new(Mutex::new(ConnectionState {
                prefs,
                ..Default::default()
            })),
            path,
        })
    }

    pub(crate) fn disconnect(&self) {
        let mut state = self.state.lock().expect("connection");
        state.closed = true;
        state.connecting = None;
        if let Some(client) = state.client.take() {
            client.close();
        }
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

    pub(crate) fn view(&self) -> RemoteConnectionView {
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

    pub(crate) fn is_current(&self, revision: u64) -> bool {
        let state = self.state.lock().expect("connection");
        !state.closed && !state.changing && state.prefs.revision == revision
    }

    fn persist(&self, prefs: &Preferences) -> Result<(), String> {
        let mut f = tempfile::Builder::new()
            .prefix("remote-connection.json.")
            .tempfile_in(self.path.parent().ok_or("保存连接配置失败")?)
            .map_err(|_| "保存连接配置失败")?;
        let result = (|| -> anyhow::Result<()> {
            f.write_all(&serde_json::to_vec(prefs)?)?;
            f.as_file().sync_all()?;
            f.persist(&self.path)?;
            Ok(())
        })();
        result.map_err(|_| "保存连接配置失败".into())
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
        {
            let mut s = self.state.lock().expect("connection");
            if s.closed {
                return Err("连接已关闭，操作已取消".into());
            }
            self.persist(&prefs)?;
            if let Some(previous) = s.client.take() {
                if !client
                    .as_ref()
                    .is_some_and(|next| Arc::ptr_eq(&previous, next))
                {
                    previous.close();
                }
            }
            s.connecting = None;
            s.prefs = prefs;
            s.client = client;
        }
        Ok(self.view())
    }

    async fn reconnect(machine: RemoteMachineSummary) -> Result<Arc<ssh::Client>, String> {
        Self::connect(&machine).await.map(|(client, _)| client)
    }

    async fn ensure_client(&self, revision: u64) -> Result<Arc<ssh::Client>, String> {
        self.ensure_client_with(revision, Self::reconnect).await
    }

    async fn ensure_client_with<F, Fut>(
        &self,
        revision: u64,
        connect: F,
    ) -> Result<Arc<ssh::Client>, String>
    where
        F: FnOnce(RemoteMachineSummary) -> Fut,
        Fut: std::future::Future<Output = Result<Arc<ssh::Client>, String>>,
    {
        let (attempt, machine) = {
            let mut s = self.state.lock().expect("connection");
            if s.closed
                || s.changing
                || s.prefs.revision != revision
                || s.prefs.target != MachineTarget::Remote
            {
                return Err("连接已变化，操作已取消".into());
            }
            if let Some(client) = s.client.as_ref().filter(|c| c.is_connected()) {
                return Ok(client.clone());
            }
            if let Some(client) = s.client.take() {
                client.close();
            }
            let machine = s.prefs.active().cloned().ok_or("请先选择远程机器")?;
            let attempt = s
                .connecting
                .get_or_insert_with(|| Arc::new(tokio::sync::OnceCell::new()))
                .clone();
            (attempt, machine)
        };
        let result = attempt.get_or_init(|| connect(machine)).await.clone();
        let mut s = self.state.lock().expect("connection");
        if s.connecting
            .as_ref()
            .is_some_and(|pending| Arc::ptr_eq(pending, &attempt))
        {
            s.connecting = None;
        }
        if s.closed
            || s.changing
            || s.prefs.revision != revision
            || s.prefs.target != MachineTarget::Remote
        {
            if let Ok(client) = result {
                client.close();
            }
            return Err("连接已变化，操作已取消".into());
        }
        let client = result?;
        if !client.is_connected() {
            return Err("SSH 连接已断开，请稍后重试".into());
        }
        s.client = Some(client.clone());
        Ok(client)
    }

    fn forget_client(&self, failed: &Arc<ssh::Client>) {
        let mut s = self.state.lock().expect("connection");
        if s.client
            .as_ref()
            .is_some_and(|client| Arc::ptr_eq(client, failed))
        {
            s.client = None;
        }
        failed.close();
    }

    pub(crate) async fn invoke(&self, request: &RemoteInvokeRequest) -> Result<Value, String> {
        self.invoke_with(request, Self::reconnect).await
    }

    async fn invoke_with<F, Fut>(
        &self,
        request: &RemoteInvokeRequest,
        connect: F,
    ) -> Result<Value, String>
    where
        F: Fn(RemoteMachineSummary) -> Fut + Clone,
        Fut: std::future::Future<Output = Result<Arc<ssh::Client>, String>>,
    {
        let mut client = self
            .ensure_client_with(request.revision, connect.clone())
            .await?;
        let result = client.call(&request.command, request.args.clone()).await;
        match result {
            Err(error) if error.is_transport() => {
                self.forget_client(&client);
                if is_mutating_rpc_command(&request.command) {
                    return Err(error.to_string());
                }
                client = self.ensure_client_with(request.revision, connect).await?;
                let result = client.call(&request.command, request.args.clone()).await;
                if result.as_ref().is_err_and(|error| error.is_transport()) {
                    self.forget_client(&client);
                }
                result.map_err(|error| error.to_string())
            }
            result => result.map_err(|error| error.to_string()),
        }
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

pub async fn remoteGet(state: &RemoteConnections) -> Result<RemoteConnectionView, String> {
    let view = state.view();
    if view.target == MachineTarget::Remote {
        let state = state.clone();
        let revision = view.revision;
        tokio::spawn(async move {
            let _ = state.ensure_client(revision).await;
        });
    }
    Ok(view)
}

pub async fn remoteSshHosts() -> Result<SshHostList, String> {
    tokio::task::spawn_blocking(ssh_config::list)
        .await
        .map_err(|_| "SSH 配置读取失败")?
}

pub async fn remoteSshDelete(
    state: &RemoteConnections,
    request: SshDeleteRequest,
) -> Result<RemoteConnectionView, String> {
    let _guard = state.begin_change(request.revision)?;
    let state = state.clone();
    tokio::task::spawn_blocking(move || {
        ssh_config::delete_with(&request.alias, || {
            let mut prefs = state.state.lock().expect("connection").prefs.clone();
            prefs
                .machines
                .retain(|machine| machine.ssh_host != request.alias);
            if prefs.active().is_none() {
                prefs.active_machine_id = None;
                prefs.target = MachineTarget::Local;
            }
            let client = (prefs.target == MachineTarget::Remote)
                .then(|| state.state.lock().expect("connection").client.clone())
                .flatten();
            state.commit(prefs, client)
        })
    })
    .await
    .map_err(|_| "SSH 配置删除失败")?
}

pub async fn remoteVerify(
    state: &RemoteConnections,
    request: RemoteConnectionCredentials,
) -> Result<SystemCapabilitiesResponse, String> {
    let _guard = state.begin_change(request.revision)?;
    let machine = RemoteConnections::machine(&request)?;
    Ok(RemoteConnections::connect(&machine).await?.1)
}

pub async fn remoteSave(
    state: &RemoteConnections,
    request: RemoteConnectionCredentials,
) -> Result<RemoteConnectionView, String> {
    let _guard = state.begin_change(request.revision)?;
    let machine = RemoteConnections::machine(&request)?;
    let (client, _) = RemoteConnections::connect(&machine).await?;
    save(state, machine, client)
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

pub async fn remoteInstall(
    state: &RemoteConnections,
    request: RemoteConnectionCredentials,
) -> Result<RemoteConnectionView, String> {
    let _guard = state.begin_change(request.revision)?;
    let machine = RemoteConnections::machine(&request)?;
    install::run(&machine.ssh_host, machine.use_sudo).await?;
    let (client, _) = RemoteConnections::connect(&machine).await?;
    save(state, machine, client)
}

pub fn remoteRemove(
    state: &RemoteConnections,
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

pub async fn remoteSelect(
    state: &RemoteConnections,
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

pub async fn remoteInvoke(
    state: &RemoteConnections,
    request: RemoteInvokeRequest,
) -> Result<Value, String> {
    if !is_allowed_rpc_command(&request.command) {
        return Err("不支持的远程操作".into());
    }
    let guard = {
        let mut s = state.state.lock().expect("connection");
        if s.changing
            || s.prefs.revision != request.revision
            || s.prefs.target != MachineTarget::Remote
        {
            return Err("连接已变化，操作已取消".into());
        }
        if is_mutating_rpc_command(&request.command) {
            s.writes += 1;
            Some(LocalWriteGuard(state.state.clone()))
        } else {
            None
        }
    };
    let value = state.invoke(&request).await;
    drop(guard);
    if state.state.lock().expect("connection").prefs.revision != request.revision {
        return Err("连接已变化，已丢弃旧响应".into());
    }
    value
}

#[cfg(test)]
#[path = "remote_tests.rs"]
mod tests;
