use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiscoveredAntigravityEndpoint {
    pub pid: u32,
    pub ports: Vec<u16>,
    pub token: String,
}

#[derive(Clone)]
pub(crate) struct LiveRpc {
    pub(crate) port: u16,
    pub(crate) token: String,
}

#[derive(Debug)]
pub(crate) struct ConnectRpcHttpError {
    pub(crate) status_code: u16,
    pub(crate) code: String,
    pub(crate) detail: String,
}

impl ConnectRpcHttpError {
    pub(crate) fn is_not_found(&self) -> bool {
        self.status_code == 404
            || self.code.eq_ignore_ascii_case("not_found")
            || self.detail.to_ascii_lowercase().contains("not found")
            || self.detail.to_ascii_lowercase().contains("not_found")
    }
}

impl std::fmt::Display for ConnectRpcHttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Antigravity RPC 调用失败：{}", self.detail)
    }
}

impl std::error::Error for ConnectRpcHttpError {}

fn is_valid_csrf_token(token: &str) -> bool {
    !token.is_empty()
        && token.len() <= 256
        && token
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
}

fn candidate_daemon_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(custom) = std::env::var_os("ANTIGRAVITY_HOME").filter(|v| !v.is_empty()) {
        dirs.push(PathBuf::from(custom).join("daemon"));
    }
    if let Ok(user_home) = crate::platform::current_home() {
        let default_dir = user_home.join(".gemini/antigravity/daemon");
        if !dirs.contains(&default_dir) {
            dirs.push(default_dir);
        }
    }
    dirs
}

pub(super) fn resolve_daemon_hub_endpoint(
    daemon_dirs: &[PathBuf],
    pid: u32,
    expected_uid: u32,
    listening_ports: &[u16],
) -> Option<(Vec<u16>, String)> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    for dir in daemon_dirs {
        let Ok(dir_meta) = fs::symlink_metadata(dir) else {
            continue;
        };
        if !dir_meta.file_type().is_dir() || dir_meta.uid() != expected_uid {
            continue;
        }
        let Ok(entries) = fs::read_dir(dir) else {
            continue;
        };
        let mut files: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("hub_") && n.ends_with(".json"))
            })
            .collect();
        files.sort_by(|a, b| {
            let ma = fs::symlink_metadata(a).ok().and_then(|m| m.modified().ok());
            let mb = fs::symlink_metadata(b).ok().and_then(|m| m.modified().ok());
            mb.cmp(&ma).then_with(|| b.cmp(a))
        });
        for path in files {
            let Ok(file) = OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
                .open(&path)
            else {
                continue;
            };
            let Ok(meta) = file.metadata() else {
                continue;
            };
            if !meta.is_file()
                || meta.uid() != expected_uid
                || meta.len() == 0
                || meta.len() > 16 * 1024
            {
                continue;
            }
            let mut bytes = Vec::new();
            if file.take(16 * 1024 + 1).read_to_end(&mut bytes).is_err() || bytes.len() > 16 * 1024
            {
                continue;
            }
            let Ok(json) = serde_json::from_slice::<Value>(&bytes) else {
                continue;
            };
            if json["pid"].as_u64() != Some(u64::from(pid)) {
                continue;
            }
            let Some(http_port) = json["httpPort"]
                .as_u64()
                .and_then(|p| u16::try_from(p).ok())
                .filter(|&p| p > 0 && listening_ports.contains(&p))
            else {
                continue;
            };
            let Some(token) = json["csrfToken"]
                .as_str()
                .filter(|t| is_valid_csrf_token(t))
            else {
                continue;
            };
            return Some((vec![http_port], token.to_owned()));
        }
    }
    None
}

fn is_antigravity_language_server_args(args: &[&str]) -> bool {
    let Some(&exe) = args.first() else {
        return false;
    };
    if exe.ends_with("/Antigravity.app/Contents/Resources/bin/language_server") {
        return true;
    }
    let bin = exe.rsplit('/').next().unwrap_or(exe);
    (bin == "language_server" || bin.starts_with("language_server_"))
        && args
            .windows(2)
            .any(|w| w[0] == "--app_data_dir" && w[1] == "antigravity")
}

