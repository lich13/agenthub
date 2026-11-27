use agenthub_core::services::ssh_rpc::{self, Request, Response};
use serde_json::Value;
use std::{
    collections::HashMap,
    process::Stdio,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tokio::{
    process::{Child, ChildStdin, Command},
    sync::{oneshot, Mutex as AsyncMutex},
};
const DISCONNECTED: &str = "SSH 连接已断开，下一次读取将自动重连；已提交操作不会自动重发";
#[derive(Debug, Clone)]
pub(crate) enum CallError {
    Disconnected,
    TimedOut,
    Remote(String),
}
impl std::fmt::Display for CallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Disconnected => f.write_str(DISCONNECTED),
            Self::TimedOut => f.write_str("远程操作等待超时，请刷新确认结果后重试"),
            Self::Remote(message) => f.write_str(message),
        }
    }
}
impl CallError {
    pub(crate) fn is_transport(&self) -> bool {
        !matches!(self, Self::Remote(_))
    }
}
type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value, CallError>>>>>;
pub(crate) struct Client {
    input: AsyncMutex<ChildStdin>,
    pending: Pending,
    next: AtomicU64,
    child: Arc<Mutex<Child>>,
    reader: tokio::task::JoinHandle<()>,
    connected: Arc<AtomicBool>,
}
pub(crate) fn command(alias: &str) -> Result<Command, String> {
    if !super::ssh_config::valid_alias(alias) {
        return Err("请选择有效的 SSH Host".into());
    }
    let mut command = Command::new("/usr/bin/ssh");
    command.args([
        "-T",
        "-o",
        "BatchMode=yes",
        "-o",
        "StrictHostKeyChecking=yes",
        "-o",
        "ConnectTimeout=10",
        "-o",
        "ServerAliveInterval=15",
        "-o",
        "ServerAliveCountMax=2",
        "-o",
        "ClearAllForwardings=yes",
        "-o",
        "RequestTTY=no",
        "-o",
        "RemoteCommand=none",
        "-o",
        "ControlMaster=no",
        "-o",
        "ControlPath=none",
        "--",
        alias,
    ]);
    command
        .kill_on_drop(true)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    Ok(command)
}
impl Client {
    pub(crate) fn connect(alias: &str, sudo: bool) -> Result<Arc<Self>, String> {
        let mut command = command(alias)?;
        command.arg(if sudo {
            "sudo -n /usr/local/bin/agenthub-webd rpc-stdio"
        } else {
            "/usr/local/bin/agenthub-webd rpc-stdio"
        });
        Self::spawn(command)
    }
    fn spawn(mut command: Command) -> Result<Arc<Self>, String> {
        let mut child = command.spawn().map_err(|_| "无法启动系统 SSH")?;
        let output = child.stdout.take().ok_or("SSH 输出不可用")?;
        Self::with_output(child, output)
    }

