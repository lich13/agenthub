use agenthub_core::config::Config;
use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};

static SANDBOX_COUNTER: AtomicUsize = AtomicUsize::new(0);

pub(crate) fn write_grok_notification_fixture(
    root: &Path,
    kind: &str,
    body: &str,
    timestamp_ms: i64,
) -> agenthub_core::native_probe::NativeScan {
    use serde_json::json;
    let id = "00000000-0000-4000-8000-000000000003";
    let dir = root.join("sessions/workspace").join(id);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("summary.json"), json!({"info":{"id":id,"cwd":"/workspace/fixture"},"generated_title":"Fixture","num_messages":2}).to_string()).unwrap();
    let events = [
        json!({"type":"turn_started","session_id":id,"session_relationship":"primary","ts":timestamp_ms-500}),
        json!({"type":"turn_ended","outcome":if kind=="failure" {"error"} else {"completed"},"ts":timestamp_ms}),
    ];
    let updates = [
        json!({"timestamp":timestamp_ms-250,"params":{"sessionId":id,"_meta":{"promptId":"fixture-prompt"},"update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":body}}}}),
        json!({"timestamp":timestamp_ms,"params":{"sessionId":id,"update":{"sessionUpdate":"turn_completed","prompt_id":"fixture-prompt","stop_reason":if kind=="failure" {"error"} else {"end_turn"}}}}),
    ];
    for (name, rows) in [("events.jsonl", events), ("updates.jsonl", updates)] {
        fs::write(
            dir.join(name),
            rows.iter()
                .map(|row| format!("{row}\n"))
                .collect::<String>(),
        )
        .unwrap();
    }
    let scan = agenthub_core::native_probe::scan(agenthub_core::native_probe::NativeProvider::Grok)
        .unwrap();
    assert_eq!(scan.errors, 0);
    assert_eq!(scan.streams.len(), 1);
    assert_eq!(scan.streams[0].events.len(), 1);
    assert_eq!(scan.streams[0].events[0].kind, kind);
    scan
}
pub(crate) static CONFIG_ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