#[cfg(any(target_os = "linux", test))]
fn is_proc_net_ipv4_loopback_hex(addr_hex: &str) -> bool {
    // 0100007F is 127.0.0.1 in little-endian /proc/net/tcp;
    // 0000000000000000FFFF00000100007F is ::ffff:127.0.0.1 in little-endian /proc/net/tcp6.
    addr_hex.eq_ignore_ascii_case("0100007F")
        || addr_hex.eq_ignore_ascii_case("0000000000000000FFFF00000100007F")
}

#[cfg(any(target_os = "linux", test))]
pub(crate) fn parse_proc_net_tcp_loopback_ports(
    tcp_text: &str,
    socket_inodes: &BTreeSet<u64>,
) -> Vec<u16> {
    if socket_inodes.is_empty() {
        return Vec::new();
    }
    let mut ports = Vec::new();
    for line in tcp_text.lines().skip(1) {
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() < 10 {
            continue;
        }
        // st == 0A means TCP_LISTEN.
        if !cols[3].eq_ignore_ascii_case("0A") {
            continue;
        }
        let Some((addr_hex, port_hex)) = cols[1].split_once(':') else {
            continue;
        };
        if !is_proc_net_ipv4_loopback_hex(addr_hex) {
            continue;
        }
        let Ok(inode) = cols[9].parse::<u64>() else {
            continue;
        };
        if inode == 0 || !socket_inodes.contains(&inode) {
            continue;
        }
        if let Ok(port) = u16::from_str_radix(port_hex, 16) {
            if port > 0 {
                ports.push(port);
            }
        }
    }
    ports.sort_unstable();
    ports.dedup();
    ports
}

#[cfg(target_os = "linux")]
fn linux_pid_loopback_ports(pid: u32) -> Vec<u16> {
    let fd_dir = PathBuf::from(format!("/proc/{pid}/fd"));
    let mut socket_inodes = BTreeSet::new();
    if let Ok(entries) = fs::read_dir(&fd_dir) {
        for entry in entries.flatten() {
            if let Ok(target) = fs::read_link(entry.path()) {
                if let Some(inode) = target
                    .to_str()
                    .and_then(|s| s.strip_prefix("socket:["))
                    .and_then(|s| s.strip_suffix(']'))
                    .and_then(|s| s.parse::<u64>().ok())
                {
                    socket_inodes.insert(inode);
                }
            }
        }
    }
    if !socket_inodes.is_empty() {
        let mut ports = Vec::new();
        for proc_net in [
            format!("/proc/{pid}/net/tcp"),
            format!("/proc/{pid}/net/tcp6"),
        ] {
            if let Ok(tcp_text) = fs::read_to_string(proc_net) {
                ports.extend(parse_proc_net_tcp_loopback_ports(&tcp_text, &socket_inodes));
            }
        }
        ports.sort_unstable();
        ports.dedup();
        if !ports.is_empty() {
            return ports;
        }
    }
    lsof_loopback_ports(pid)
}

fn lsof_loopback_ports(pid: u32) -> Vec<u16> {
    let lsof_bin = if Path::new("/usr/sbin/lsof").is_file() {
        "/usr/sbin/lsof"
    } else if Path::new("/usr/bin/lsof").is_file() {
        "/usr/bin/lsof"
    } else {
        "lsof"
    };
    let Ok(lsof) = std::process::Command::new(lsof_bin)
        .args([
            "-Pan",
            "-p",
            &pid.to_string(),
            "-iTCP",
            "-sTCP:LISTEN",
            "-Fn",
        ])
        .output()
    else {
        return Vec::new();
    };
    if !lsof.status.success() {
        return Vec::new();
    }
    let mut ports: Vec<u16> = String::from_utf8_lossy(&lsof.stdout)
        .lines()
        .filter_map(parse_loopback_listener_port)
        .collect();
    ports.sort_unstable();
    ports.dedup();
    ports
}

