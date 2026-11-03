#![cfg(unix)]

use super::*;
use crate::shell::{shell_quote, QuoteStyle};
use std::{fs, os::unix::fs::symlink, path::PathBuf, time::Duration};

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "agenthub-app-server-client-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir(&root).expect("create isolated Codex CLI fixture");
        Self { root }
    }

    fn executable_with_version(&self, relative: &str, version: &str) -> PathBuf {
        self.write_executable(
            relative,
            &format!(
                "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then\n    printf '%s\\n' 'codex-cli {version}'\n    exit 0\nfi\nexit 1\n"
            ),
        )
    }

    fn executable_with_script(&self, relative: &str, script: &str) -> PathBuf {
        self.write_executable(relative, script)
    }

    fn executable_with_probe_log(&self, relative: &str) -> PathBuf {
        let log = shell_quote(
            &self.root.join("probe.log").to_string_lossy(),
            QuoteStyle::Always,
        );
        self.write_executable(
            relative,
            &format!(
                "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then\n    printf '%s\\n' probe >> {log}\n    printf '%s\\n' 'codex-cli 0.153.4'\n    exit 0\nfi\nexit 1\n"
            ),
        )
    }

    fn probe_count(&self) -> usize {
        fs::read_to_string(self.root.join("probe.log"))
            .expect("read fixture probe log")
            .lines()
            .count()
    }

    fn write_executable(&self, relative: &str, contents: &str) -> PathBuf {
        let executable = self.root.join(relative);
        fs::create_dir_all(executable.parent().expect("fixture path has a parent"))
            .expect("create fixture executable directory");
        crate::write_executable_fixture(&executable, contents);
        executable
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn discovery_keeps_new_and_legacy_chatgpt_cli_fallbacks_in_priority_order() {
    let candidates = discover_codex_candidates();
    let new_path =
        PathBuf::from("/Applications/ChatGPT.app/Contents/Resources/codex-cli/bin/codex");
    let legacy_path = PathBuf::from("/Applications/ChatGPT.app/Contents/Resources/codex");

    // PATH has priority and may itself contain the legacy bundle. Check the
    // fixed fallbacks without mistaking a user PATH entry for that fallback.
    assert!(candidates.ends_with(&[new_path, legacy_path]));
}

#[tokio::test]
async fn resolution_prefers_newest_version_then_earlier_candidate_on_ties() {
    let fixture = Fixture::new();
    let legacy = fixture.executable_with_version("ChatGPT.app/Contents/Resources/codex", "0.153.4");
    let current = fixture.executable_with_version(
        "ChatGPT.app/Contents/Resources/codex-cli/bin/codex",
        "0.154.0",
    );
    let tied = fixture.executable_with_version("other/codex", "0.154.0");
    assert!(legacy.is_file() && current.is_file());

    let client = CodexAppServerClient::with_candidates(
        vec![legacy, current.clone(), tied],
        Duration::from_secs(2),
    );

    assert_eq!(client.resolve_executable().await.unwrap(), current);
}

#[tokio::test]
async fn resolution_errors_when_every_candidate_is_unusable() {
    let fixture = Fixture::new();
    let too_old = fixture.executable_with_version("too-old/codex", "0.144.1");
    let nonzero = fixture.executable_with_script("failed/codex", "#!/bin/sh\nexit 1\n");
    let client =
        CodexAppServerClient::with_candidates(vec![too_old, nonzero], Duration::from_secs(2));

    let error = client.resolve_executable().await.unwrap_err();
    assert!(error
        .to_string()
        .contains("Codex CLI 0.144.2 or newer is required"));
}

#[tokio::test]
async fn resolution_deduplicates_symlink_aliases_and_caches_the_selection() {
    let fixture = Fixture::new();
    let executable = fixture.executable_with_probe_log("codex");
    let alias = fixture.root.join("codex-alias");
    symlink(&executable, &alias).expect("create executable alias");
    let client = CodexAppServerClient::with_candidates(
        vec![executable.clone(), alias],
        Duration::from_secs(2),
    );

    assert_eq!(client.resolve_executable().await.unwrap(), executable);
    assert_eq!(fixture.probe_count(), 1);
    assert_eq!(client.resolve_executable().await.unwrap(), executable);
    assert_eq!(fixture.probe_count(), 1);
}