pub(crate) struct AsyncEnvGuard {
    _guard: tokio::sync::MutexGuard<'static, ()>,
    previous: Vec<(&'static str, Option<OsString>)>,
}

impl AsyncEnvGuard {
    pub(crate) async fn set_many(vars: &[(&'static str, &Path)]) -> Self {
        let guard = CONFIG_ENV_LOCK.lock().await;
        let mut previous = Vec::with_capacity(vars.len());
        for &(key, value) in vars {
            previous.push((key, std::env::var_os(key)));
            std::env::set_var(key, value);
        }
        Self {
            _guard: guard,
            previous,
        }
    }

    pub(crate) async fn set_config(path: &Path) -> Self {
        Self::set_many(&[("AGENTHUB_CONFIG", path)]).await
    }

    pub(crate) async fn lock_read_only() -> tokio::sync::MutexGuard<'static, ()> {
        CONFIG_ENV_LOCK.lock().await
    }
}

impl Drop for AsyncEnvGuard {
    fn drop(&mut self) {
        for (key, value) in &self.previous {
            match value {
                Some(v) => std::env::set_var(key, v),
                None => std::env::remove_var(key),
            }
        }
    }
}

#[cfg(unix)]
pub(crate) fn write_executable_fixture(path: &Path, script: &str) {
    use std::os::unix::fs::PermissionsExt;
    let rest = script
        .strip_prefix("#!/bin/sh\n")
        .expect("executable fixture script must start with #!/bin/sh");
    let body = format!(
        "#!/bin/sh\nif [ \"${{AGENTHUB_TEST_EXEC_PROBE:-}}\" = \"1\" ]; then exit 0; fi\n{rest}"
    );
    fs::write(path, body).expect("write executable fixture");
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .expect("set executable fixture permissions");
    for attempt in 0..10 {
        match std::process::Command::new(path)
            .env("AGENTHUB_TEST_EXEC_PROBE", "1")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
        {
            Err(err) if err.kind() == std::io::ErrorKind::ExecutableFileBusy && attempt < 9 => {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            Ok(_) => break,
            Err(err) => panic!("probe executable fixture {}: {err}", path.display()),
        }
    }
}

pub(crate) fn sandboxed_test_config() -> (Config, PathBuf) {
    let idx = SANDBOX_COUNTER.fetch_add(1, Ordering::Relaxed);
    let sandbox_root = std::env::temp_dir().join(format!(
        "agenthub-test-cfg-sandbox-{}-{idx}",
        std::process::id()
    ));
    let codex_home = sandbox_root.join(".codex");
    let _ = fs::create_dir_all(&codex_home);
    let mut config = Config::current_defaults().unwrap();
    config.codex.home = codex_home;
    config.codex.workspace = sandbox_root.join("workspace");
    config.paths.data_dir = sandbox_root.join("data");
    config.paths.db_path = sandbox_root.join("data/panel.sqlite");
    config.paths.log_dir = sandbox_root.join("logs");

    (config, sandbox_root)
}

#[derive(Clone, Debug)]
pub(crate) struct TestHttpRequest {
    pub(crate) method: reqwest::Method,
    pub(crate) uri: String,
    pub(crate) body: Vec<u8>,
}

impl TestHttpRequest {
    pub(crate) fn json(&self) -> serde_json::Value {
        serde_json::from_slice(&self.body).unwrap_or(serde_json::Value::Null)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct TestHttpResponse {
    pub(crate) status: reqwest::StatusCode,
    pub(crate) headers: Vec<(String, String)>,
    pub(crate) body: Vec<u8>,
}

impl TestHttpResponse {
    pub(crate) fn json(status: reqwest::StatusCode, value: serde_json::Value) -> Self {
        Self {
            status,
            headers: Vec::new(),
            body: serde_json::to_vec(&value).expect("serialize test HTTP JSON"),
        }
    }

    pub(crate) fn ok_json(value: serde_json::Value) -> Self {
        Self::json(reqwest::StatusCode::OK, value)
    }

    pub(crate) fn status_only(status: reqwest::StatusCode) -> Self {
        Self {
            status,
            headers: Vec::new(),
            body: Vec::new(),
        }
    }

    pub(crate) fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }
}

type RouteHandler = std::sync::Arc<
    dyn Fn(
            TestHttpRequest,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = TestHttpResponse> + Send>>
        + Send
        + Sync,
>;

#[derive(Clone, Default)]
pub(crate) struct TestHttpRouter {
    routes: Vec<(String, RouteHandler)>,
}

impl TestHttpRouter {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn post<F, Fut>(mut self, path: impl Into<String>, handler: F) -> Self
    where
        F: Fn(TestHttpRequest) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = TestHttpResponse> + Send + 'static,
    {
        let handler: RouteHandler = std::sync::Arc::new(move |req| Box::pin(handler(req)));
        self.routes.push((path.into(), handler));
        self
    }

    pub(crate) async fn serve_on(
        self,
        listener: tokio::net::TcpListener,
    ) -> tokio::task::JoinHandle<()> {
        let routes = std::sync::Arc::new(self.routes);
        spawn_http_server_on(listener, move |req| {
            let routes = routes.clone();
            async move {
                if req.method != reqwest::Method::POST {
                    return TestHttpResponse::status_only(reqwest::StatusCode::METHOD_NOT_ALLOWED);
                }
                let path = req.uri.split('?').next().unwrap_or(&req.uri);
                for (route_path, handler) in routes.iter() {
                    if route_path == path {
                        return handler(req).await;
                    }
                }
                TestHttpResponse::status_only(reqwest::StatusCode::NOT_FOUND)
            }
        })
    }
}

pub(crate) fn spawn_http_server_on<F, Fut>(
    listener: tokio::net::TcpListener,
    handler: F,
) -> tokio::task::JoinHandle<()>
where
    F: Fn(TestHttpRequest) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = TestHttpResponse> + Send + 'static,
{
    let handler = std::sync::Arc::new(handler);
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let handler = handler.clone();
            tokio::spawn(async move {
                let _ = serve_single_http_connection(stream, handler).await;
            });
        }
    })
}

async fn serve_single_http_connection<F, Fut>(
    stream: tokio::net::TcpStream,
    handler: std::sync::Arc<F>,
) -> std::io::Result<()>
where
    F: Fn(TestHttpRequest) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = TestHttpResponse> + Send + 'static,
{
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).await? == 0 {
        return Ok(());
    }
    let mut parts = request_line
        .trim_end_matches(['\r', '\n'])
        .split_whitespace();
    let method_str = parts.next().unwrap_or("GET");
    let uri = parts.next().unwrap_or("/").to_string();
    let method = reqwest::Method::from_bytes(method_str.as_bytes()).unwrap_or(reqwest::Method::GET);

    let mut content_length = 0usize;
    let mut chunked = false;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).await? == 0 || line == "\r\n" || line == "\n" {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            let value = value.trim();
            if name.eq_ignore_ascii_case("content-length") {
                content_length = value.parse::<usize>().unwrap_or(0);
            } else if name.eq_ignore_ascii_case("transfer-encoding")
                && value.to_ascii_lowercase().contains("chunked")
            {
                chunked = true;
            }
        }
    }

    let mut body = Vec::new();
    if chunked {
        loop {
            let mut size_line = String::new();
            if reader.read_line(&mut size_line).await? == 0 {
                break;
            }
            let size_hex = size_line.trim().split(';').next().unwrap_or("0");
            let chunk_size = usize::from_str_radix(size_hex, 16).unwrap_or(0);
            if chunk_size == 0 {
                let mut trailer = String::new();
                let _ = reader.read_line(&mut trailer).await;
                break;
            }
            let start = body.len();
            body.resize(start + chunk_size, 0);
            reader.read_exact(&mut body[start..]).await?;
            let mut crlf = [0u8; 2];
            reader.read_exact(&mut crlf).await?;
        }
    } else if content_length > 0 {
        body.resize(content_length, 0);
        reader.read_exact(&mut body).await?;
    }

    let response = handler(TestHttpRequest { method, uri, body }).await;
    let reason = response.status.canonical_reason().unwrap_or("OK");
    let mut head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n",
        response.status.as_u16(),
        reason,
        response.body.len()
    );
    for (name, value) in &response.headers {
        head.push_str(name);
        head.push_str(": ");
        head.push_str(value);
        head.push_str("\r\n");
    }
    head.push_str("\r\n");
    write_half.write_all(head.as_bytes()).await?;
    if !response.body.is_empty() {
        write_half.write_all(&response.body).await?;
    }
    write_half.flush().await?;
    let _ = write_half.shutdown().await;
    Ok(())
}