fn push_discovered_candidate(
    pid: u32,
    uid: u32,
    args: &[&str],
    ports: Vec<u16>,
    daemon_dirs: &[PathBuf],
    seen_pids: &mut BTreeSet<u32>,
    candidates: &mut Vec<DiscoveredAntigravityEndpoint>,
) {
    if ports.is_empty() || seen_pids.contains(&pid) {
        return;
    }
    let cli_token = args
        .windows(2)
        .find(|w| w[0] == "--csrf_token")
        .map(|w| w[1]);
    if let Some(token) = cli_token {
        if is_valid_csrf_token(token) && ports.len() <= 8 {
            seen_pids.insert(pid);
            candidates.push(DiscoveredAntigravityEndpoint {
                pid,
                ports,
                token: token.to_owned(),
            });
        }
    } else if let Some((hub_ports, token)) =
        resolve_daemon_hub_endpoint(daemon_dirs, pid, uid, &ports)
    {
        seen_pids.insert(pid);
        candidates.push(DiscoveredAntigravityEndpoint {
            pid,
            ports: hub_ports,
            token,
        });
    }
}

#[cfg(target_os = "linux")]
pub fn discover_host_endpoints() -> Vec<DiscoveredAntigravityEndpoint> {
    use std::os::unix::fs::MetadataExt;
    // SAFETY: Reading the process UID takes no pointers or arguments.
    let uid = unsafe { libc::getuid() };
    let daemon_dirs = candidate_daemon_dirs();
    let Ok(proc_entries) = fs::read_dir("/proc") else {
        return Vec::new();
    };
    let mut seen_pids = BTreeSet::new();
    let mut candidates = Vec::new();
    for entry in proc_entries.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|s| s.parse::<u32>().ok())
        else {
            continue;
        };
        let Ok(meta) = fs::symlink_metadata(entry.path()) else {
            continue;
        };
        if meta.uid() != uid {
            continue;
        }
        let mut cmdline = Vec::new();
        let Ok(file) = fs::File::open(entry.path().join("cmdline")) else {
            continue;
        };
        if file.take(32 * 1024).read_to_end(&mut cmdline).is_err() || cmdline.is_empty() {
            continue;
        }
        let args: Vec<&str> = cmdline
            .split(|&b| b == 0)
            .filter(|s| !s.is_empty())
            .filter_map(|s| std::str::from_utf8(s).ok())
            .collect();
        if !is_antigravity_language_server_args(&args) {
            continue;
        }
        let ports = linux_pid_loopback_ports(pid);
        push_discovered_candidate(
            pid,
            uid,
            &args,
            ports,
            &daemon_dirs,
            &mut seen_pids,
            &mut candidates,
        );
    }
    candidates
}

#[cfg(not(target_os = "linux"))]
pub fn discover_host_endpoints() -> Vec<DiscoveredAntigravityEndpoint> {
    use std::process::Command;
    let Ok(output) = Command::new("/bin/ps")
        .args(["-ww", "-axo", "pid=,uid=,command="])
        .output()
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    // SAFETY: Reading the process UID takes no pointers or arguments.
    let uid = unsafe { libc::getuid() };
    let text = String::from_utf8_lossy(&output.stdout);
    let daemon_dirs = candidate_daemon_dirs();
    let mut seen_pids = BTreeSet::new();
    let mut candidates = Vec::new();
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        let Some(pid) = parts.next().and_then(|s| s.parse::<u32>().ok()) else {
            continue;
        };
        let Some(owner) = parts.next().and_then(|s| s.parse::<u32>().ok()) else {
            continue;
        };
        if owner != uid || seen_pids.contains(&pid) {
            continue;
        };
        let args: Vec<&str> = parts.collect();
        if !is_antigravity_language_server_args(&args) {
            continue;
        }
        let ports = lsof_loopback_ports(pid);
        push_discovered_candidate(
            pid,
            uid,
            &args,
            ports,
            &daemon_dirs,
            &mut seen_pids,
            &mut candidates,
        );
    }
    candidates
}

