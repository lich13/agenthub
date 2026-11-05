use agenthub_core::services::ssh_rpc::{self, Request, Response};
use serde_json::Value;
use std::{
    collections::HashMap,
    process::Stdio,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tokio::{
    process::{Child, ChildStdin, Command},
    sync::{oneshot, Mutex as AsyncMutex},
};
const DISCONNECTED: &str = "SSH 连接已断开，请重新连接；已提交操作不会自动重发";
type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value, String>>>>>;
pub struct Client {
    input: AsyncMutex<ChildStdin>,
    pending: Pending,
    next: AtomicU64,
    child: Mutex<Child>,
    reader: tokio::task::JoinHandle<()>,
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
        let reader = tokio::spawn(async move {
            while let Ok(Some(response)) = ssh_rpc::read::<_, Response>(&mut output).await {
                if let Some(reply) = replies.lock().expect("SSH replies").remove(&response.id) {
                    let _ = reply.send(match (response.result, response.error) {
                        (_, Some(error)) => Err(error),
                        (Some(value), _) => Ok(value),
                        _ => Err("远程响应格式无效".into()),
                    });
                }
            }
            for (_, reply) in replies.lock().expect("SSH replies").drain() {
                let _ = reply.send(Err(DISCONNECTED.into()));
            }
        });
        Ok(Arc::new(Self {
            input: AsyncMutex::new(input),
            pending,
            next: AtomicU64::new(1),
            child: Mutex::new(child),
            reader,
        }))
    }
    pub fn close(&self) {
        self.reader.abort();
        for (_, reply) in self.pending.lock().expect("SSH replies").drain() {
            let _ = reply.send(Err(DISCONNECTED.into()));
        }
        let _ = self.child.lock().expect("SSH child").start_kill();
    }
    pub async fn call(&self, command: &str, args: Value) -> Result<Value, String> {
        if self.reader.is_finished() {
            return Err(DISCONNECTED.into());
        }
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let (send, receive) = oneshot::channel();
        self.pending.lock().expect("SSH replies").insert(id, send);
        let result = tokio::time::timeout(Duration::from_secs(60), async {
            ssh_rpc::write(
                &mut *self.input.lock().await,
                &Request {
                    id,
                    command: command.into(),
                    args,
                },
            )
            .await
            .map_err(|_| DISCONNECTED.to_string())?;
            receive.await.map_err(|_| DISCONNECTED.to_string())?
        })
        .await
        .unwrap_or_else(|_| Err("远程操作等待超时，请刷新确认结果后重试".into()));
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
mod tests {
    use super::*;
    fn fixture(program: &str) -> Arc<Client> {
        let mut cmd = Command::new("python3");
        cmd.args(["-c", program])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        Client::spawn(cmd).unwrap()
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
