use crate::state::AppState;
use agenthub_core::services::ssh_rpc::{self, Request, Response, SOCKET_PATH};
use anyhow::{Context, Result};
use std::{path::Path, sync::Arc};
use tokio::{
    net::{UnixListener, UnixStream},
    sync::Mutex,
};

pub async fn proxy() -> Result<()> {
    let stream = UnixStream::connect(SOCKET_PATH)
        .await
        .context("AgentHub 服务不可用，请安装或启动服务")?;
    proxy_stream(stream).await
}
async fn proxy_stream(stream: UnixStream) -> Result<()> {
    let (mut input, mut output) = stream.into_split();
    let mut stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();
    let result = tokio::select! {
        result=tokio::io::copy(&mut stdin,&mut output)=>result,
        result=tokio::io::copy(&mut input,&mut stdout)=>result,
    };
    // Tokio's blocking stdin read cannot be cancelled when the service closes.
    // This dedicated proxy owns no application state; exit also releases stdin.
    if let Err(error) = &result {
        eprintln!("SSH proxy: {error}");
    }
    std::process::exit(if result.is_ok() { 0 } else { 1 });
}
pub async fn health() -> Result<()> {
    let mut stream = UnixStream::connect(SOCKET_PATH).await?;
    ssh_rpc::write(
        &mut stream,
        &Request {
            id: 1,
            command: "system.capabilities".into(),
            args: serde_json::json!({}),
        },
    )
    .await?;
    let response: Response = ssh_rpc::read(&mut stream)
        .await?
        .context("service closed")?;
    anyhow::ensure!(response.error.is_none(), "service health failed");
    println!("ok");
    Ok(())
}
pub async fn serve(state: AppState) -> Result<()> {
    serve_at(state, Path::new(SOCKET_PATH)).await
}
async fn serve_at(state: AppState, path: &Path) -> Result<()> {
    use std::os::unix::fs::{FileTypeExt, PermissionsExt};
    if let Ok(meta) = std::fs::symlink_metadata(path) {
        anyhow::ensure!(
            meta.file_type().is_socket(),
            "service socket path is not a socket"
        );
        anyhow::ensure!(
            UnixStream::connect(path).await.is_err(),
            "AgentHub service is already running"
        );
        std::fs::remove_file(path)?;
    }
    let listener = UnixListener::bind(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    loop {
        let (stream, _) = listener.accept().await?;
        let state = state.clone();
        tokio::spawn(async move {
            let _ = connection(state, stream).await;
        });
    }
}
async fn connection(state: AppState, stream: UnixStream) -> Result<()> {
    let (mut reader, writer) = stream.into_split();
    let writer = Arc::new(Mutex::new(writer));
    while let Some(request) = ssh_rpc::read::<_, Request>(&mut reader).await? {
        let state = state.clone();
        let writer = writer.clone();
        // An accepted operation finishes independently of the SSH connection.
        tokio::spawn(async move {
            let response = dispatch(state, request).await;
            let mut writer = writer.lock().await;
            if ssh_rpc::write(&mut *writer, &response).await.is_err() {
                use tokio::io::AsyncWriteExt;
                let _ = writer.shutdown().await;
            }
        });
    }
    Ok(())
}
pub(crate) async fn dispatch(state: AppState, request: Request) -> Response {
    match crate::api::rpc_dispatch::rpc_dispatch(state, request.command, request.args).await {
        Ok(value) => Response {
            id: request.id,
            result: Some(value),
            error: None,
        },
        Err(error) => Response {
            id: request.id,
            result: None,
            error: Some(agenthub_core::security::redact_output(&error.message)),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agenthub_core::{db::PanelDb, jobs::ProcessCommand};
    use serde_json::json;
    fn state() -> AppState {
        let (config, root) = crate::api::test_support::sandboxed_test_config();
        AppState::new(config, PanelDb::open(":memory:").unwrap()).with_test_sandbox(root)
    }
    #[tokio::test]
    async fn proxy_exits_when_service_closes_with_stdin_still_open() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        if let Some(path) = std::env::var_os("AGENTHUB_PROXY_FIXTURE_SOCKET") {
            proxy_stream(UnixStream::connect(path).await.unwrap())
                .await
                .unwrap();
            unreachable!();
        }
        let state = state();
        let path = state.config().codex.home.join("proxy.sock");
        let listener = UnixListener::bind(&path).unwrap();
        let mut child = tokio::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "ssh_server::tests::proxy_exits_when_service_closes_with_stdin_still_open",
                "--nocapture",
            ])
            .env("AGENTHUB_PROXY_FIXTURE_SOCKET", &path)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            let (mut stream, _) = listener.accept().await.unwrap();
            child
                .stdin
                .as_mut()
                .unwrap()
                .write_all(b"frame")
                .await
                .unwrap();
            let mut bytes = [0; 5];
            stream.read_exact(&mut bytes).await.unwrap();
            assert_eq!(&bytes, b"frame");
            drop(stream);
            assert!(child.wait().await.unwrap().success());
        })
        .await
        .unwrap();
    }
    #[tokio::test]
    async fn private_socket_dispatches_concurrent_frames_and_keeps_jobs_after_disconnect() {
        let state = state();
        let db = state.db.clone();
        let (mut client, server) = UnixStream::pair().unwrap();
        let server = tokio::spawn(connection(state.clone(), server));
        for (id, command) in [(7, "system.capabilities"), (8, "jobs.list")] {
            ssh_rpc::write(
                &mut client,
                &Request {
                    id,
                    command: command.into(),
                    args: json!({}),
                },
            )
            .await
            .unwrap();
        }
        let mut ids = Vec::new();
        for _ in 0..2 {
            let response: Response = ssh_rpc::read(&mut client).await.unwrap().unwrap();
            assert!(response.error.is_none());
            if response.id == 7 {
                assert_eq!(response.result.unwrap()["api_version"], 2);
            }
            ids.push(response.id);
        }
        ids.sort();
        assert_eq!(ids, vec![7, 8]);
        let job = state
            .jobs
            .start_process_job(
                "fixture",
                "fixture",
                ProcessCommand::new("/bin/sh", ["-c", "sleep 0.05; printf 'complete\\n'"]),
            )
            .unwrap();
        drop(client);
        server.await.unwrap().unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            loop {
                if db.job(&job).unwrap().unwrap().status != "running" {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(db.job(&job).unwrap().unwrap().output, "complete\n");
        let detail = dispatch(
            state.clone(),
            Request {
                id: 9,
                command: "jobs.detail".into(),
                args: json!({"id":job}),
            },
        )
        .await;
        assert_eq!(detail.result.unwrap()["output"], "complete\n");
        let list = dispatch(
            state,
            Request {
                id: 10,
                command: "jobs.list".into(),
                args: json!({}),
            },
        )
        .await;
        assert!(!list.result.unwrap().to_string().contains("complete"));
    }
    #[tokio::test]
    async fn socket_has_private_permissions_and_unsupported_actions_fail() {
        use std::os::unix::fs::PermissionsExt;
        let state = state();
        let root = state.config().codex.home;
        let path = root.join("rpc.sock");
        let server_state = state.clone();
        let socket = path.clone();
        let server = tokio::spawn(async move { serve_at(server_state, &socket).await });
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while !path.exists() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let response = dispatch(
            state,
            Request {
                id: 1,
                command: "remote.install".into(),
                args: json!({}),
            },
        )
        .await;
        assert!(response.error.is_some());
        server.abort();
        let _ = server.await;
    }
}
