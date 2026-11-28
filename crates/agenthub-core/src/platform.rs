use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlatformKind {
    Linux,
    Macos,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlatformPaths {
    pub kind: PlatformKind,
    pub data_dir: PathBuf,
    pub config_file: PathBuf,

    pub log_dir: PathBuf,
    pub service_name: String,
    pub service_kind: String,
    pub service_file: Option<PathBuf>,
}

impl PlatformKind {
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::Macos
        } else {
            Self::Linux
        }
    }
}

pub(crate) fn current_home() -> anyhow::Result<PathBuf> {
    dirs::home_dir()
        .filter(|p| p.is_absolute())
        .ok_or_else(|| anyhow::anyhow!("cannot resolve user home directory"))
}

pub(crate) fn provider_root(variable: &str, relative: &str) -> anyhow::Result<PathBuf> {
    match std::env::var_os(variable).filter(|value| !value.is_empty()) {
        Some(root) => Ok(PathBuf::from(root)),
        None => Ok(current_home()?.join(relative)),
    }
}

impl PlatformPaths {
    pub fn current() -> anyhow::Result<Self> {
        match PlatformKind::current() {
            PlatformKind::Linux => Ok(Self::linux_server()),
            PlatformKind::Macos => Self::desktop_current(),
        }
    }

    pub fn desktop_current() -> anyhow::Result<Self> {
        Ok(Self::macos(current_home()?))
    }

    pub fn linux_server() -> Self {
        Self {
            kind: PlatformKind::Linux,
            data_dir: PathBuf::from("/var/lib/agenthub-webd"),
            config_file: PathBuf::from("/etc/agenthub-webd/config.toml"),

            log_dir: PathBuf::from("/var/log/agenthub-webd"),
            service_name: "agenthub-webd".to_string(),
            service_kind: "systemd".to_string(),
            service_file: Some(PathBuf::from("/etc/systemd/system/agenthub-webd.service")),
        }
    }

    pub fn macos(home: impl Into<PathBuf>) -> Self {
        let home = home.into();
        Self {
            kind: PlatformKind::Macos,
            data_dir: home.join("Library/Application Support/AgentHub"),
            config_file: home.join("Library/Application Support/AgentHub/config.toml"),

            log_dir: home.join("Library/Logs/AgentHub"),
            service_name: "AgentHub.app".to_string(),
            // Protocol 2 service kind consumed by older Apps.
            service_kind: "tauri".to_string(),
            service_file: None,
        }
    }

    pub fn daemon_binary(&self) -> PathBuf {
        let binary = "agenthub-webd";
        if self.kind == PlatformKind::Linux && self.service_kind == "systemd" {
            return PathBuf::from("/usr/local/bin").join(binary);
        }
        self.data_dir.join("bin").join(binary)
    }
}

pub(crate) fn same_canonical_dir(left: &std::path::Path, right: &std::path::Path) -> bool {
    left == right
        || matches!(
            (std::fs::canonicalize(left), std::fs::canonicalize(right)),
            (Ok(left), Ok(right)) if left == right
        )
}

pub(crate) fn resolve_kernel_exe(pid: u32) -> Option<PathBuf> {
    #[cfg(target_os = "linux")]
    {
        let target = std::fs::read_link(format!("/proc/{pid}/exe")).ok()?;
        if let Some(stripped) = target.to_str().and_then(|s| s.strip_suffix(" (deleted)")) {
            return Some(PathBuf::from(stripped));
        }
        Some(target)
    }
    #[cfg(target_os = "macos")]
    {
        let mut buf = [0_u8; 4096];
        // SAFETY: `buf` is writable for `buf.len()` bytes and `pid` is validated to fit `i32`.
        let count = unsafe {
            libc::proc_pidpath(
                pid.try_into().ok()?,
                buf.as_mut_ptr().cast(),
                buf.len() as u32,
            )
        };
        if count <= 0 {
            return None;
        }
        let end = buf.iter().position(|b| *b == 0).unwrap_or(count as usize);
        std::str::from_utf8(&buf[..end]).ok().map(PathBuf::from)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = pid;
        None
    }
}

