#![allow(non_snake_case)]

use agenthub_core::services::{
    commands::{is_allowed_rpc_command, is_mutating_rpc_command},
    remote::{
        credentials::{CredentialAccess, CredentialAccessError},
        *,
    },
    system::{HostSurface, SystemCapabilitiesResponse, API_VERSION},
};
use reqwest::{header, redirect::Policy, Client, Url};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const KEYCHAIN_SERVICE: &str = "com.lich13.agenthub.remote";
const LEGACY_MACHINE_ID: &str = "m-legacy";
use agenthub_core::resource_limits::{
    CREDENTIAL_WAIT, REMOTE_RESPONSE_BYTES as MAX_RESPONSE_BYTES,
};

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Preferences {
    target: MachineTarget,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    active_machine_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    machines: Vec<RemoteMachineSummary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    base_url: Option<String>,
    revision: u64,
}

impl Preferences {
    fn normalize_and_migrate(&mut self) -> Result<(), String> {
        for machine in &mut self.machines {
            if machine.id.trim().is_empty() || machine.id.chars().any(char::is_control) {
                return Err(safe_error("连接配置损坏"));
            }
            machine.base_url = normalized_base_url(&machine.base_url)?;
            machine.name = normalized_machine_name(Some(&machine.name), &machine.base_url)?;
        }
        if self.machines.is_empty() {
            if let Some(legacy_base) = self.base_url.take() {
                let normalized = normalized_base_url(&legacy_base)?;
                let name = default_machine_name(&normalized);
                self.machines.push(RemoteMachineSummary {
                    id: LEGACY_MACHINE_ID.to_string(),
                    name,
                    base_url: normalized,
                });
                self.active_machine_id = Some(LEGACY_MACHINE_ID.to_string());
            }
        } else {
            self.base_url = None;
        }

        if self.machines.is_empty() {
            self.active_machine_id = None;
            if self.target == MachineTarget::Remote {
                self.target = MachineTarget::Local;
            }
        } else {
            let valid_active = self
                .active_machine_id
                .as_deref()
                .is_some_and(|id| self.machines.iter().any(|m| m.id == id));
            if !valid_active {
                self.active_machine_id = self.machines.first().map(|m| m.id.clone());
            }
        }
        Ok(())
    }

    fn active_machine(&self) -> Option<&RemoteMachineSummary> {
        match self.active_machine_id.as_deref() {
            Some(id) => self.machines.iter().find(|m| m.id == id),
            None => self.machines.first(),
        }
    }
}

#[derive(Default)]
struct ConnectionState {
    prefs: Preferences,
    writes: usize,
    changing: bool,
    verified_revision: Option<u64>,
}

#[derive(Clone)]
pub struct RemoteConnections {
    state: Arc<Mutex<ConnectionState>>,
    path: PathBuf,
    http: Client,
    credentials: CredentialAccess,
}

fn safe_error(message: &'static str) -> String {
    message.into()
}

fn credential_access_error(error: CredentialAccessError) -> String {
    safe_error(match error {
        CredentialAccessError::TimedOut => "钥匙串未及时响应，请检查系统授权后重试",
        CredentialAccessError::WorkerUnavailable => "钥匙串操作失败，请重试",
    })
}

pub fn normalized_base_url(raw: &str) -> Result<String, String> {
    let url = Url::parse(raw.trim()).map_err(|_| safe_error("请输入有效的 HTTPS 地址"))?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || raw.contains('\\')
        || raw.chars().any(char::is_control)
    {
        return Err(safe_error(
            "地址必须使用 HTTPS，且不能包含凭据、查询参数或片段",
        ));
    }
    Ok(format!("{}/", url.as_str().trim_end_matches('/')))
}

pub fn default_machine_name(normalized_base: &str) -> String {
    Url::parse(normalized_base)
        .ok()
        .and_then(|url| {
            let host = url.host_str()?;
            match url.port() {
                Some(port) => Some(format!("{host}:{port}")),
                None => Some(host.to_string()),
            }
        })
        .unwrap_or_else(|| "远程机器".to_string())
}

pub fn normalized_machine_name(raw: Option<&str>, normalized_base: &str) -> Result<String, String> {
    let Some(raw) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(default_machine_name(normalized_base));
    };
    if raw.chars().count() > 64 || raw.chars().any(char::is_control) {
        return Err(safe_error("机器名称不能超过 64 个字符，且不能包含控制字符"));
    }
    Ok(raw.to_string())
}

