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
pub enum CallError {
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
    pub fn is_transport(&self) -> bool {
        !matches!(self, Self::Remote(_))
    }
}
type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value, CallError>>>>>;
pub struct Client {
    input: AsyncMutex<ChildStdin>,
    pending: Pending,
    next: AtomicU64,
    child: Mutex<Child>,
    reader: tokio::task::JoinHandle<()>,
    connected: Arc<AtomicBool>,
}
pub fn command(alias: &str) -> Result<Command, String> {
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
    pub fn connect(alias: &str, sudo: bool) -> Result<Arc<Self>, String> {
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
        let input = child.stdin.take().ok_or("SSH 输入不可用")?;
        let mut output = child.stdout.take().ok_or("SSH 输出不可用")?;
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
            child: Mutex::new(child),
            reader,
            connected,
        }))
    }
    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Acquire)
    }
    pub fn close(&self) {
        self.connected.store(false, Ordering::Release);
        self.reader.abort();
        for (_, reply) in self.pending.lock().expect("SSH replies").drain() {
            let _ = reply.send(Err(CallError::Disconnected));
        }
        let _ = self.child.lock().expect("SSH child").start_kill();
    }
    pub async fn call(&self, command: &str, args: Value) -> Result<Value, CallError> {
        self.call_with_timeout(command, args, Duration::from_secs(60))
            .await
    }
    async fn call_with_timeout(
        &self,
        command: &str,
        args: Value,
        timeout: Duration,
    ) -> Result<Value, CallError> {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let (send, receive) = oneshot::channel();
        {
            let mut pending = self.pending.lock().expect("SSH replies");
            if !self.is_connected() {
                return Err(CallError::Disconnected);
            }
            pending.insert(id, send);
        }
        let result = tokio::time::timeout(timeout, async {
            ssh_rpc::write(
                &mut *self.input.lock().await,
                &Request {
                    id,
                    command: command.into(),
                    args,
                },
            )
            .await
            .map_err(|_| CallError::Disconnected)?;
            receive.await.map_err(|_| CallError::Disconnected)?
        })
        .await
        .unwrap_or(Err(CallError::TimedOut));
        if result.as_ref().is_err_and(|error| error.is_transport()) {
            self.close();
        }
        self.pending.lock().expect("SSH replies").remove(&id);
        result
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
pub(super) fn fixture(program: &str) -> Arc<Client> {
    let mut cmd = Command::new("python3");
    cmd.args(["-c", program])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    Client::spawn(cmd).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn a_timed_out_write_closes_the_channel_and_releases_other_requests() {
        let client = fixture("import sys; sys.stdin.buffer.read()");
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
        let client = fixture(
            r#"
import sys,struct,json
requests=[]
for _ in range(2):
 n=struct.unpack('>I',sys.stdin.buffer.read(4))[0]
 requests.append(json.loads(sys.stdin.buffer.read(n)))
for r in reversed(requests):
 b=json.dumps({'id':r['id'],'result':r['command'],'error':None}).encode()
 sys.stdout.buffer.write(struct.pack('>I',len(b))+b)
sys.stdout.buffer.flush()
"#,
        );
        let (one, two) = tokio::join!(
            client.call("first", serde_json::json!({})),
            client.call("second", serde_json::json!({}))
        );
        assert_eq!(one.unwrap(), "first");
        assert_eq!(two.unwrap(), "second");
        assert!(client.call("write", serde_json::json!({})).await.is_err());
        let reconnect = fixture("import sys; sys.exit(255)");
        assert!(reconnect
            .call("auth", serde_json::json!({}))
            .await
            .unwrap_err()
            .to_string()
            .contains("断开"));
    }
    #[tokio::test]
    async fn explicit_close_releases_inflight_reads_and_owned_process() {
        let client = fixture("import sys; sys.stdin.buffer.read()");
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
        let status = client.child.lock().unwrap().try_wait().unwrap();
        if status.is_none() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(client.child.lock().unwrap().try_wait().unwrap().is_some());
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
}