    fn with_output(
        mut child: Child,
        mut output: impl tokio::io::AsyncRead + Unpin + Send + 'static,
    ) -> Result<Arc<Self>, String> {
        let input = child.stdin.take().ok_or("SSH 输入不可用")?;
        let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
        let replies = pending.clone();
        let connected = Arc::new(AtomicBool::new(true));
        let reader_connected = connected.clone();
        let reader = tokio::spawn(async move {
            while let Ok(Some(response)) = ssh_rpc::read::<_, Response>(&mut output).await {
                if let Some(reply) = replies.lock().expect("SSH replies").remove(&response.id) {
                    let _ = reply.send(match (response.result, response.error) {
                        (_, Some(error)) => Err(CallError::Remote(error)),
                        (Some(value), _) => Ok(value),
                        _ => Err(CallError::Remote("远程响应格式无效".into())),
                    });
                }
            }
            reader_connected.store(false, Ordering::Release);
            for (_, reply) in replies.lock().expect("SSH replies").drain() {
                let _ = reply.send(Err(CallError::Disconnected));
            }
        });
        Ok(Arc::new(Self {
            input: AsyncMutex::new(input),
            pending,
            next: AtomicU64::new(1),
            child: Arc::new(Mutex::new(child)),
            reader,
            connected,
        }))
    }
    pub(crate) fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Acquire)
    }
    pub(crate) fn close(&self) {
        self.connected.store(false, Ordering::Release);
        self.reader.abort();
        for (_, reply) in self.pending.lock().expect("SSH replies").drain() {
            let _ = reply.send(Err(CallError::Disconnected));
        }
        let already_reaped = {
            let mut child = self.child.lock().expect("SSH child");
            let _ = child.start_kill();
            child.try_wait().ok().flatten().is_some()
        };
        if !already_reaped {
            let child = self.child.clone();
            if let Ok(handle) = tokio::runtime::Handle::try_current() {
                handle.spawn(async move {
                    for _ in 0..50 {
                        if child
                            .lock()
                            .expect("SSH child")
                            .try_wait()
                            .ok()
                            .flatten()
                            .is_some()
                        {
                            return;
                        }
                        tokio::time::sleep(Duration::from_millis(20)).await;
                    }
                    let _ = tokio::task::spawn_blocking(move || {
                        for _ in 0..50 {
                            if child
                                .lock()
                                .expect("SSH child")
                                .try_wait()
                                .ok()
                                .flatten()
                                .is_some()
                            {
                                return;
                            }
                            std::thread::sleep(Duration::from_millis(20));
                        }
                    })
                    .await;
                });
            } else {
                let _ = std::thread::Builder::new()
                    .name("agenthub-ssh-reap".into())
                    .spawn(move || {
                        for _ in 0..100 {
                            if child
                                .lock()
                                .expect("SSH child")
                                .try_wait()
                                .ok()
                                .flatten()
                                .is_some()
                            {
                                return;
                            }
                            std::thread::sleep(Duration::from_millis(20));
                        }
                    });
            }
        }
    }
    pub(crate) async fn call(&self, command: &str, args: Value) -> Result<Value, CallError> {
        self.call_with_timeout(command, args, Duration::from_secs(60))
            .await
    }
    async fn call_with_timeout(
        &self,
        command: &str,
        args: Value,
        timeout: Duration,
    ) -> Result<Value, CallError> {
        #[derive(Clone, Copy)]
        enum CallPhase {
            Queued,
            WritingFrame,
            WaitingReply,
            Completed,
        }
        struct CallCancelGuard<'a> {
            client: &'a Client,
            id: u64,
            phase: CallPhase,
        }
        impl Drop for CallCancelGuard<'_> {
            fn drop(&mut self) {
                self.client
                    .pending
                    .lock()
                    .expect("SSH replies")
                    .remove(&self.id);
                if matches!(self.phase, CallPhase::WritingFrame) {
                    // Mid-write cancellation may leave a partial JSON frame on stdin.
                    self.client.close();
                }
            }
        }

        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let (send, receive) = oneshot::channel();
        {
            let mut pending = self.pending.lock().expect("SSH replies");
            if !self.is_connected() {
                return Err(CallError::Disconnected);
            }
            pending.insert(id, send);
        }
        let mut guard = CallCancelGuard {
            client: self,
            id,
            phase: CallPhase::Queued,
        };
        let result = tokio::time::timeout(timeout, async {
            let mut input = self.input.lock().await;
            guard.phase = CallPhase::WritingFrame;
            ssh_rpc::write(
                &mut *input,
                &Request {
                    id,
                    command: command.into(),
                    args,
                },
            )
            .await
            .map_err(|_| CallError::Disconnected)?;
            guard.phase = CallPhase::WaitingReply;
            drop(input);
            let value = receive.await.map_err(|_| CallError::Disconnected)?;
            guard.phase = CallPhase::Completed;
            value
        })
        .await
        .unwrap_or(Err(CallError::TimedOut));
        if result.as_ref().is_err_and(|error| error.is_transport()) {
            self.close();
        }
        result
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
pub(super) fn fixture(mode: &str) -> Arc<Client> {
    // libtest owns stdout. The child uses its separate stderr pipe for RPC frames.
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "remote::ssh::tests::fixture_peer",
            "--nocapture",
        ])
        .env("AGENTHUB_TEST_SSH_PEER", mode)
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .stdout(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let output = child.stderr.take().unwrap();
    Client::with_output(child, output).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore = "started as the RPC peer by SSH transport tests"]
    async fn fixture_peer() {
        let Ok(mode) = std::env::var("AGENTHUB_TEST_SSH_PEER") else {
            return;
        };
        if mode == "exit" {
            return;
        }
        let mut input = tokio::io::stdin();
        let mut output = tokio::io::stderr();
        while let Some(request) = ssh_rpc::read::<_, Request>(&mut input).await.unwrap() {
            if mode == "disconnect" {
                break;
            }
            if mode == "silent" {
                continue;
            }
            let mut requests = vec![request];
            if mode == "reverse" {
                requests.push(ssh_rpc::read(&mut input).await.unwrap().unwrap());
                requests.reverse();
            }
            for request in requests {
                let response = match mode.as_str() {
                    "error" => Response {
                        id: request.id,
                        result: None,
                        error: Some("job not found".into()),
                    },
                    "reverse" => Response {
                        id: request.id,
                        result: Some(request.command.into()),
                        error: None,
                    },
                    "reply" => Response {
                        id: request.id,
                        result: Some("fixture".into()),
                        error: None,
                    },
                    _ => unreachable!(),
                };
                ssh_rpc::write(&mut output, &response).await.unwrap();
            }
            if mode == "reverse" {
                break;
            }
        }
    }

    #[tokio::test]
    async fn a_timed_out_write_closes_the_channel_and_releases_other_requests() {
        let client = fixture("silent");
        let (write, read) = tokio::join!(
            client.call_with_timeout(
                "threads.rename",
                serde_json::json!({}),
                Duration::from_millis(20)
            ),
            client.call("jobs.list", serde_json::json!({}))
        );
        assert!(matches!(write, Err(CallError::TimedOut)));
        assert!(matches!(read, Err(CallError::Disconnected)));
        assert!(!client.is_connected());
        assert!(matches!(
            client.call("jobs.list", serde_json::json!({})).await,
            Err(CallError::Disconnected)
        ));
    }

    #[tokio::test]
    async fn responses_follow_request_ids_and_disconnect_does_not_replay() {
        let client = fixture("reverse");
        let (one, two) = tokio::join!(
            client.call("first", serde_json::json!({})),
            client.call("second", serde_json::json!({}))
        );
        assert_eq!(one.unwrap(), "first");
        assert_eq!(two.unwrap(), "second");
        assert!(client.call("write", serde_json::json!({})).await.is_err());
        let reconnect = fixture("exit");
        assert!(reconnect
            .call("auth", serde_json::json!({}))
            .await
            .unwrap_err()
            .to_string()
            .contains("断开"));
    }
    #[tokio::test]
    async fn explicit_close_releases_inflight_reads_and_owned_process() {
        let client = fixture("silent");
        let pending = client.clone();
        let request =
            tokio::spawn(async move { pending.call("read", serde_json::json!({})).await });
        tokio::task::yield_now().await;
        client.close();
        assert!(tokio::time::timeout(Duration::from_secs(2), request)
            .await
            .unwrap()
            .unwrap()
            .is_err());
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if client.child.lock().unwrap().try_wait().unwrap().is_some() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("closed SSH child must exit");
    }
    #[test]
    fn system_ssh_resolves_identity_and_jump_configuration() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("config");
        std::fs::write(&path,"Host fixture\n HostName example.invalid\n User fixture-user\n Port 2222\n ProxyJump fixture-jump\n IdentityAgent none\n").unwrap();
        let base = command("fixture").unwrap();
        let mut args = base
            .as_std()
            .get_args()
            .map(|a| a.to_os_string())
            .collect::<Vec<_>>();
        args.truncate(args.len() - 2);
        let output = std::process::Command::new("/usr/bin/ssh")
            .args(args)
            .args(["-G", "-F"])
            .arg(path)
            .args(["--", "fixture"])
            .output()
            .unwrap();
        assert!(output.status.success());
        let config = String::from_utf8(output.stdout).unwrap();
        for line in [
            "hostname example.invalid",
            "user fixture-user",
            "port 2222",
            "proxyjump fixture-jump",
            "identityagent none",
            "batchmode yes",
        ] {
            assert!(
                config.lines().any(|s| s == line),
                "missing SSH setting {line}"
            );
        }
    }

    #[tokio::test]
    async fn cancelled_waiting_reply_cleans_pending_without_breaking_channel() {
        let client = fixture("reverse");
        {
            let mut fut = Box::pin(client.call("first", serde_json::json!({})));
            // Poll once so the frame is written and the call enters WaitingReply, then drop!
            let _ = tokio::time::timeout(Duration::from_millis(15), &mut fut).await;
        }
        assert!(client.pending.lock().unwrap().is_empty());
        assert!(client.is_connected());
        // Second call unblocks "reverse" fixture and receives its own reply cleanly.
        let second = client.call("second", serde_json::json!({})).await.unwrap();
        assert_eq!(second, "second");
    }
}