fn validate_api_key_format(api_key: &str) -> Result<(), String> {
    if api_key.len() != 68
        || !api_key.starts_with("nhk_")
        || !api_key[4..].bytes().all(|c| c.is_ascii_hexdigit())
    {
        return Err(safe_error("API Key 格式无效"));
    }
    Ok(())
}

fn machine_credential_account(machine_id: &str) -> String {
    format!("machine:{machine_id}")
}

fn machine_credential(machine_id: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new(KEYCHAIN_SERVICE, &machine_credential_account(machine_id))
        .map_err(|_| safe_error("无法访问钥匙串"))
}

fn legacy_credential(base_url: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new(KEYCHAIN_SERVICE, base_url).map_err(|_| safe_error("无法访问钥匙串"))
}

fn read_credential(entry: &keyring::Entry) -> Result<Option<String>, String> {
    match entry.get_password() {
        Ok(value) => Ok(Some(value)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(_) => Err(safe_error("无法读取钥匙串")),
    }
}

fn restore_credential(entry: &keyring::Entry, value: Option<&str>) -> Result<(), String> {
    match value {
        Some(value) => entry.set_password(value),
        None => entry.delete_credential().or_else(|error| {
            if matches!(error, keyring::Error::NoEntry) {
                Ok(())
            } else {
                Err(error)
            }
        }),
    }
    .map_err(|_| safe_error("无法更新钥匙串，请检查钥匙串权限"))
}

fn read_or_migrate_machine_credential(
    machine_id: &str,
    base_url: &str,
) -> Result<Option<String>, String> {
    let primary = machine_credential(machine_id)?;
    if let Some(value) = read_credential(&primary)? {
        return Ok(Some(value));
    }
    let legacy = legacy_credential(base_url)?;
    if let Some(value) = read_credential(&legacy)? {
        if primary.set_password(&value).is_ok() {
            let _ = restore_credential(&legacy, None);
        }
        return Ok(Some(value));
    }
    Ok(None)
}

impl RemoteConnections {
    pub(crate) fn credential_access(&self) -> CredentialAccess {
        self.credentials.clone()
    }

    pub(crate) fn require_local(&self, revision: u64) -> Result<(), String> {
        let current = self.state.lock().expect("connection state");
        if current.changing
            || current.prefs.revision != revision
            || current.prefs.target != MachineTarget::Local
        {
            return Err(safe_error("连接已变化，操作已取消"));
        }
        Ok(())
    }

    pub(crate) fn begin_local_write(&self, revision: u64) -> Result<LocalWriteGuard, String> {
        let mut current = self.state.lock().expect("connection state");
        if current.changing
            || current.prefs.revision != revision
            || current.prefs.target != MachineTarget::Local
        {
            return Err(safe_error("连接已变化，操作已取消"));
        }
        current.writes += 1;
        Ok(LocalWriteGuard(self.state.clone()))
    }

    pub fn load(data_dir: &Path) -> Result<Self, String> {
        fs::create_dir_all(data_dir).map_err(|_| safe_error("无法创建连接配置目录"))?;
        let path = data_dir.join("remote-connection.json");
        let prefs = if path.exists() {
            let mut value: Preferences = serde_json::from_slice(
                &fs::read(&path).map_err(|_| safe_error("无法读取连接配置"))?,
            )
            .map_err(|_| safe_error("连接配置损坏"))?;
            value.normalize_and_migrate()?;
            value
        } else {
            Preferences::default()
        };
        let http = Client::builder()
            .redirect(Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(60))
            .build()
            .map_err(|_| safe_error("无法初始化远程连接"))?;
        Ok(Self {
            state: Arc::new(Mutex::new(ConnectionState {
                prefs,
                ..Default::default()
            })),
            path,
            http,
            credentials: CredentialAccess::default(),
        })
    }

    fn view(&self) -> RemoteConnectionView {
        let state = self.state.lock().expect("connection state");
        let active = state.prefs.active_machine();
        RemoteConnectionView {
            target: state.prefs.target,
            revision: state.prefs.revision,
            base_url: active.map(|m| m.base_url.clone()),
            configured: !state.prefs.machines.is_empty(),
            active_machine_id: active.map(|m| m.id.clone()),
            active_machine_name: active.map(|m| m.name.clone()),
            machines: state.prefs.machines.clone(),
        }
    }

    fn persist(&self, prefs: &Preferences) -> Result<(), String> {
        let temp = self.path.with_extension("json.pending");
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let result = (|| {
            let mut clean = prefs.clone();
            clean.base_url = None;
            let mut file = options.open(&temp)?;
            file.write_all(&serde_json::to_vec(&clean)?)?;
            file.sync_all()?;
            fs::rename(&temp, &self.path)?;
            Ok::<_, anyhow::Error>(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        result.map_err(|_| safe_error("保存连接配置失败"))
    }

    fn begin_change(&self, revision: u64) -> Result<ChangeGuard, String> {
        let mut state = self.state.lock().expect("connection state");
        if state.prefs.revision != revision {
            return Err(safe_error("连接已变化，请重试"));
        }
        if state.writes > 0 || state.changing {
            return Err(safe_error("操作进行中，请稍后切换"));
        }
        state.changing = true;
        Ok(ChangeGuard(self.state.clone()))
    }

    fn require_current_remote(
        &self,
        revision: u64,
        machine_id: &str,
        base: &str,
    ) -> Result<(), String> {
        let current = self.state.lock().expect("connection state");
        let active = current.prefs.active_machine();
        if current.changing
            || current.prefs.revision != revision
            || current.prefs.target != MachineTarget::Remote
            || active.map(|m| (m.id.as_str(), m.base_url.as_str())) != Some((machine_id, base))
        {
            return Err(safe_error("连接已变化，操作已取消"));
        }
        Ok(())
    }

    async fn call(
        &self,
        base: &str,
        key: &str,
        command: &str,
        args: &Value,
    ) -> Result<Value, String> {
        if !is_allowed_rpc_command(command) {
            return Err(safe_error("不支持的远程操作"));
        }
        let mut key =
            header::HeaderValue::from_str(key).map_err(|_| safe_error("API Key 格式无效"))?;
        key.set_sensitive(true);
        let endpoint = format!("{base}api/rpc/{command}");
        let mut response = self
            .http
            .post(endpoint)
            .header("x-api-key", key)
            .json(args)
            .send()
            .await
            .map_err(|_| safe_error("无法连接远程机器，请检查地址和网络"))?;
        let status = response.status();
        if status.is_redirection() {
            return Err(safe_error("服务器返回重定向，请填写最终 HTTPS 地址"));
        }
        if status == 401 || status == 403 {
            return Err(safe_error("API Key 无效或已撤销"));
        }
        if status == 429 {
            return Err(safe_error("鉴权请求过于频繁，请稍后重试"));
        }
        if !response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| {
                v.split(';')
                    .next()
                    .is_some_and(|mime| mime.trim() == "application/json")
            })
        {
            return Err(safe_error("远程地址未返回 AgentHub API 数据"));
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
        {
            return Err(safe_error("远程响应超出大小限制"));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| safe_error("远程响应读取失败"))?
        {
            if bytes.len() + chunk.len() > MAX_RESPONSE_BYTES {
                return Err(safe_error("远程响应超出大小限制"));
            }
            bytes.extend_from_slice(&chunk);
        }
        let value: Value =
            serde_json::from_slice(&bytes).map_err(|_| safe_error("远程响应格式无效"))?;
        if !status.is_success() {
            return Err(value
                .get("error")
                .and_then(Value::as_str)
                .map(agenthub_core::security::redact_output)
                .unwrap_or_else(|| format!("远程操作失败，HTTP {}", status.as_u16())));
        }
        Ok(value)
    }

    async fn verify_endpoint(
        &self,
        base: &str,
        api_key: &str,
    ) -> Result<SystemCapabilitiesResponse, String> {
        validate_api_key_format(api_key)?;
        let value = self
            .call(base, api_key, "system.capabilities", &json!({}))
            .await?;
        compatible_capabilities(value)
    }

    async fn verify(
        &self,
        request: &RemoteConnectionCredentials,
    ) -> Result<SystemCapabilitiesResponse, String> {
        let base = normalized_base_url(&request.base_url)?;
        self.verify_endpoint(&base, &request.api_key).await
    }

    fn resolve_target_machine_for_save(
        &self,
        requested_id: Option<&str>,
        normalized_base: &str,
    ) -> Result<(String, Option<RemoteMachineSummary>), String> {
        let state = self.state.lock().expect("connection state");
        if let Some(id) = requested_id.map(str::trim).filter(|s| !s.is_empty()) {
            let existing = state
                .prefs
                .machines
                .iter()
                .find(|m| m.id == id)
                .cloned()
                .ok_or_else(|| safe_error("未找到要编辑的远程机器"))?;
            return Ok((existing.id.clone(), Some(existing)));
        }
        if let Some(existing) = state
            .prefs
            .machines
            .iter()
            .find(|m| m.base_url == normalized_base)
            .cloned()
        {
            return Ok((existing.id.clone(), Some(existing)));
        }
        Ok((next_machine_id(&state.prefs.machines), None))
    }

    fn apply_saved_profile_in_prefs(
        &self,
        machine_id: String,
        name: String,
        base: String,
    ) -> Result<RemoteConnectionView, String> {
        let mut state = self.state.lock().expect("connection state");
        let mut next = state.prefs.clone();
        if let Some(existing) = next.machines.iter_mut().find(|m| m.id == machine_id) {
            existing.name = name;
            existing.base_url = base;
        } else {
            next.machines.push(RemoteMachineSummary {
                id: machine_id.clone(),
                name,
                base_url: base,
            });
        }
        next.active_machine_id = Some(machine_id);
        next.target = MachineTarget::Remote;
        next.base_url = None;
        next.revision = next.revision.wrapping_add(1);
        self.persist(&next)?;
        state.prefs = next;
        state.verified_revision = Some(state.prefs.revision);
        drop(state);
        Ok(self.view())
    }

    // Both methods run as one serialized blocking transaction. Their caller's
    // ChangeGuard stays in that worker through commit or rollback, even if the
    // frontend stops awaiting the command.
    fn save_connection(
        &self,
        machine_id: String,
        name: String,
        base: String,
        previous_base: Option<String>,
        new_api_key: Option<String>,
    ) -> Result<RemoteConnectionView, String> {
        let entry = machine_credential(&machine_id)?;
        let old_key = read_credential(&entry)?;
        let legacy_entry = previous_base
            .as_deref()
            .map(legacy_credential)
            .transpose()?;
        let old_legacy_key = legacy_entry
            .as_ref()
            .map(read_credential)
            .transpose()?
            .flatten();

        if let Some(api_key) = &new_api_key {
            entry
                .set_password(api_key)
                .map_err(|_| safe_error("无法保存 API Key 到钥匙串"))?;
            if let Some(legacy) = &legacy_entry {
                let _ = restore_credential(legacy, None);
            }
        } else if old_key.is_none() {
            if let Some(migrated) = old_legacy_key.as_deref() {
                entry
                    .set_password(migrated)
                    .map_err(|_| safe_error("无法保存 API Key 到钥匙串"))?;
                if let Some(legacy) = &legacy_entry {
                    let _ = restore_credential(legacy, None);
                }
            } else {
                return Err(safe_error("未找到已有 API Key，请重新填写"));
            }
        }

        match self.apply_saved_profile_in_prefs(machine_id, name, base) {
            Ok(view) => Ok(view),
            Err(error) => {
                restore_credential(&entry, old_key.as_deref())?;
                if let Some(legacy) = &legacy_entry {
                    restore_credential(legacy, old_legacy_key.as_deref())?;
                }
                Err(error)
            }
        }
    }

    fn select_machine(
        &self,
        target: MachineTarget,
        requested_machine_id: Option<&str>,
    ) -> Result<RemoteConnectionView, String> {
        let mut next = self.state.lock().expect("connection state").prefs.clone();
        match target {
            MachineTarget::Local => {
                next.target = MachineTarget::Local;
            }
            MachineTarget::Remote => {
                if next.machines.is_empty() {
                    return Err(safe_error("请先设置远程连接"));
                }
                if let Some(id) = requested_machine_id
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                {
                    if !next.machines.iter().any(|m| m.id == id) {
                        return Err(safe_error("未找到指定的远程机器"));
                    }
                    next.active_machine_id = Some(id.to_string());
                } else if next.active_machine_id.is_none() {
                    next.active_machine_id = next.machines.first().map(|m| m.id.clone());
                }
                next.target = MachineTarget::Remote;
            }
        }
        next.revision = next.revision.wrapping_add(1);
        self.persist(&next)?;
        let mut state = self.state.lock().expect("connection state");
        state.prefs = next;
        state.verified_revision = None;
        drop(state);
        Ok(self.view())
    }

    fn remove_machine_from_prefs(&self, removed_id: &str) -> Result<RemoteConnectionView, String> {
        let mut state = self.state.lock().expect("connection state");
        let mut next = state.prefs.clone();
        let was_active = next.active_machine().map(|m| m.id.as_str()) == Some(removed_id);
        let before = next.machines.len();
        next.machines.retain(|m| m.id != removed_id);
        if next.machines.len() == before {
            return Err(safe_error("未找到指定的远程机器"));
        }
        if next.machines.is_empty() {
            next.active_machine_id = None;
            next.target = MachineTarget::Local;
        } else if was_active {
            next.active_machine_id = next.machines.first().map(|m| m.id.clone());
            next.target = MachineTarget::Local;
        }
        next.base_url = None;
        next.revision = next.revision.wrapping_add(1);
        self.persist(&next)?;
        state.prefs = next;
        if was_active {
            state.verified_revision = None;
        }
        drop(state);
        Ok(self.view())
    }

    fn remove_connection(
        &self,
        requested_id: Option<&str>,
    ) -> Result<RemoteConnectionView, String> {
        let target_machine = {
            let state = self.state.lock().expect("connection state");
            match requested_id.map(str::trim).filter(|s| !s.is_empty()) {
                Some(id) => state.prefs.machines.iter().find(|m| m.id == id).cloned(),
                None => state.prefs.active_machine().cloned(),
            }
        };
        let Some(machine) = target_machine else {
            let mut state = self.state.lock().expect("connection state");
            let mut next = state.prefs.clone();
            next.target = MachineTarget::Local;
            next.active_machine_id = None;
            next.machines.clear();
            next.base_url = None;
            next.revision = next.revision.wrapping_add(1);
            self.persist(&next)?;
            state.prefs = next;
            state.verified_revision = None;
            drop(state);
            return Ok(self.view());
        };

        let entry = machine_credential(&machine.id)?;
        let old_key = read_credential(&entry)?;
        let legacy_entry = legacy_credential(&machine.base_url)?;
        let old_legacy = read_credential(&legacy_entry)?;
        restore_credential(&entry, None)?;
        restore_credential(&legacy_entry, None)?;

        match self.remove_machine_from_prefs(&machine.id) {
            Ok(view) => Ok(view),
            Err(error) => {
                if let Some(key) = old_key {
                    restore_credential(&entry, Some(&key))?;
                }
                if let Some(legacy_key) = old_legacy {
                    restore_credential(&legacy_entry, Some(&legacy_key))?;
                }
                Err(error)
            }
        }
    }
}

fn next_machine_id(existing: &[RemoteMachineSummary]) -> String {
    static NEXT_MACHINE_SEQ: AtomicU64 = AtomicU64::new(1);
    loop {
        let seq = NEXT_MACHINE_SEQ.fetch_add(1, Ordering::Relaxed);
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_micros())
            .unwrap_or(0);
        let candidate = format!("m-{ts:x}-{seq:x}");
        if !existing.iter().any(|m| m.id == candidate) {
            return candidate;
        }
    }
}

fn compatible_capabilities(value: Value) -> Result<SystemCapabilitiesResponse, String> {
    let capabilities: SystemCapabilitiesResponse =
        serde_json::from_value(value).map_err(|_| safe_error("远程 API 协议不兼容"))?;
    if capabilities.api_version != API_VERSION
        || capabilities.host_surface != HostSurface::LinuxServerApi
    {
        return Err(safe_error("远程 API 协议不兼容，请更新服务"));
    }
    Ok(capabilities)
}

struct ChangeGuard(Arc<Mutex<ConnectionState>>);
pub(crate) struct LocalWriteGuard(Arc<Mutex<ConnectionState>>);
impl Drop for LocalWriteGuard {
    fn drop(&mut self) {
        self.0.lock().expect("connection state").writes -= 1;
    }
}
impl Drop for ChangeGuard {
    fn drop(&mut self) {
        self.0.lock().expect("connection state").changing = false;
    }
}
struct WriteGuard<'a>(&'a RemoteConnections, bool);
impl Drop for WriteGuard<'_> {
    fn drop(&mut self) {
        if self.1 {
            self.0.state.lock().expect("connection state").writes -= 1;
        }
    }
}

#[tauri::command(rename = "remote.get")]
pub fn remoteGet(state: tauri::State<'_, RemoteConnections>) -> RemoteConnectionView {
    state.view()
}

#[tauri::command(rename = "remote.verify")]
pub async fn remoteVerify(
    state: tauri::State<'_, RemoteConnections>,
    request: RemoteConnectionCredentials,
) -> Result<SystemCapabilitiesResponse, String> {
    let _guard = state.begin_change(request.revision)?;
    state.verify(&request).await
}

#[tauri::command(rename = "remote.save")]
pub async fn remoteSave(
    state: tauri::State<'_, RemoteConnections>,
    request: RemoteConnectionCredentials,
) -> Result<RemoteConnectionView, String> {
    let guard = state.begin_change(request.revision)?;
    let base = normalized_base_url(&request.base_url)?;
    let name = normalized_machine_name(request.name.as_deref(), &base)?;
    let (machine_id, existing) =
        state.resolve_target_machine_for_save(request.id.as_deref(), &base)?;
    let trimmed_key = request.api_key.trim().to_string();

    let new_api_key = if !trimmed_key.is_empty() {
        state.verify_endpoint(&base, &trimmed_key).await?;
        Some(trimmed_key)
    } else if let Some(prev) = &existing {
        let lookup_id = prev.id.clone();
        let lookup_base = prev.base_url.clone();
        let stored_key = state
            .credentials
            .read(CREDENTIAL_WAIT, move || {
                read_or_migrate_machine_credential(&lookup_id, &lookup_base)?
                    .ok_or_else(|| safe_error("未找到已有 API Key，请重新填写"))
            })
            .await
            .map_err(credential_access_error)??;
        if prev.base_url != base {
            state.verify_endpoint(&base, &stored_key).await?;
        }
        None
    } else {
        return Err(safe_error("API Key 格式无效"));
    };

    let previous_base = existing.map(|m| m.base_url);
    let credentials = state.credentials.clone();
    let connection = state.inner().clone();
    credentials
        .transaction(CREDENTIAL_WAIT, move || {
            let _guard = guard;
            connection.save_connection(machine_id, name, base, previous_base, new_api_key)
        })
        .await
        .map_err(credential_access_error)?
}

#[tauri::command(rename = "remote.remove")]
pub async fn remoteRemove(
    state: tauri::State<'_, RemoteConnections>,
    request: RemoteRevisionRequest,
) -> Result<RemoteConnectionView, String> {
    let guard = state.begin_change(request.revision)?;
    let requested_id = request.machine_id;
    let credentials = state.credentials.clone();
    let connection = state.inner().clone();
    credentials
        .transaction(CREDENTIAL_WAIT, move || {
            let _guard = guard;
            connection.remove_connection(requested_id.as_deref())
        })
        .await
        .map_err(credential_access_error)?
}

#[tauri::command(rename = "remote.select")]
pub fn remoteSelect(
    state: tauri::State<'_, RemoteConnections>,
    request: RemoteSelectionRequest,
) -> Result<RemoteConnectionView, String> {
    let _guard = state.begin_change(request.revision)?;
    state.select_machine(request.target, request.machine_id.as_deref())
}

#[tauri::command(rename = "remote.invoke")]
pub async fn remoteInvoke(
    state: tauri::State<'_, RemoteConnections>,
    request: RemoteInvokeRequest,
) -> Result<RemoteInvokeResponse, String> {
    if !is_allowed_rpc_command(&request.command) {
        return Err(safe_error("不支持的远程操作"));
    }
    let write = is_mutating_rpc_command(&request.command);
    let (machine_id, base) = {
        let mut current = state.state.lock().expect("connection state");
        if current.changing
            || current.prefs.revision != request.revision
            || current.prefs.target != MachineTarget::Remote
        {
            return Err(safe_error("连接已变化，操作已取消"));
        }
        let active = current
            .prefs
            .active_machine()
            .cloned()
            .ok_or_else(|| safe_error("请先设置远程连接"))?;
        if write {
            current.writes += 1;
        }
        (active.id, active.base_url)
    };
    let _guard = WriteGuard(&state, write);
    let credential_id = machine_id.clone();
    let credential_base = base.clone();
    let key = state
        .credentials
        .read(CREDENTIAL_WAIT, move || {
            read_or_migrate_machine_credential(&credential_id, &credential_base)?
                .ok_or_else(|| safe_error("无法读取 API Key，请重新保存远程连接"))
        })
        .await
        .map_err(credential_access_error)??;
    state.require_current_remote(request.revision, &machine_id, &base)?;
    let needs_validation = state
        .state
        .lock()
        .expect("connection state")
        .verified_revision
        != Some(request.revision);
    if needs_validation && request.command != "system.capabilities" {
        compatible_capabilities(
            state
                .call(&base, &key, "system.capabilities", &json!({}))
                .await?,
        )?;
    }
    state.require_current_remote(request.revision, &machine_id, &base)?;
    let value = state
        .call(&base, &key, &request.command, &request.args)
        .await?;
    if request.command == "system.capabilities" {
        compatible_capabilities(value.clone())?;
    }
    let mut current = state.state.lock().expect("connection state");
    let active = current.prefs.active_machine();
    if current.changing
        || current.prefs.revision != request.revision
        || active.map(|m| (m.id.as_str(), m.base_url.as_str()))
            != Some((machine_id.as_str(), base.as_str()))
    {
        return Err(safe_error("连接已变化，已丢弃旧响应"));
    }
    if needs_validation {
        current.verified_revision = Some(request.revision);
    }
    Ok(value)
}

#[cfg(test)]
#[path = "remote/credential_tests.rs"]
mod credential_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::Read, net::TcpListener, thread};

    fn response_server(
        status: &str,
        headers: &str,
        body: String,
    ) -> (String, thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}/", listener.local_addr().unwrap());
        let head = format!(
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n{headers}\r\n",
            body.len()
        );
        let server = thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            let mut chunk = [0u8; 4096];
            loop {
                let size = socket.read(&mut chunk).unwrap();
                assert!(size > 0);
                request.extend_from_slice(&chunk[..size]);
                if let Some(end) = request.windows(4).position(|part| part == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&request[..end]);
                    let length = headers
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length: ")
                                .and_then(|value| value.parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    if request.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            let _ = socket
                .write_all(head.as_bytes())
                .and_then(|_| socket.write_all(body.as_bytes()));
            String::from_utf8(request).unwrap()
        });
        (base, server)
    }

    #[test]
    fn remote_bridge_never_follows_redirect_or_exposes_key_in_errors() {
        let destination = TcpListener::bind("127.0.0.1:0").unwrap();
        destination.set_nonblocking(true).unwrap();
        let (base, server) = response_server(
            "302 Found",
            &format!(
                "Location: http://{}/stolen\r\n",
                destination.local_addr().unwrap()
            ),
            String::new(),
        );
        let dir = tempfile::tempdir().unwrap();
        let state = RemoteConnections::load(dir.path()).unwrap();
        let key = format!("nhk_{}", "a".repeat(64));
        let error = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(state.call(&base, &key, "system.capabilities", &json!({})))
            .unwrap_err();
        assert!(error.contains("重定向"));
        assert!(!error.contains(&key));
        assert!(server
            .join()
            .unwrap()
            .contains(&format!("x-api-key: {key}")));
        assert_eq!(
            destination.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
    }

    #[test]
    fn remote_bridge_covers_twenty_mib_attachments_and_bounds_response_memory() {
        let dir = tempfile::tempdir().unwrap();
        let state = RemoteConnections::load(dir.path()).unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let image_size = (20 * 1024 * 1024usize).div_ceil(3) * 4;
        let (base, server) = response_server(
            "200 OK",
            "Content-Type: application/json\r\n",
            format!("{{\"data\":\"{}\"}}", "A".repeat(image_size)),
        );
        let value = runtime
            .block_on(state.call(&base, "example", "sessions.attachmentRead", &json!({})))
            .unwrap();
        assert_eq!(value["data"].as_str().unwrap().len(), image_size);
        server.join().unwrap();
        let (base, server) = response_server(
            "200 OK",
            "Content-Type: application/json\r\n",
            " ".repeat(MAX_RESPONSE_BYTES + 1),
        );
        assert!(runtime
            .block_on(state.call(&base, "example", "threads.detail", &json!({})))
            .unwrap_err()
            .contains("大小限制"));
        server.join().unwrap();
        assert!(runtime
            .block_on(state.call("invalid", "example", "remote.invoke", &json!({})))
            .unwrap_err()
            .contains("不支持"));
    }

    #[test]
    fn connection_preferences_migrate_legacy_json_and_manage_multiple_named_machines() {
        let dir = tempfile::tempdir().unwrap();
        let legacy_path = dir.path().join("remote-connection.json");
        fs::write(
            &legacy_path,
            serde_json::to_vec(&json!({
                "target": "remote",
                "baseUrl": "https://api.example.com/agenthub",
                "revision": 8
            }))
            .unwrap(),
        )
        .unwrap();

        let loaded = RemoteConnections::load(dir.path()).unwrap();
        let view = loaded.view();
        assert_eq!(view.target, MachineTarget::Remote);
        assert_eq!(view.revision, 8);
        assert!(view.configured);
        assert_eq!(
            view.base_url.as_deref(),
            Some("https://api.example.com/agenthub/")
        );
        assert_eq!(view.active_machine_id.as_deref(), Some(LEGACY_MACHINE_ID));
        assert_eq!(view.active_machine_name.as_deref(), Some("api.example.com"));
        assert_eq!(view.machines.len(), 1);

        // Add a second named machine and rename the first without touching Keychain.
        let view_b = loaded
            .apply_saved_profile_in_prefs(
                "m-second".to_string(),
                "构建节点 B".to_string(),
                "https://build-b.example.com/agenthub/".to_string(),
            )
            .unwrap();
        assert_eq!(view_b.machines.len(), 2);
        assert_eq!(view_b.active_machine_id.as_deref(), Some("m-second"));
        assert_eq!(view_b.active_machine_name.as_deref(), Some("构建节点 B"));
        assert_eq!(
            view_b.base_url.as_deref(),
            Some("https://build-b.example.com/agenthub/")
        );

        let view_renamed = loaded
            .apply_saved_profile_in_prefs(
                LEGACY_MACHINE_ID.to_string(),
                "主力节点 A".to_string(),
                "https://api.example.com/agenthub/".to_string(),
            )
            .unwrap();
        assert_eq!(
            view_renamed.active_machine_id.as_deref(),
            Some(LEGACY_MACHINE_ID)
        );
        assert_eq!(
            view_renamed.active_machine_name.as_deref(),
            Some("主力节点 A")
        );

        // Switch between named machines and reject unknown IDs.
        assert!(loaded
            .select_machine(MachineTarget::Remote, Some("m-nonexistent"))
            .is_err());
        let switched = loaded
            .select_machine(MachineTarget::Remote, Some("m-second"))
            .unwrap();
        assert_eq!(switched.active_machine_id.as_deref(), Some("m-second"));
        assert_eq!(
            switched.base_url.as_deref(),
            Some("https://build-b.example.com/agenthub/")
        );

        // Removing the active machine safely falls back to Local while keeping remaining machines.
        let after_remove = loaded.remove_machine_from_prefs("m-second").unwrap();
        assert_eq!(after_remove.target, MachineTarget::Local);
        assert_eq!(after_remove.machines.len(), 1);
        assert_eq!(
            after_remove.active_machine_id.as_deref(),
            Some(LEGACY_MACHINE_ID)
        );

        let saved_raw = fs::read_to_string(&loaded.path).unwrap();
        assert!(!saved_raw.contains("apiKey"));
        assert!(!saved_raw.contains("nhk_"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&loaded.path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        let invalid = json!({ "api_version": API_VERSION + 1, "host_surface": "linux_server_api", "capabilities": {} });
        assert!(compatible_capabilities(invalid).is_err());
    }

    #[test]
    fn connection_addresses_and_names_validate_cleanly() {
        assert_eq!(
            normalized_base_url("https://api.example.com/agenthub").unwrap(),
            "https://api.example.com/agenthub/"
        );
        assert_eq!(
            normalized_machine_name(None, "https://api.example.com:8443/agenthub/").unwrap(),
            "api.example.com:8443"
        );
        assert_eq!(
            normalized_machine_name(Some("  远程构建机  "), "https://api.example.com/").unwrap(),
            "远程构建机"
        );
        assert!(normalized_machine_name(Some("bad\nname"), "https://api.example.com/").is_err());
        assert!(
            normalized_machine_name(Some(&"x".repeat(65)), "https://api.example.com/").is_err()
        );
        for invalid in [
            "http://api.example.com",
            "https://user.example.com:secret@example.com/",
            "https://example.com/?token=example",
            "file:///tmp/demo",
            "https://example.com/#part",
        ] {
            assert!(normalized_base_url(invalid).is_err());
        }
    }

    #[test]
    fn target_changes_reject_stale_revisions_and_pending_writes() {
        let dir = tempfile::tempdir().unwrap();
        let state = RemoteConnections::load(dir.path()).unwrap();
        assert!(state.begin_change(1).is_err());
        state.state.lock().unwrap().writes = 1;
        assert!(state.begin_change(0).is_err());
        state.state.lock().unwrap().writes = 0;
        let guard = state.begin_change(0).unwrap();
        assert!(state.begin_change(0).is_err());
        drop(guard);
        assert!(state.begin_change(0).is_ok());
    }

    #[test]
    fn local_write_guard_rejects_stale_remote_and_changing_connections() {
        let dir = tempfile::tempdir().unwrap();
        let state = RemoteConnections::load(dir.path()).unwrap();

        assert!(state.begin_local_write(1).is_err());
        {
            let mut current = state.state.lock().unwrap();
            current.prefs.target = MachineTarget::Remote;
            current.prefs.revision = 2;
        }
        assert!(state.begin_local_write(2).is_err());

        {
            let mut current = state.state.lock().unwrap();
            current.prefs.target = MachineTarget::Local;
            current.prefs.revision = 0;
            current.changing = true;
        }
        assert!(state.begin_local_write(0).is_err());

        state.state.lock().unwrap().changing = false;
        let guard = state.begin_local_write(0).unwrap();
        assert_eq!(state.state.lock().unwrap().writes, 1);
        assert!(state.begin_change(0).is_err());

        drop(guard);
        assert_eq!(state.state.lock().unwrap().writes, 0);
        assert!(state.begin_change(0).is_ok());
    }
}

#[cfg(test)]
mod maintenance_tests;
