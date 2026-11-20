use crate::{host::HostBridge, overview::DesktopState};
use agenthub_core::platform::PlatformPaths;
use agenthub_core::services::updates::{self, UpdateAction, UpdateState, UpdateStatus};
use anyhow::{anyhow, bail, Context, Result};
use base64::Engine;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    io::Read,
    path::{Component, Path, PathBuf},
    process::Command,
    sync::OnceLock,
    time::Duration,
};

const UPDATER_FEED_URL: &str =
    "https://github.com/lich13/agenthub/releases/latest/download/latest.json";
const UPDATER_PLATFORM_KEY: &str = "darwin-aarch64";
const UPDATER_PUBKEY_BOX: &str = include_str!("../../../../deploy/agenthub-webd/updater.pub");
const MAX_MANIFEST_BYTES: usize = 1024 * 1024;
const MAX_ARCHIVE_BYTES: usize = 256 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
pub struct DesktopUpdateCheckResponse {
    pub job_id: String,
    pub status: UpdateStatus,
}

#[derive(Debug, Clone, Serialize)]
pub struct DesktopUpdateInstallResponse {
    pub job_id: String,
    pub installed: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct LatestManifest {
    version: String,
    platforms: BTreeMap<String, ManifestPlatform>,
}

#[derive(Debug, Clone, Deserialize)]
struct ManifestPlatform {
    url: String,
    signature: String,
}

pub fn desktop_update_status_with_state(
    state: &DesktopState,
    latest_version: Option<&str>,
    last_error: Option<&str>,
) -> Result<UpdateStatus> {
    let recent_check_job = if latest_version.is_none() && last_error.is_none() {
        state
            .db
            .list_jobs(25)?
            .into_iter()
            .find(|job| job.kind == "agenthub_update_check")
            .map(|job| state.db.job(&job.id))
            .transpose()?
            .flatten()
    } else {
        None
    };
    Ok(updates::update_status_with_recent_check_job(
        state.platform(),
        latest_version,
        last_error,
        recent_check_job.as_ref(),
    ))
}

pub(crate) async fn check_update_status(
    state: &DesktopState,
) -> std::result::Result<DesktopUpdateCheckResponse, String> {
    let job = native_update_job_plan_for_action(state.platform(), UpdateAction::Check)
        .map_err(|err| err.to_string())?;
    let job_id = start_update_job(state, &job).map_err(|err| err.to_string())?;

    match check_update(state, &job_id, UPDATER_FEED_URL).await {
        Ok(status) => {
            state
                .db
                .finish_job(&job_id, "succeeded", Some(0), None)
                .map_err(|err| format!("操作已完成，但无法保存作业 {job_id} 的结果: {err}"))?;
            Ok(DesktopUpdateCheckResponse { job_id, status })
        }
        Err(err) => Err(finish_update_error(state, &job_id, err)),
    }
}

pub(crate) async fn install_update_and_restart(
    state: &DesktopState,
    host: &HostBridge,
    isolated: bool,
) -> std::result::Result<DesktopUpdateInstallResponse, String> {
    let job = native_update_job_plan_for_action(state.platform(), UpdateAction::Install)
        .map_err(|err| err.to_string())?;
    let job_id = start_update_job(state, &job).map_err(|err| err.to_string())?;

    if isolated {
        return Err(finish_update_error(
            state,
            &job_id,
            anyhow!("预览隔离模式下已禁用原地应用更新安装"),
        ));
    }

    match install_update(state, &job_id, UPDATER_FEED_URL).await {
        Ok(installed) => {
            state
                .db
                .finish_job(&job_id, "succeeded", Some(0), None)
                .map_err(|err| format!("操作已完成，但无法保存作业 {job_id} 的结果: {err}"))?;
            if installed {
                host.request_app_restart();
            }
            Ok(DesktopUpdateInstallResponse { job_id, installed })
        }
        Err(err) => Err(finish_update_error(state, &job_id, err)),
    }
}

fn update_job_id(spec: &updates::MacosUpdaterJobSpec) -> String {
    let action = spec
        .kind
        .strip_prefix("agenthub_update_")
        .unwrap_or(spec.kind.as_str());
    format!(
        "desktop-update-{action}-{}-{}",
        jiff::Timestamp::now().as_millisecond(),
        std::process::id()
    )
}

fn start_update_job(state: &DesktopState, spec: &updates::MacosUpdaterJobSpec) -> Result<String> {
    let id = update_job_id(spec);
    state.db.create_job(&id, &spec.kind, &spec.title)?;
    state.db.append_job_output(&id, &spec.initial_output)?;
    Ok(id)
}

fn finish_update_error(state: &DesktopState, job_id: &str, err: anyhow::Error) -> String {
    let message = err.to_string();
    let _ = state
        .db
        .append_job_output(job_id, &format!("error: {message}\n"));
    if let Err(error) = state.db.finish_job(job_id, "failed", None, Some(&message)) {
        return format!("{message}；无法保存作业 {job_id} 的结果: {error}");
    }
    message
}

fn native_update_job_plan_for_action(
    platform: &PlatformPaths,
    action: UpdateAction,
) -> Result<updates::MacosUpdaterJobSpec> {
    let plan = updates::plan_update_action(platform, action)?;
    plan.macos_job
        .ok_or_else(|| anyhow!("update action did not produce a macOS updater job spec"))
}

fn http_client() -> Result<reqwest::Client> {
    static PROVIDER: OnceLock<()> = OnceLock::new();
    PROVIDER.get_or_init(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
    reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .user_agent(concat!("AgentHub/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|err| anyhow!("初始化更新器失败: {err}"))
}

async fn fetch_bounded(client: &reqwest::Client, url: &str, max_bytes: usize) -> Result<Vec<u8>> {
    let mut response = client
        .get(url)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)?;
    if let Some(len) = response.content_length() {
        if len > max_bytes as u64 {
            bail!("响应内容超出安全大小上限");
        }
    }
    let mut out = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if out.len().saturating_add(chunk.len()) > max_bytes {
            bail!("响应内容超出安全大小上限");
        }
        out.extend_from_slice(&chunk);
    }
    Ok(out)
}

async fn fetch_latest_manifest(
    client: &reqwest::Client,
    feed_url: &str,
) -> Result<(LatestManifest, ManifestPlatform)> {
    let bytes = fetch_bounded(client, feed_url, MAX_MANIFEST_BYTES)
        .await
        .map_err(|err| anyhow!("检查更新失败: {err}"))?;
    let manifest: LatestManifest =
        serde_json::from_slice(&bytes).map_err(|err| anyhow!("解析更新清单失败: {err}"))?;
    let platform = manifest
        .platforms
        .get(UPDATER_PLATFORM_KEY)
        .cloned()
        .ok_or_else(|| anyhow!("更新清单缺少 {UPDATER_PLATFORM_KEY} 资产"))?;
    if platform.signature.trim().is_empty() || !platform.url.starts_with("https://") {
        bail!("更新清单签名或下载地址无效");
    }
    Ok((manifest, platform))
}

pub(crate) fn verify_minisign_archive(
    archive_bytes: &[u8],
    signature_base64: &str,
    expected_version: &str,
) -> Result<()> {
    let pubkey = minisign_verify::PublicKey::decode(UPDATER_PUBKEY_BOX)
        .map_err(|err| anyhow!("更新公钥无效: {err}"))?;
    let decoded_sig = base64::engine::general_purpose::STANDARD
        .decode(signature_base64.trim())
        .map_err(|_| anyhow!("更新签名编码无效"))?;
    let sig_text = std::str::from_utf8(&decoded_sig).map_err(|_| anyhow!("更新签名文本无效"))?;
    let signature = minisign_verify::Signature::decode(sig_text)
        .map_err(|err| anyhow!("更新签名格式无效: {err}"))?;
    pubkey
        .verify(archive_bytes, &signature, false)
        .map_err(|err| anyhow!("更新包 Minisign 签名校验失败: {err}"))?;

    let normalized_expected = expected_version.trim().trim_start_matches('v');
    for token in signature.trusted_comment().split(['\t', ' ']) {
        if let Some(ver) = token.strip_prefix("version:") {
            if ver.trim().trim_start_matches('v') != normalized_expected {
                bail!("签名可信注释版本与清单不一致");
            }
        }
    }
    Ok(())
}

async fn check_update(state: &DesktopState, job_id: &str, feed_url: &str) -> Result<UpdateStatus> {
    let client = http_client()?;
    let mut status = desktop_update_status_with_state(state, None, None)?;
    let (manifest, _) = fetch_latest_manifest(&client, feed_url).await?;
    let available =
        updates::update_available_for_versions(&status.current_version, &manifest.version);
    if available == Some(true) {
        state.db.append_job_output(
            job_id,
            &updates::macos_updater_update_available_output(&manifest.version),
        )?;
        status.latest_version = Some(manifest.version);
        status.update_available = Some(true);
        status.state = UpdateState::Ready;
    } else {
        state
            .db
            .append_job_output(job_id, updates::macos_updater_no_update_output())?;
        status.latest_version = Some(status.current_version.clone());
        status.update_available = Some(false);
        status.state = UpdateState::Idle;
    }
    Ok(status)
}

fn current_app_bundle_path() -> Result<PathBuf> {
    let exe = std::env::current_exe().context("无法定位当前可执行文件")?;
    let canonical = exe.canonicalize().unwrap_or(exe);
    for ancestor in canonical.ancestors() {
        if ancestor.extension().and_then(|s| s.to_str()) == Some("app") {
            return Ok(ancestor.to_path_buf());
        }
    }
    bail!("当前并非从 .app 应用包启动，无法执行原地更新")
}

fn read_bundle_short_version(app_bundle: &Path) -> Result<String> {
    let plist = app_bundle.join("Contents/Info.plist");
    let output = Command::new("/usr/bin/plutil")
        .args(["-extract", "CFBundleShortVersionString", "raw", "-o", "-"])
        .arg(&plist)
        .output()
        .context("无法读取应用包 Info.plist")?;
    if !output.status.success() {
        bail!("应用包缺少有效的 CFBundleShortVersionString");
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn extract_and_verify_bundle(
    archive_bytes: &[u8],
    staging_dir: &Path,
    expected_version: &str,
) -> Result<PathBuf> {
    let decoder = flate2::read::GzDecoder::new(archive_bytes);
    let mut archive = tar::Archive::new(decoder.take((MAX_ARCHIVE_BYTES as u64) * 4));
    for entry in archive.entries().context("无法读取更新归档")? {
        let mut entry = entry.context("更新归档条目损坏")?;
        let rel = entry.path().context("更新归档路径无效")?.into_owned();
        if rel.is_absolute()
            || rel.components().any(|c| {
                matches!(
                    c,
                    Component::ParentDir | Component::RootDir | Component::Prefix(_)
                )
            })
        {
            bail!("更新归档包含非法路径");
        }
        let first = rel.components().next().and_then(|c| c.as_os_str().to_str());
        if first != Some("AgentHub.app") {
            bail!("更新归档顶层目录必须为 AgentHub.app");
        }
        let entry_type = entry.header().entry_type();
        if !(entry_type.is_file() || entry_type.is_dir() || entry_type.is_symlink()) {
            bail!("更新归档包含不支持的文件类型");
        }
        if entry_type.is_symlink() {
            let link = entry
                .link_name()
                .context("符号链接无效")?
                .ok_or_else(|| anyhow!("符号链接目标为空"))?;
            if link.is_absolute()
                || link
                    .components()
                    .any(|c| matches!(c, Component::ParentDir | Component::RootDir))
            {
                bail!("更新归档包含越界符号链接");
            }
        }
        entry.unpack_in(staging_dir).context("解压更新归档失败")?;
    }

    let extracted = staging_dir.join("AgentHub.app");
    if !extracted.is_dir() {
        bail!("更新归档未包含 AgentHub.app");
    }
    let bundle_ver = read_bundle_short_version(&extracted)?;
    if bundle_ver.trim_start_matches('v') != expected_version.trim().trim_start_matches('v') {
        bail!("更新包内版本号与清单不一致");
    }
    // Minisign authenticates the publisher; codesign checks the sealed bundle contents.
    let verify = Command::new("/usr/bin/codesign")
        .args(["--verify", "--deep", "--strict"])
        .arg(&extracted)
        .status()
        .context("无法调用 codesign 校验更新包")?;
    if !verify.success() {
        bail!("更新包代码签名校验未通过");
    }
    Ok(extracted)
}

async fn install_update(state: &DesktopState, job_id: &str, feed_url: &str) -> Result<bool> {
    let client = http_client()?;
    let status = desktop_update_status_with_state(state, None, None)?;
    let (manifest, platform) = fetch_latest_manifest(&client, feed_url).await?;
    if updates::update_available_for_versions(&status.current_version, &manifest.version)
        != Some(true)
    {
        state
            .db
            .append_job_output(job_id, updates::macos_updater_no_update_output())?;
        return Ok(false);
    }

    let target_bundle = current_app_bundle_path()?;
    let parent_dir = target_bundle
        .parent()
        .ok_or_else(|| anyhow!("无法定位应用安装目录"))?
        .to_path_buf();

    state.db.append_job_output(
        job_id,
        &format!("downloading signed app update {}\n", manifest.version),
    )?;
    let bytes = fetch_bounded(&client, &platform.url, MAX_ARCHIVE_BYTES)
        .await
        .map_err(|err| anyhow!("下载更新失败: {err}"))?;

    verify_minisign_archive(&bytes, &platform.signature, &manifest.version)?;

    state.db.append_job_output(
        job_id,
        &format!("installing signed app update {}\n", manifest.version),
    )?;
    let expected_ver = manifest.version.clone();
    tokio::task::spawn_blocking(move || -> Result<()> {
        let staging = tempfile::Builder::new()
            .prefix(".AgentHub-update-")
            .tempdir_in(&parent_dir)
            .map_err(|err| anyhow!("安装更新失败（目录无写权限）: {err}"))?;
        let extracted = extract_and_verify_bundle(&bytes, staging.path(), &expected_ver)?;
        let backup = parent_dir.join(format!(
            ".AgentHub.app.backup-{}-{}",
            std::process::id(),
            jiff::Timestamp::now().as_millisecond()
        ));
        std::fs::rename(&target_bundle, &backup)
            .map_err(|err| anyhow!("备份当前应用失败: {err}"))?;
        replace_backed_up_bundle(&extracted, &target_bundle, &backup)
    })
    .await
    .map_err(|err| anyhow!("安装更新任务异常: {err}"))??;

    state
        .db
        .append_job_output(job_id, "signed app update installed\n")?;
    Ok(true)
}

fn replace_backed_up_bundle(extracted: &Path, target: &Path, backup: &Path) -> Result<()> {
    if let Err(err) = std::fs::rename(extracted, target) {
        if let Err(restore) = std::fs::rename(backup, target) {
            anyhow::bail!(
                "替换应用失败: {err}；恢复失败: {restore}；原应用保存在 {}",
                backup.display()
            );
        }
        bail!("替换应用包失败: {err}");
    }
    let _ = std::fs::remove_dir_all(backup);
    Ok(())
}

#[cfg(test)]
#[path = "updates_tests.rs"]
mod tests;