pub(crate) fn parse_loopback_listener_port(line: &str) -> Option<u16> {
    let (host, port) = line.strip_prefix('n')?.rsplit_once(':')?;
    let host = host
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or(host);
    let is_v4_loopback = host.parse::<Ipv4Addr>().ok() == Some(Ipv4Addr::LOCALHOST)
        || host
            .parse::<std::net::Ipv6Addr>()
            .ok()
            .and_then(|v6| v6.to_ipv4_mapped())
            == Some(Ipv4Addr::LOCALHOST);
    is_v4_loopback.then(|| port.parse::<u16>().ok()).flatten()
}

pub(crate) fn connect_rpc_post(
    port: u16,
    token: &str,
    method: &str,
    body: &Value,
) -> Result<Value> {
    ensure!(
        matches!(
            method,
            "GetAllCascadeTrajectories"
                | "GetCascadeTrajectory"
                | "GetCascadeTrajectorySteps"
                | "UpdateConversationAnnotations"
                | "ForceStopCascadeTree"
                | "DeleteCascadeTrajectory"
        ),
        "不支持的 Antigravity RPC 方法"
    );
    let payload = serde_json::to_vec(body)?;
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_millis(1500))
        .context("无法连接本机 Antigravity 服务")?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    let request = format!(
        "POST {SERVICE_PREFIX}{method} HTTP/1.1\r\n\
         Host: 127.0.0.1:{port}\r\n\
         Content-Type: application/json\r\n\
         Accept: application/json\r\n\
         Accept-Encoding: identity\r\n\
         Connect-Protocol-Version: 1\r\n\
         x-codeium-csrf-token: {token}\r\n\
         Connection: close\r\n\
         Content-Length: {}\r\n\r\n",
        payload.len()
    );
    stream.write_all(request.as_bytes())?;
    stream.write_all(&payload)?;
    stream.flush()?;

    let mut reader = BufReader::new(stream);
    let mut status_line = String::new();
    reader.read_line(&mut status_line)?;
    let status_code = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse::<u16>().ok())
        .context("Antigravity HTTP 响应头无效")?;
    let mut content_length: Option<usize> = None;
    let mut chunked = false;
    loop {
        let mut header = String::new();
        let read = reader.read_line(&mut header)?;
        if read == 0 || header == "\r\n" || header == "\n" {
            break;
        }
        if let Some((name, val)) = header.split_once(':') {
            let name = name.trim();
            let val = val.trim();
            if name.eq_ignore_ascii_case("content-length") {
                let len = val.parse::<usize>().context("无效的 Content-Length")?;
                ensure!(len <= RESPONSE_LIMIT, "Antigravity 响应超过读取上限");
                content_length = Some(len);
            } else if name.eq_ignore_ascii_case("transfer-encoding")
                && val.to_ascii_lowercase().contains("chunked")
            {
                chunked = true;
            }
        }
    }
    let body_bytes = if chunked {
        read_chunked_body(&mut reader, RESPONSE_LIMIT)?
    } else if let Some(len) = content_length {
        let mut buf = vec![0u8; len];
        reader.read_exact(&mut buf)?;
        buf
    } else {
        let mut buf = Vec::new();
        reader
            .take(RESPONSE_LIMIT as u64 + 1)
            .read_to_end(&mut buf)?;
        ensure!(buf.len() <= RESPONSE_LIMIT, "Antigravity 响应超过读取上限");
        buf
    };
    if !(200..300).contains(&status_code) {
        let parsed = serde_json::from_slice::<Value>(&body_bytes).ok();
        let code = parsed
            .as_ref()
            .and_then(|v| v["code"].as_str())
            .unwrap_or("")
            .to_owned();
        let detail = parsed
            .and_then(|v| v["message"].as_str().map(str::to_owned))
            .unwrap_or_else(|| format!("HTTP {status_code}"));
        return Err(ConnectRpcHttpError {
            status_code,
            code,
            detail,
        }
        .into());
    }
    if body_bytes.iter().all(u8::is_ascii_whitespace) {
        return Ok(json!({}));
    }
    serde_json::from_slice(&body_bytes).context("Antigravity 返回了无效 JSON")
}

