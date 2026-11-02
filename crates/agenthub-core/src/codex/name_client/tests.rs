use super::*;
use std::{fs, os::unix::fs::PermissionsExt};

fn fixture(script: &str) -> (std::path::PathBuf, CodexAppServerClient) {
    let root = std::env::temp_dir().join(format!("agenthub-name-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let executable = root.join("codex");
    fs::write(&executable, format!("#!/bin/sh\nif [ \"$1\" = --version ]; then echo 'codex-cli 0.153.4'; exit 0; fi\n{script}")).unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    // CLI discovery has separate tests; protocol fixtures start with a resolved executable.
    (
        root,
        CodexAppServerClient::with_resolved_executable(executable, Duration::from_secs(2)),
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
        let (root, client) = fixture(&format!(
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
    let (root, client) = fixture(
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
    let client = CodexAppServerClient::with_candidates(vec![], Duration::from_secs(2));
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