pub fn resolve_kernel_args(pid: u32) -> Option<Vec<String>> {
    #[cfg(target_os = "macos")]
    {
        let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid.try_into().ok()?];
        let mut size = 0usize;
        // SAFETY: The three-element MIB is live; a null output buffer requests the length in `size`.
        if unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                3,
                std::ptr::null_mut(),
                &mut size,
                std::ptr::null_mut(),
                0,
            )
        } != 0
            || !(4..=262_144).contains(&size)
        {
            return None;
        }
        let mut buffer = vec![0_u8; size];
        // SAFETY: `buffer` has `size` writable bytes matching the kernel-reported capacity.
        if unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                3,
                buffer.as_mut_ptr().cast(),
                &mut size,
                std::ptr::null_mut(),
                0,
            )
        } != 0
        {
            return None;
        }
        buffer.truncate(size);
        let argc = i32::from_ne_bytes(buffer.get(..4)?.try_into().ok()?);
        if !(1..=16_384).contains(&argc) {
            return None;
        }
        let data = buffer.get(4..)?;
        let mut offset = data.iter().position(|byte| *byte == 0)?;
        while data.get(offset) == Some(&0) {
            offset += 1;
        }
        let mut args = Vec::with_capacity((argc as usize).min(256));
        for _ in 0..argc {
            let remaining = data.get(offset..)?;
            let end = remaining.iter().position(|byte| *byte == 0)?;
            args.push(std::str::from_utf8(&remaining[..end]).ok()?.to_owned());
            offset += end + 1;
        }
        Some(args)
    }
    #[cfg(target_os = "linux")]
    {
        use std::io::Read;
        let mut bytes = Vec::new();
        std::fs::File::open(format!("/proc/{pid}/cmdline"))
            .ok()?
            .take(262_145)
            .read_to_end(&mut bytes)
            .ok()?;
        if bytes.is_empty() || bytes.len() > 262_144 || !bytes.ends_with(&[0]) {
            return None;
        }
        bytes[..bytes.len() - 1]
            .split(|byte| *byte == 0)
            .map(|arg| std::str::from_utf8(arg).map(str::to_owned))
            .collect::<Result<Vec<_>, _>>()
            .ok()
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = pid;
        None
    }
}

pub(crate) fn resolve_kernel_process_identity(pid: u32) -> Option<(PathBuf, Vec<String>)> {
    Some((resolve_kernel_exe(pid)?, resolve_kernel_args(pid)?))
}

#[cfg(target_os = "macos")]
fn decode_vip_path(vip_path: &[[libc::c_char; 32]; 32]) -> Option<PathBuf> {
    let len = std::mem::size_of_val(vip_path);
    // SAFETY: `vip_path` is a contiguous `[[c_char; 32]; 32]` (1024-byte) array in memory.
    let bytes = unsafe { std::slice::from_raw_parts(vip_path.as_ptr().cast::<u8>(), len) };
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(len);
    if end == 0 {
        return None;
    }
    let s = std::str::from_utf8(&bytes[..end]).ok()?;
    s.starts_with('/').then(|| PathBuf::from(s))
}

pub(crate) fn resolve_kernel_cwd(pid: u32) -> Option<PathBuf> {
    #[cfg(target_os = "linux")]
    {
        std::fs::read_link(format!("/proc/{pid}/cwd")).ok()
    }
    #[cfg(target_os = "macos")]
    {
        let pid_i32: libc::c_int = pid.try_into().ok()?;
        let mut info: libc::proc_vnodepathinfo = unsafe { std::mem::zeroed() };
        let expected = std::mem::size_of::<libc::proc_vnodepathinfo>() as libc::c_int;
        // SAFETY: `info` is zero-initialized and writable for `expected` bytes.
        let count = unsafe {
            libc::proc_pidinfo(
                pid_i32,
                libc::PROC_PIDVNODEPATHINFO,
                0,
                (&mut info as *mut libc::proc_vnodepathinfo).cast(),
                expected,
            )
        };
        if count < expected {
            return None;
        }
        decode_vip_path(&info.pvi_cdir.vip_path)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = pid;
        None
    }
}

#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Clone, Copy)]
struct ProcFileInfo {
    fi_openflags: u32,
    fi_status: u32,
    fi_offset: libc::off_t,
    fi_type: i32,
    fi_guardflags: u32,
}

#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Clone, Copy)]
struct VnodeFdInfoWithPath {
    pfi: ProcFileInfo,
    pvip: libc::vnode_info_path,
}