fn read_chunked_body<R: BufRead>(reader: &mut R, limit: usize) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    loop {
        let mut size_line = String::new();
        ensure!(reader.read_line(&mut size_line)? > 0, "分块响应意外结束");
        let hex_part = size_line.trim().split(';').next().unwrap_or("").trim();
        let chunk_size = usize::from_str_radix(hex_part, 16).context("无效的分块大小")?;
        if chunk_size == 0 {
            break;
        }
        ensure!(
            out.len().saturating_add(chunk_size) <= limit,
            "Antigravity 响应超过读取上限"
        );
        let offset = out.len();
        out.resize(offset + chunk_size, 0);
        reader.read_exact(&mut out[offset..])?;
        let mut crlf = [0u8; 2];
        reader.read_exact(&mut crlf)?;
    }
    Ok(out)
}

type CachedEndpointSlot = (Option<DiscoveredAntigravityEndpoint>, std::time::Instant);

static CACHED_LIVE_RPC: std::sync::Mutex<Option<CachedEndpointSlot>> = std::sync::Mutex::new(None);

pub fn get_cached_live_endpoint() -> Option<Result<DiscoveredAntigravityEndpoint, ()>> {
    let (entry, ts) = CACHED_LIVE_RPC.lock().ok().and_then(|g| g.clone())?;
    match entry {
        Some(ep) => Some(Ok(ep)),
        None if ts.elapsed() < Duration::from_secs(5) => Some(Err(())),
        None => None,
    }
}

pub fn record_verified_live_endpoint(pid: u32, port: u16, token: String) {
    if let Ok(mut guard) = CACHED_LIVE_RPC.lock() {
        *guard = Some((
            Some(DiscoveredAntigravityEndpoint {
                pid,
                ports: vec![port],
                token,
            }),
            std::time::Instant::now(),
        ));
    }
}

pub fn record_endpoint_failure(had_candidate_processes: bool) {
    if let Ok(mut guard) = CACHED_LIVE_RPC.lock() {
        if had_candidate_processes {
            *guard = None;
        } else {
            *guard = Some((None, std::time::Instant::now()));
        }
    }
}

pub fn evict_idle_endpoint_cache(max_idle: Duration) {
    if let Ok(mut guard) = CACHED_LIVE_RPC.lock() {
        if guard
            .as_ref()
            .is_some_and(|(_, ts)| ts.elapsed() >= max_idle)
        {
            *guard = None;
        }
    }
    super::cli_meta::evict_idle_meta_cache(max_idle);
    super::evict_idle_file_caches(max_idle);
}

#[derive(Debug)]
pub(crate) struct LiveHistoryUnavailable;

impl std::fmt::Display for LiveHistoryUnavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("原生历史服务不可达或暂无在线内容")
    }
}

impl std::error::Error for LiveHistoryUnavailable {}

