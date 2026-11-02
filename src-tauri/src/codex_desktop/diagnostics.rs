use agenthub_core::services::codex_desktop::DesktopDiagnostics;
use std::{process::Stdio, time::Duration};

pub async fn metadata() -> DesktopDiagnostics {
    let (desktop_build, bundled_cli_version) = tokio::join!(
        read_version(
            "/usr/bin/plutil",
            &[
                "-extract",
                "CFBundleVersion",
                "raw",
                "-o",
                "-",
                "/Applications/ChatGPT.app/Contents/Info.plist"
            ],
            true
        ),
        read_version(
            "/Applications/ChatGPT.app/Contents/Resources/codex-cli/bin/codex",
            &["--version"],
            false
        ),
    );
    DesktopDiagnostics {
        records: agenthub_core::codex::desktop::diagnostics::snapshot(),
        desktop_build,
        bundled_cli_version,
    }
}

async fn read_version(program: &str, args: &[&str], build: bool) -> Option<String> {
    let output = tokio::time::timeout(
        Duration::from_secs(3),
        tokio::process::Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .output(),
    )
    .await
    .ok()?
    .ok()?;
    if !output.status.success() {
        return None;
    }
    version_text(&output.stdout, build)
}

fn version_text(bytes: &[u8], build: bool) -> Option<String> {
    if bytes.len() > 128 {
        return None;
    }
    let value = std::str::from_utf8(bytes).ok()?.trim();
    let value = if build {
        value
    } else {
        value.strip_prefix("codex-cli ")?
    };
    if value.is_empty()
        || !value.chars().all(|c| {
            c.is_ascii_digit() || !build && (c.is_ascii_alphabetic() || matches!(c, '.' | '-'))
        })
    {
        return None;
    }
    if !build {
        let core = value.split('-').next()?;
        let parts: Vec<_> = core.split('.').collect();
        if parts.len() != 3
            || parts
                .iter()
                .any(|part| part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()))
        {
            return None;
        }
    }
    Some(value.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn version_metadata_accepts_only_the_expected_shape() {
        assert_eq!(version_text(b"20586\n", true).as_deref(), Some("20586"));
        assert_eq!(
            version_text(b"codex-cli 0.163.0-alpha.10.2\n", false).as_deref(),
            Some("0.163.0-alpha.10.2")
        );
        for value in [
            b"/private/fixture".as_slice(),
            b"user@example.invalid",
            b"https://example.invalid",
            b"codex-cli key=secret",
        ] {
            assert!(version_text(value, false).is_none());
        }
    }
}