#[cfg(target_os = "macos")]
const PROC_PIDFDVNODEPATHINFO: libc::c_int = 2;

pub(crate) type KernelOpenVnodes = (
    std::collections::HashSet<PathBuf>,
    std::collections::HashSet<(u64, u64)>,
);

pub(crate) fn collect_kernel_open_vnodes(pid: u32) -> Option<KernelOpenVnodes> {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::MetadataExt;
        let entries = std::fs::read_dir(format!("/proc/{pid}/fd")).ok()?;
        let mut paths = std::collections::HashSet::new();
        let mut inodes = std::collections::HashSet::new();
        for entry in entries.flatten().take(4096) {
            let fd_path = entry.path();
            let Ok(meta) = std::fs::metadata(&fd_path) else {
                continue;
            };
            if !meta.is_file() {
                continue;
            }
            inodes.insert((meta.dev(), meta.ino()));
            if let Some(target) = std::fs::read_link(&fd_path)
                .ok()
                .filter(|p| p.is_absolute())
            {
                paths.insert(target);
            }
        }
        Some((paths, inodes))
    }
    #[cfg(target_os = "macos")]
    {
        let pid_i32: libc::c_int = pid.try_into().ok()?;
        // SAFETY: Null buffer queries required byte size for `PROC_PIDLISTFDS`.
        let needed = unsafe {
            libc::proc_pidinfo(pid_i32, libc::PROC_PIDLISTFDS, 0, std::ptr::null_mut(), 0)
        };
        if needed <= 0 {
            return None;
        }
        let fd_size = std::mem::size_of::<libc::proc_fdinfo>();
        let cap = ((needed as usize) / fd_size + 32).min(8192);
        let mut fds: Vec<libc::proc_fdinfo> = vec![unsafe { std::mem::zeroed() }; cap];
        // SAFETY: `fds` holds `cap * fd_size` writable bytes.
        let bytes = unsafe {
            libc::proc_pidinfo(
                pid_i32,
                libc::PROC_PIDLISTFDS,
                0,
                fds.as_mut_ptr().cast(),
                (cap * fd_size) as libc::c_int,
            )
        };
        if bytes < 0 {
            return None;
        }
        let count = (bytes as usize) / fd_size;
        let mut paths = std::collections::HashSet::new();
        let mut inodes = std::collections::HashSet::new();
        let info_size = std::mem::size_of::<VnodeFdInfoWithPath>() as libc::c_int;
        for fd in fds.iter().take(count) {
            if fd.proc_fdtype != libc::PROX_FDTYPE_VNODE as u32 {
                continue;
            }
            let mut info: VnodeFdInfoWithPath = unsafe { std::mem::zeroed() };
            // SAFETY: `info` is zero-initialized and writable for `info_size` bytes.
            let got = unsafe {
                libc::proc_pidfdinfo(
                    pid_i32,
                    fd.proc_fd,
                    PROC_PIDFDVNODEPATHINFO,
                    (&mut info as *mut VnodeFdInfoWithPath).cast(),
                    info_size,
                )
            };
            if got >= info_size {
                let st = &info.pvip.vip_vi.vi_stat;
                if st.vst_ino != 0 {
                    inodes.insert((u64::from(st.vst_dev), st.vst_ino));
                }
                if let Some(p) = decode_vip_path(&info.pvip.vip_path) {
                    paths.insert(p);
                }
            }
        }
        Some((paths, inodes))
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = pid;
        None
    }
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn linux_proc_stat_is_dead_or_zombie(stat: &str) -> bool {
    stat.rsplit_once(')')
        .map(|(_, rest)| rest.trim_start())
        .is_some_and(|after_comm| after_comm.starts_with(['Z', 'X', 'x']))
}

pub fn pid_is_alive(pid: u32) -> bool {
    if pid <= 1 || pid > i32::MAX as u32 {
        return false;
    }
    #[cfg(unix)]
    {
        // SAFETY: `kill(pid, 0)` performs a kernel existence/permission check without sending a signal.
        let ok = unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
            || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM);
        #[cfg(target_os = "linux")]
        if ok
            && std::fs::read_to_string(format!("/proc/{pid}/stat"))
                .is_ok_and(|stat| linux_proc_stat_is_dead_or_zombie(&stat))
        {
            return false;
        }
        ok
    }
    #[cfg(not(unix))]
    {
        false
    }
}