pub(crate) fn fetch_live_trajectory_steps(rpc: &LiveRpc, id: &str) -> Result<(Value, Vec<Value>)> {
    let mut resp = connect_rpc_post(
        rpc.port,
        &rpc.token,
        "GetCascadeTrajectory",
        &json!({"cascadeId": id}),
    )
    .map_err(|error| {
        if service_unreachable(&error) {
            anyhow::Error::new(LiveHistoryUnavailable)
        } else {
            error
        }
    })?;
    if resp.is_object() && resp.get("trajectory").is_none() {
        return Err(LiveHistoryUnavailable.into());
    }
    ensure!(
        resp["trajectory"]["cascadeId"].as_str() == Some(id),
        "原生历史身份不匹配"
    );
    let mut steps = match resp["trajectory"].get_mut("steps") {
        Some(value) => value
            .as_array_mut()
            .map(std::mem::take)
            .context("原生历史步骤格式错误")?,
        None => Vec::new(),
    };
    let total = match resp.get("numTotalSteps") {
        Some(value) => value.as_u64().context("原生历史总数格式错误")? as usize,
        None => steps.len(),
    };
    if steps.is_empty() && total == 0 {
        return Err(LiveHistoryUnavailable.into());
    }
    ensure!(!steps.is_empty(), "原生历史读取不完整，请重试");
    while steps.len() < total {
        let mut more = connect_rpc_post(
            rpc.port,
            &rpc.token,
            "GetCascadeTrajectorySteps",
            &json!({"cascadeId": id, "stepOffset": steps.len()}),
        )
        .map_err(|error| {
            if service_unreachable(&error) {
                anyhow::Error::new(LiveHistoryUnavailable)
            } else {
                error
            }
        })?;
        if let Some(cascade_id) = more.get("cascadeId") {
            ensure!(cascade_id.as_str() == Some(id), "原生历史身份不匹配");
        }
        let batch = more["steps"]
            .as_array_mut()
            .context("原生历史分页格式错误")?;
        ensure!(!batch.is_empty(), "原生历史读取不完整，请重试");
        steps.append(batch);
    }
    Ok((resp, steps))
}

pub(crate) fn service_unreachable(error: &anyhow::Error) -> bool {
    use std::io::ErrorKind;
    error.downcast_ref::<std::io::Error>().is_some_and(|io| {
        matches!(
            io.kind(),
            ErrorKind::ConnectionRefused
                | ErrorKind::ConnectionReset
                | ErrorKind::ConnectionAborted
                | ErrorKind::TimedOut
                | ErrorKind::WouldBlock
                | ErrorKind::NotConnected
                | ErrorKind::BrokenPipe
        )
    })
}
pub(crate) fn connect_endpoint(paths: &AntigravityPaths) -> Result<Option<(LiveRpc, Value)>> {
    let try_endpoint = |port: u16, token: &str| match connect_rpc_post(
        port,
        token,
        "GetAllCascadeTrajectories",
        &json!({"excludeSubtrajectories": false}),
    ) {
        Ok(mut value) => {
            if value.is_object() && value.get("trajectorySummaries").is_none() {
                value["trajectorySummaries"] = json!({});
            }
            if value["trajectorySummaries"].is_object() {
                Ok(Some(value))
            } else {
                Err(anyhow::anyhow!("Antigravity 会话列表格式错误"))
            }
        }
        Err(error) => {
            if service_unreachable(&error) {
                Ok(None)
            } else {
                Err(error)
            }
        }
    };
    match &paths.rpc_mode {
        RpcMode::OfflineOnly => Ok(None),
        #[cfg(test)]
        RpcMode::Fixed { port, token } => {
            if let Some(value) = try_endpoint(*port, token)? {
                return Ok(Some((
                    LiveRpc {
                        port: *port,
                        token: token.clone(),
                    },
                    value,
                )));
            }
            Ok(None)
        }
        RpcMode::AutoDiscover => {
            match get_cached_live_endpoint() {
                Some(Ok(ep)) => {
                    for &port in &ep.ports {
                        if let Some(value) = try_endpoint(port, &ep.token).ok().flatten() {
                            return Ok(Some((
                                LiveRpc {
                                    port,
                                    token: ep.token,
                                },
                                value,
                            )));
                        }
                    }
                }
                Some(Err(())) => return Ok(None),
                None => {}
            }
            let candidates = discover_host_endpoints();
            for ep in &candidates {
                for &port in &ep.ports {
                    if let Some(value) = try_endpoint(port, &ep.token).ok().flatten() {
                        record_verified_live_endpoint(ep.pid, port, ep.token.clone());
                        return Ok(Some((
                            LiveRpc {
                                port,
                                token: ep.token.clone(),
                            },
                            value,
                        )));
                    }
                }
            }
            record_endpoint_failure(!candidates.is_empty());
            Ok(None)
        }
    }
}
