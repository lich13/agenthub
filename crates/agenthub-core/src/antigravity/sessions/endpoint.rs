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

fn is_antigravity_app_data_dir_value(value: &str) -> bool {
    let trimmed = value.trim().trim_end_matches('/');
    trimmed == "antigravity"
        || Path::new(trimmed)
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|name| name == "antigravity")
}

pub(crate) fn is_antigravity_language_server_args(args: &[&str]) -> bool {
    let Some(&exe) = args.first() else {
        return false;
    };
    if exe.ends_with("/Antigravity.app/Contents/Resources/bin/language_server") {
        return true;
    }
    let bin = exe.rsplit('/').next().unwrap_or(exe);
    if !(bin == "language_server" || bin.starts_with("language_server_")) {
        return false;
    }
    args.windows(2)
        .any(|w| w[0] == "--app_data_dir" && is_antigravity_app_data_dir_value(w[1]))
        || args.iter().any(|arg| {
            arg.strip_prefix("--app_data_dir=")
                .is_some_and(is_antigravity_app_data_dir_value)
        })
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
    if !crate::platform::pid_is_alive(pid) {
        return Vec::new();
    }
    let mut cmd = crate::platform::sanitized_lsof_command(crate::platform::default_lsof_bin());
    let Ok(lsof) = cmd
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

fn extract_cli_csrf_token<'a>(args: &[&'a str]) -> Option<&'a str> {
    args.windows(2)
        .find(|w| w[0] == "--csrf_token")
        .map(|w| w[1])
        .or_else(|| {
            args.iter()
                .find_map(|arg| arg.strip_prefix("--csrf_token="))
        })
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
    if let Some(token) = extract_cli_csrf_token(args) {
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

fn collect_live_daemon_hub_candidates(
    daemon_dirs: &[PathBuf],
    expected_uid: u32,
    seen_pids: &mut BTreeSet<u32>,
    candidates: &mut Vec<DiscoveredAntigravityEndpoint>,
) {
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
            let Some(pid) = json["pid"]
                .as_u64()
                .and_then(|p| u32::try_from(p).ok())
                .filter(|&p| p > 0 && !seen_pids.contains(&p) && crate::platform::pid_is_alive(p))
            else {
                continue;
            };
            let Some(http_port) = json["httpPort"]
                .as_u64()
                .and_then(|p| u16::try_from(p).ok())
                .filter(|&p| p > 0)
            else {
                continue;
            };
            let Some(token) = json["csrfToken"]
                .as_str()
                .filter(|t| is_valid_csrf_token(t))
            else {
                continue;
            };
            #[cfg(target_os = "linux")]
            let listening = linux_pid_loopback_ports(pid);
            #[cfg(not(target_os = "linux"))]
            let listening = lsof_loopback_ports(pid);
            if listening.contains(&http_port) {
                seen_pids.insert(pid);
                candidates.push(DiscoveredAntigravityEndpoint {
                    pid,
                    ports: vec![http_port],
                    token: token.to_owned(),
                });
            }
        }
    }
}

#[cfg(target_os = "linux")]
pub fn discover_host_endpoints() -> Vec<DiscoveredAntigravityEndpoint> {
    use std::os::unix::fs::MetadataExt;
    // SAFETY: Reading the process UID takes no pointers or arguments.
    let process_uid = unsafe { libc::getuid() };
    let uid = if process_uid == 0 {
        std::env::var_os("ANTIGRAVITY_HOME")
            .filter(|v| !v.is_empty())
            .and_then(|p| fs::symlink_metadata(p).ok())
            .map(|m| m.uid())
            .unwrap_or(process_uid)
    } else {
        process_uid
    };
    let daemon_dirs = candidate_daemon_dirs();
    let mut seen_pids = BTreeSet::new();
    let mut candidates = Vec::new();
    if let Ok(proc_entries) = fs::read_dir("/proc") {
        for entry in proc_entries.flatten() {
            let Some(pid) = entry
                .file_name()
                .to_str()
                .and_then(|s| s.parse::<u32>().ok())
                .filter(|&p| crate::platform::pid_is_alive(p))
            else {
                continue;
            };
            let Ok(meta) = fs::symlink_metadata(entry.path()) else {
                continue;
            };
            if meta.uid() != uid {
                continue;
            }
            let Some(kernel_args) = crate::platform::resolve_kernel_args(pid) else {
                continue;
            };
            let arg_refs: Vec<&str> = kernel_args.iter().map(String::as_str).collect();
            if !is_antigravity_language_server_args(&arg_refs) {
                continue;
            }
            let ports = linux_pid_loopback_ports(pid);
            push_discovered_candidate(
                pid,
                uid,
                &arg_refs,
                ports,
                &daemon_dirs,
                &mut seen_pids,
                &mut candidates,
            );
        }
    }
    collect_live_daemon_hub_candidates(&daemon_dirs, uid, &mut seen_pids, &mut candidates);
    candidates
}