pub fn default_lsof_bin() -> &'static str {
    if std::path::Path::new("/usr/sbin/lsof").is_file() {
        "/usr/sbin/lsof"
    } else if std::path::Path::new("/usr/bin/lsof").is_file() {
        "/usr/bin/lsof"
    } else {
        "lsof"
    }
}

pub fn sanitized_lsof_command(bin: &str) -> std::process::Command {
    let mut cmd = std::process::Command::new(bin);
    cmd.env_remove("LC_ALL")
        .env_remove("LC_CTYPE")
        .env("LC_ALL", "en_US.UTF-8")
        .env("LANG", "en_US.UTF-8");
    cmd
}

pub fn decode_lsof_path(raw: &str) -> PathBuf {
    use std::os::unix::ffi::OsStringExt;
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 3 < bytes.len() && bytes[i + 1] == b'x' {
            if let (Some(hi), Some(lo)) = (
                (bytes[i + 2] as char).to_digit(16),
                (bytes[i + 3] as char).to_digit(16),
            ) {
                out.push(((hi << 4) | lo) as u8);
                i += 4;
                continue;
            }
        } else if bytes[i] == b'\\' && i + 1 < bytes.len() {
            match bytes[i + 1] {
                b'n' => {
                    out.push(b'\n');
                    i += 2;
                    continue;
                }
                b't' => {
                    out.push(b'\t');
                    i += 2;
                    continue;
                }
                b'\\' => {
                    out.push(b'\\');
                    i += 2;
                    continue;
                }
                _ => {}
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    PathBuf::from(std::ffi::OsString::from_vec(out))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::MetadataExt;

    #[test]
    fn lsof_hex_escape_decoder_restores_utf8_chinese_paths() {
        let escaped =
            r"/Users/example/Documents/\xe7\xa8\x8b\xe5\xba\x8f\xe5\xbc\x80\xe5\x8f\x91/AgentHub";
        assert_eq!(
            decode_lsof_path(escaped),
            PathBuf::from("/Users/example/Documents/程序开发/AgentHub")
        );
    }

    #[test]
    fn linux_proc_stat_parser_handles_parentheses_and_spaces_in_comm() {
        assert!(linux_proc_stat_is_dead_or_zombie(
            "4242 (claude (worker)) Z 1 4242 4242 0 -1 4194560 0 0 0 0"
        ));
        assert!(linux_proc_stat_is_dead_or_zombie(
            "4242 (codex helper) X 1 4242 4242 0 -1"
        ));
        assert!(!linux_proc_stat_is_dead_or_zombie(
            "4242 (claude (worker)) S 1 4242 4242 0 -1 4194560 0 0 0 0"
        ));
        assert!(!linux_proc_stat_is_dead_or_zombie(
            "4242 (Z-worker) R 1 4242 4242 0 -1"
        ));
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn kernel_probes_read_utf8_cwd_args_and_open_vnode_inodes_without_locale_dependency() {
        let pid = std::process::id();
        assert!(pid_is_alive(pid));
        assert!(!pid_is_alive(u32::MAX));

        let cwd = resolve_kernel_cwd(pid).expect("kernel cwd");
        assert_eq!(
            cwd.canonicalize().unwrap(),
            std::env::current_dir().unwrap().canonicalize().unwrap()
        );
        let args = resolve_kernel_args(pid).expect("kernel args");
        assert!(!args.is_empty());

        let dir = std::env::temp_dir().join(format!(
            "agenthub-kernel-中文路径-{}-{}",
            pid,
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("会话文件.jsonl");
        std::fs::write(&target, b"{\"ok\":true}\n").unwrap();
        let open_file = std::fs::File::open(&target).unwrap();
        let meta = open_file.metadata().unwrap();

        let (paths, inodes) = collect_kernel_open_vnodes(pid).expect("kernel open vnodes");
        let canonical_target = target.canonicalize().unwrap();
        assert!(
            inodes.contains(&(meta.dev(), meta.ino())),
            "kernel fd table must contain open (dev, ino)"
        );
        assert!(
            paths.iter().any(|p| p == &canonical_target
                || p.canonicalize().ok().as_ref() == Some(&canonical_target)),
            "kernel fd paths must preserve UTF-8 Chinese filename without C-locale escaping"
        );
        drop(open_file);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
