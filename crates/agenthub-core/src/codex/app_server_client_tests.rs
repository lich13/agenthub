#![cfg(unix)]

use super::*;
use crate::shell::{shell_quote, QuoteStyle};
use std::{fs, os::unix::fs::symlink, path::PathBuf};

#[tokio::test]
async fn protocol_line_limit_stops_before_waiting_for_a_newline() {
    let (mut writer, reader) = tokio::io::duplex(MAX_PROTOCOL_LINE_BYTES + 1);
    writer
        .write_all(&vec![b'x'; MAX_PROTOCOL_LINE_BYTES + 1])
        .await
        .unwrap();
    // The writer stays open: a read_line-based implementation would hang here.
    let mut reader = tokio::io::BufReader::new(reader);
    let error = tokio::time::timeout(
        Duration::from_secs(1),
        read_response::<Value>(&mut reader, 1),
    )
    .await
    .unwrap()
    .unwrap_err();
    assert!(error.to_string().contains("size limit"));
}

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

    let client = CodexAppServerClient::with_candidates(vec![legacy, current.clone(), tied]);

    assert_eq!(client.resolve_executable().await.unwrap(), current);
}

#[tokio::test]
async fn resolution_errors_when_every_candidate_is_unusable() {
    let fixture = Fixture::new();
    let too_old = fixture.executable_with_version("too-old/codex", "0.144.1");
    let nonzero = fixture.executable_with_script("failed/codex", "#!/bin/sh\nexit 1\n");
    let client = CodexAppServerClient::with_candidates(vec![too_old, nonzero]);

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
    let client = CodexAppServerClient::with_candidates(vec![executable.clone(), alias]);

    assert_eq!(client.resolve_executable().await.unwrap(), executable);
    assert_eq!(fixture.probe_count(), 1);
    assert_eq!(client.resolve_executable().await.unwrap(), executable);
    assert_eq!(fixture.probe_count(), 1);
}

fn rename_fixture(script: &str) -> (PathBuf, CodexAppServerClient) {
    let root = std::env::temp_dir().join(format!("agenthub-name-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let executable = root.join("codex");
    crate::write_executable_fixture(
        &executable,
        &format!(
            "#!/bin/sh\nif [ \"$1\" = --version ]; then echo 'codex-cli 0.153.4'; exit 0; fi\n{script}"
        ),
    );
    (
        root,
        CodexAppServerClient::with_resolved_executable(executable),
    )
}

#[tokio::test]
async fn native_rename_uses_fixed_protocol_and_rejects_unverified_success() {
    for (returned_id, returned_name, success) in [
        ("fixture", "A quoted \"title\"", true),
        ("fixture", "Wrong name", false),
        ("another-thread", "A quoted \"title\"", false),
    ] {
        let response = json!({"id":3,"result":{"thread":{"id":returned_id,"name":returned_name}}});
        let (root, client) = rename_fixture(&format!(
            r#"
[ "$#" -eq 2 ] && [ "$1" = app-server ] && [ "$2" = --stdio ] || exit 2
while IFS= read -r request; do
printf '%s\n' "$request" >> "$CODEX_HOME/requests.jsonl"
case "$request" in
*'"id":1,'*) printf '%s\n' '{{"id":1,"result":{{}}}}';;
*'"id":2,'*) printf '%s\n' '{{"method":"thread/name/updated","params":{{}}}}' '{{"id":99,"result":{{}}}}' '{{"id":2,"result":{{}}}}';;
*'"id":3,'*) printf '%s\n' '{response}';;
esac
done
"#
        ));
        let result = rename_with_client(
            &CodexPaths::new(&root),
            "fixture",
            " A quoted \"title\" ",
            &client,
            Duration::from_secs(2),
        )
        .await;
        assert_eq!(result.is_ok(), success, "{result:?}");
        let requests: Vec<Value> = fs::read_to_string(root.join("requests.jsonl"))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(
            requests
                .iter()
                .map(|value| value["method"].as_str().unwrap())
                .collect::<Vec<_>>(),
            [
                "initialize",
                "initialized",
                "thread/name/set",
                "thread/read"
            ]
        );
        assert_eq!(
            requests[2]["params"],
            json!({"threadId":"fixture","name":"A quoted \"title\""})
        );
        assert_eq!(
            requests[3]["params"],
            json!({"threadId":"fixture","includeTurns":false})
        );
        assert!(!root.join("state_5.sqlite").exists());
        fs::remove_dir_all(root).unwrap();
    }
}

#[tokio::test]
async fn native_rename_timeout_reaps_child_and_does_not_fall_back_to_sql() {
    let (root, client) = rename_fixture(
        "printf '%s' $$ > \"$CODEX_HOME/pid\"\nwhile read -r request; do read -r next; done\n",
    );
    let error = rename_with_client(
        &CodexPaths::new(&root),
        "fixture",
        "New name",
        &client,
        Duration::from_secs(2),
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("resulting state is unknown"));
    let pid = fs::read_to_string(root.join("pid")).unwrap();
    assert!(!std::process::Command::new("kill")
        .args(["-0", pid.trim()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap()
        .success());
    assert!(!root.join("state_5.sqlite").exists());
    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn archived_native_rename_is_rejected_without_changing_metadata_or_spawning() {
    let root = std::env::temp_dir().join(format!("agenthub-name-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let conn = rusqlite::Connection::open(root.join("state_5.sqlite")).unwrap();
    conn.execute_batch("CREATE TABLE threads (id TEXT, title TEXT, archived INTEGER, archived_at INTEGER); INSERT INTO threads VALUES ('fixture', 'Archived native title', 1, NULL);").unwrap();
    drop(conn);
    let before = fs::read(root.join("state_5.sqlite")).unwrap();
    let client = CodexAppServerClient::with_candidates(vec![]);
    let error = client
        .rename_thread(&CodexPaths::new(&root), "fixture", "New title")
        .await
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "Archived tasks must be restored before renaming"
    );
    assert_eq!(fs::read(root.join("state_5.sqlite")).unwrap(), before);
    assert!(!root.join("session_index.jsonl").exists());
    fs::remove_dir_all(root).unwrap();
}