#[cfg(not(target_os = "linux"))]
pub fn discover_host_endpoints() -> Vec<DiscoveredAntigravityEndpoint> {
    use std::process::Command;
    // SAFETY: Reading the process UID takes no pointers or arguments.
    let uid = unsafe { libc::getuid() };
    let daemon_dirs = candidate_daemon_dirs();
    let mut seen_pids = BTreeSet::new();
    let mut candidates = Vec::new();
    if let Ok(output) = Command::new("/bin/ps")
        .env("LC_ALL", "C")
        .args(["-ww", "-axo", "pid=,uid=,command="])
        .output()
    {
        if output.status.success() {
            let text = String::from_utf8_lossy(&output.stdout);
            for line in text.lines() {
                let mut parts = line.split_whitespace();
                let Some(pid) = parts
                    .next()
                    .and_then(|s| s.parse::<u32>().ok())
                    .filter(|&p| crate::platform::pid_is_alive(p))
                else {
                    continue;
                };
                let Some(owner) = parts.next().and_then(|s| s.parse::<u32>().ok()) else {
                    continue;
                };
                if owner != uid || seen_pids.contains(&pid) {
                    continue;
                }
                let ps_args: Vec<&str> = parts.collect();
                let kernel_args = crate::platform::resolve_kernel_args(pid);
                let arg_refs: Vec<&str> = match &kernel_args {
                    Some(kargs) if !kargs.is_empty() => kargs.iter().map(String::as_str).collect(),
                    _ => ps_args,
                };
                if !is_antigravity_language_server_args(&arg_refs) {
                    continue;
                }
                let ports = lsof_loopback_ports(pid);
                push_discovered_candidate(
                    pid,
                    uid,
                    &arg_refs,
                    ports,
                    &daemon_dirs,
                    &mut seen_pids,
                    &mut candidates,
                );
            }
        }
    }
    collect_live_daemon_hub_candidates(&daemon_dirs, uid, &mut seen_pids, &mut candidates);
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

#[cfg(test)]
thread_local! {
    static TEST_RPC_READ_TIMEOUT_MS: std::cell::Cell<Option<u64>> = const { std::cell::Cell::new(None) };
}

#[cfg(test)]
pub(crate) fn with_test_rpc_read_timeout_ms<R>(timeout_ms: u64, f: impl FnOnce() -> R) -> R {
    struct Reset(Option<u64>);
    impl Drop for Reset {
        fn drop(&mut self) {
            TEST_RPC_READ_TIMEOUT_MS.with(|slot| slot.set(self.0));
        }
    }
    let prev = TEST_RPC_READ_TIMEOUT_MS.with(|slot| {
        let old = slot.get();
        slot.set(Some(timeout_ms));
        old
    });
    let _reset = Reset(prev);
    f()
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
    #[cfg(test)]
    let read_timeout = TEST_RPC_READ_TIMEOUT_MS
        .with(|slot| slot.get())
        .map_or(Duration::from_secs(5), Duration::from_millis);
    #[cfg(not(test))]
    let read_timeout = Duration::from_secs(5);
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_millis(1500))
        .context("无法连接本机 Antigravity 服务")?;
    stream.set_read_timeout(Some(read_timeout))?;
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

type CachedEndpointSlot = (
    Option<Vec<DiscoveredAntigravityEndpoint>>,
    std::time::Instant,
);

static CACHED_LIVE_RPC: std::sync::Mutex<Option<CachedEndpointSlot>> = std::sync::Mutex::new(None);

const POSITIVE_ENDPOINT_CACHE_TTL: Duration = Duration::from_secs(3);
const NEGATIVE_ENDPOINT_CACHE_TTL: Duration = Duration::from_secs(5);

pub fn get_cached_live_endpoints() -> Option<Result<Vec<DiscoveredAntigravityEndpoint>, ()>> {
    let (entry, ts) = CACHED_LIVE_RPC.lock().ok().and_then(|g| g.clone())?;
    match entry {
        Some(eps) if ts.elapsed() < POSITIVE_ENDPOINT_CACHE_TTL && !eps.is_empty() => Some(Ok(eps)),
        None if ts.elapsed() < NEGATIVE_ENDPOINT_CACHE_TTL => Some(Err(())),
        _ => None,
    }
}

pub fn get_cached_live_endpoint() -> Option<Result<DiscoveredAntigravityEndpoint, ()>> {
    match get_cached_live_endpoints() {
        Some(Ok(mut eps)) if !eps.is_empty() => Some(Ok(eps.remove(0))),
        Some(Err(())) => Some(Err(())),
        _ => None,
    }
}

pub fn record_verified_live_endpoints(endpoints: Vec<DiscoveredAntigravityEndpoint>) {
    if endpoints.is_empty() {
        return;
    }
    if let Ok(mut guard) = CACHED_LIVE_RPC.lock() {
        *guard = Some((Some(endpoints), std::time::Instant::now()));
    }
}

pub fn record_verified_live_endpoint(pid: u32, port: u16, token: String) {
    record_verified_live_endpoints(vec![DiscoveredAntigravityEndpoint {
        pid,
        ports: vec![port],
        token,
    }]);
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
            crate::note_cache_eviction();
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

pub(crate) fn port_closed(error: &anyhow::Error) -> bool {
    use std::io::ErrorKind;
    error.downcast_ref::<std::io::Error>().is_some_and(|io| {
        matches!(
            io.kind(),
            ErrorKind::ConnectionRefused | ErrorKind::NotConnected
        )
    })
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
fn should_prefer_live_summary(candidate: &Value, existing: &Value) -> bool {
    let cand_active = super::live_item_is_running(candidate);
    let exist_active = super::live_item_is_running(existing);
    if cand_active != exist_active {
        return cand_active;
    }
    let ts_of = |item: &Value| {
        item["lastModifiedTime"]
            .as_str()
            .or_else(|| item["updatedTime"].as_str())
            .or_else(|| item["createdTime"].as_str())
            .and_then(super::parse_any_timestamp_ms)
            .unwrap_or(i64::MIN)
    };
    ts_of(candidate) > ts_of(existing)
}

#[derive(Clone, Default)]
pub(crate) struct LiveEndpointSet {
    pub(crate) instances: Vec<(LiveRpc, Value)>,
    pub(crate) merged_payload: Option<Value>,
    owners: BTreeMap<String, Vec<usize>>,
    pub(crate) probe_incomplete_reason: Option<String>,
}

impl LiveEndpointSet {
    pub(crate) fn from_instances(instances: Vec<(LiveRpc, Value)>) -> Self {
        Self::from_probe(instances, None)
    }

    pub(crate) fn from_probe(
        instances: Vec<(LiveRpc, Value)>,
        probe_incomplete_reason: Option<String>,
    ) -> Self {
        if instances.is_empty() {
            return Self {
                instances,
                merged_payload: None,
                owners: BTreeMap::new(),
                probe_incomplete_reason,
            };
        }
        let mut owners: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        if instances.len() == 1 {
            if let Some(map) = instances[0].1["trajectorySummaries"].as_object() {
                for id in map.keys() {
                    owners.insert(id.clone(), vec![0]);
                }
            }
            let merged_payload = Some(instances[0].1.clone());
            return Self {
                instances,
                merged_payload,
                owners,
                probe_incomplete_reason,
            };
        }
        let mut merged_map = serde_json::Map::new();
        for (idx, (_, payload)) in instances.iter().enumerate() {
            let Some(map) = payload["trajectorySummaries"].as_object() else {
                continue;
            };
            for (id, item) in map {
                owners.entry(id.clone()).or_default().push(idx);
                match merged_map.get(id) {
                    Some(existing) if !should_prefer_live_summary(item, existing) => {}
                    _ => {
                        merged_map.insert(id.clone(), item.clone());
                    }
                }
            }
        }
        Self {
            instances,
            merged_payload: Some(json!({ "trajectorySummaries": merged_map })),
            owners,
            probe_incomplete_reason,
        }
    }

    pub(crate) fn is_complete(&self) -> bool {
        self.probe_incomplete_reason.is_none()
    }

    pub(crate) fn owner_for_read(&self, id: &str) -> Option<&(LiveRpc, Value)> {
        if let Some(indices) = self.owners.get(id) {
            let mut best: Option<&(LiveRpc, Value)> = None;
            for &idx in indices {
                if let Some(candidate) = self.instances.get(idx) {
                    match best {
                        Some((_, prev_val))
                            if !should_prefer_live_summary(
                                &candidate.1["trajectorySummaries"][id],
                                &prev_val["trajectorySummaries"][id],
                            ) => {}
                        _ => best = Some(candidate),
                    }
                }
            }
            return best;
        }
        if self.instances.len() == 1 && self.is_complete() {
            self.instances.first()
        } else {
            None
        }
    }

    pub(crate) fn has_multi_instance_conflict<'a>(
        &self,
        target_ids: impl IntoIterator<Item = &'a str>,
    ) -> bool {
        let mut unique = BTreeSet::new();
        for id in target_ids {
            if let Some(indices) = self.owners.get(id) {
                for &idx in indices {
                    unique.insert(idx);
                    if unique.len() > 1 {
                        return true;
                    }
                }
            }
        }
        false
    }

    pub(crate) fn resolve_mutation_owner<'a>(
        &self,
        target_ids: impl IntoIterator<Item = &'a str>,
    ) -> Result<Option<(LiveRpc, Value)>> {
        if let Some(reason) = &self.probe_incomplete_reason {
            bail!("{reason}");
        }
        let mut unique = BTreeSet::new();
        for id in target_ids {
            if let Some(indices) = self.owners.get(id) {
                for &idx in indices {
                    unique.insert(idx);
                }
            }
        }
        if unique.len() > 1 {
            bail!("该会话同时在多个 Antigravity 实例中加载，请先关闭重复窗口后再操作");
        }
        if let Some(&idx) = unique.iter().next() {
            return Ok(self.instances.get(idx).cloned());
        }
        Ok(self.instances.first().cloned())
    }
}

#[cfg(test)]
thread_local! {
    static TEST_AUTO_DISCOVER_CANDIDATES: std::cell::RefCell<Option<Vec<DiscoveredAntigravityEndpoint>>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(crate) fn with_test_auto_discover_candidates<R>(
    candidates: Vec<DiscoveredAntigravityEndpoint>,
    f: impl FnOnce() -> R,
) -> R {
    struct Reset(Option<Vec<DiscoveredAntigravityEndpoint>>);
    impl Drop for Reset {
        fn drop(&mut self) {
            TEST_AUTO_DISCOVER_CANDIDATES.with(|slot| *slot.borrow_mut() = self.0.take());
        }
    }
    let prev = TEST_AUTO_DISCOVER_CANDIDATES.with(|slot| slot.replace(Some(candidates)));
    let _reset = Reset(prev);
    f()
}

pub(crate) fn connect_all_endpoints(paths: &AntigravityPaths) -> Result<LiveEndpointSet> {
    connect_all_endpoints_inner(paths, true)
}

pub(crate) fn connect_all_endpoints_for_mutation(
    paths: &AntigravityPaths,
) -> Result<LiveEndpointSet> {
    connect_all_endpoints_inner(paths, false)
}

fn connect_all_endpoints_inner(
    paths: &AntigravityPaths,
    use_cache: bool,
) -> Result<LiveEndpointSet> {
    let probe_endpoint = |port: u16, token: &str| match connect_rpc_post(
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
            if port_closed(&error) {
                Ok(None)
            } else {
                Err(error)
            }
        }
    };
    match &paths.rpc_mode {
        RpcMode::OfflineOnly => Ok(LiveEndpointSet::default()),
        #[cfg(test)]
        RpcMode::Fixed { port, token } => match probe_endpoint(*port, token) {
            Ok(Some(value)) => Ok(LiveEndpointSet::from_instances(vec![(
                LiveRpc {
                    port: *port,
                    token: token.clone(),
                },
                value,
            )])),
            Ok(None) => Ok(LiveEndpointSet::default()),
            Err(error) => {
                if service_unreachable(&error) {
                    Ok(LiveEndpointSet::from_probe(
                        Vec::new(),
                        Some(format!(
                            "Antigravity 实例未响应或连接中断（端口 {port}），无法确认会话归属：{error}"
                        )),
                    ))
                } else {
                    Err(error)
                }
            }
        },
        #[cfg(test)]
        RpcMode::FixedMany(list) => {
            let mut instances = Vec::new();
            let mut incomplete_reason: Option<String> = None;
            for (port, token) in list {
                match probe_endpoint(*port, token) {
                    Ok(Some(value)) => {
                        instances.push((
                            LiveRpc {
                                port: *port,
                                token: token.clone(),
                            },
                            value,
                        ));
                    }
                    Ok(None) => {}
                    Err(err) => {
                        incomplete_reason.get_or_insert_with(|| {
                            format!(
                                "部分 Antigravity 实例未响应或返回异常（端口 {port}），无法确认会话归属：{err}"
                            )
                        });
                    }
                }
            }
            Ok(LiveEndpointSet::from_probe(instances, incomplete_reason))
        }
        RpcMode::AutoDiscover => {
            if use_cache {
                match get_cached_live_endpoints() {
                    Some(Ok(cached)) => {
                        let mut instances = Vec::new();
                        let mut all_alive = true;
                        for ep in cached {
                            if ep.pid > 0 && !crate::platform::pid_is_alive(ep.pid) {
                                all_alive = false;
                                break;
                            }
                            let mut connected = false;
                            for &port in &ep.ports {
                                if let Ok(Some(value)) = probe_endpoint(port, &ep.token) {
                                    instances.push((
                                        LiveRpc {
                                            port,
                                            token: ep.token.clone(),
                                        },
                                        value,
                                    ));
                                    connected = true;
                                    break;
                                }
                            }
                            if !connected {
                                all_alive = false;
                                break;
                            }
                        }
                        if all_alive && !instances.is_empty() {
                            return Ok(LiveEndpointSet::from_instances(instances));
                        }
                    }
                    Some(Err(())) => return Ok(LiveEndpointSet::default()),
                    None => {}
                }
            }
            #[cfg(test)]
            let candidates = TEST_AUTO_DISCOVER_CANDIDATES
                .with(|slot| slot.borrow().clone())
                .unwrap_or_else(discover_host_endpoints);
            #[cfg(not(test))]
            let candidates = discover_host_endpoints();
            let mut instances = Vec::new();
            let mut verified = Vec::new();
            let mut incomplete_reason: Option<String> = None;
            for ep in &candidates {
                if ep.pid > 0 && !crate::platform::pid_is_alive(ep.pid) {
                    continue;
                }
                let mut ep_connected = false;
                let mut last_err: Option<anyhow::Error> = None;
                for &port in &ep.ports {
                    match probe_endpoint(port, &ep.token) {
                        Ok(Some(value)) => {
                            verified.push(DiscoveredAntigravityEndpoint {
                                pid: ep.pid,
                                ports: vec![port],
                                token: ep.token.clone(),
                            });
                            instances.push((
                                LiveRpc {
                                    port,
                                    token: ep.token.clone(),
                                },
                                value,
                            ));
                            ep_connected = true;
                            break;
                        }
                        Ok(None) => {}
                        Err(err) => {
                            last_err = Some(err);
                        }
                    }
                }
                if !ep_connected && (ep.pid == 0 || crate::platform::pid_is_alive(ep.pid)) {
                    incomplete_reason.get_or_insert_with(|| match last_err {
                        Some(err) => format!(
                            "部分运行中的 Antigravity 实例未响应或返回异常（PID {}），无法确认会话归属：{err}",
                            ep.pid
                        ),
                        None => format!(
                            "部分运行中的 Antigravity 实例尚未就绪（PID {}），无法确认会话归属，请稍后重试",
                            ep.pid
                        ),
                    });
                }
            }
            if !instances.is_empty() && incomplete_reason.is_none() {
                record_verified_live_endpoints(verified);
            } else {
                record_endpoint_failure(!candidates.is_empty() || incomplete_reason.is_some());
            }
            Ok(LiveEndpointSet::from_probe(instances, incomplete_reason))
        }
    }
}

pub(crate) fn connect_endpoint(paths: &AntigravityPaths) -> Result<Option<(LiveRpc, Value)>> {
    let mut set = connect_all_endpoints(paths)?;
    if set.instances.is_empty() {
        return Ok(None);
    }
    let first_rpc = set.instances.remove(0).0;
    let payload = set
        .merged_payload
        .unwrap_or_else(|| json!({ "trajectorySummaries": {} }));
    Ok(Some((first_rpc, payload)))
}
