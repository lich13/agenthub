use super::{
    app_server_client::{read_response, write_message, CodexAppServerClient},
    thread_rows::read_thread_rows_matching,
    CodexPaths, ThreadStatus,
};
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::{process::Stdio, sync::OnceLock, time::Duration};
use tokio::{
    io::{AsyncReadExt, BufReader},
    process::Command,
    time::{timeout_at, Instant},
};

pub async fn set_thread_title(paths: &CodexPaths, id: &str, title: &str) -> Result<()> {
    static CLIENT: OnceLock<CodexAppServerClient> = OnceLock::new();
    CLIENT
        .get_or_init(CodexAppServerClient::new)
        .rename_thread(paths, id, title)
        .await
}

impl CodexAppServerClient {
    pub async fn rename_thread(&self, paths: &CodexPaths, id: &str, title: &str) -> Result<()> {
        rename_with_client(paths, id, title, self, Duration::from_secs(10)).await
    }
}

async fn rename_with_client(
    paths: &CodexPaths,
    id: &str,
    title: &str,
    client: &CodexAppServerClient,
    timeout: Duration,
) -> Result<()> {
    let name = title.trim();
    ensure!(!name.is_empty(), "thread title cannot be empty");
    ensure!(!id.trim().is_empty(), "thread_id is required");
    ensure!(
        !read_thread_rows_matching(paths, Some(id))?
            .iter()
            .any(|row| row.summary.status == ThreadStatus::Archived),
        "Archived tasks must be restored before renaming"
    );
    let deadline = Instant::now() + timeout;
    let executable = timeout_at(deadline, client.resolve_executable())
        .await
        .context("Codex CLI discovery timed out")?
        .context("Codex CLI is required to rename native tasks")?;
    let mut child = crate::jobs::spawn_retrying_etxtbsy(
        Command::new(executable)
            .args(["app-server", "--stdio"])
            .env("CODEX_HOME", &paths.home)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true),
    )
    .context("start Codex app-server for task rename")?;
    let result = timeout_at(deadline, async {
        let mut stdin = child.stdin.take().context("open Codex app-server stdin")?;
        let stdout = child
            .stdout
            .take()
            .context("open Codex app-server stdout")?;
        let mut stdout = BufReader::new(stdout.take(4 * 1024 * 1024));
        write_message(
            &mut stdin,
            &json!({"id":1,"method":"initialize","params":{
                "clientInfo":{"name":"agenthub","version":env!("CARGO_PKG_VERSION")}
            }}),
        )
        .await?;
        let _: Value = read_response(&mut stdout, 1).await?;
        write_message(&mut stdin, &json!({"method":"initialized"})).await?;
        write_message(
            &mut stdin,
            &json!({"id":2,"method":"thread/name/set","params":{"threadId":id,"name":name}}),
        )
        .await?;
        let _: Value = read_response(&mut stdout, 2).await?;
        // Native persistence chooses title/name by history format and updates the name index.
        write_message(
            &mut stdin,
            &json!({"id":3,"method":"thread/read","params":{"threadId":id,"includeTurns":false}}),
        )
        .await?;
        let value: Value = read_response(&mut stdout, 3).await?;
        ensure!(
            value.pointer("/thread/id").and_then(Value::as_str) == Some(id)
                && value.pointer("/thread/name").and_then(Value::as_str) == Some(name),
            "Codex native rename verification failed; resulting state is unknown"
        );
        Ok(())
    })
    .await
    .context("Codex native rename timed out; resulting state is unknown")
    .and_then(|result| result);
    let _ = child.start_kill();
    let _ = child.wait().await;
    result
}

#[cfg(test)]
mod tests;
