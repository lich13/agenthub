//! Private desktop protocol boundary, separate from the historical reader and
//! short-lived app-server helper. This layer only follows an existing owner;
//! it never starts a native execution process or writes a queue snapshot.
//!
//! Wire versions were checked against desktop build 20586. New mutations must
//! pass the dedicated native protocol checks before being exposed by Tauri.
pub mod attachments;
pub mod commands;
pub mod diagnostics;
mod framing;
pub mod helper;
pub mod projection;
mod protocol;
pub mod references;
mod review;
mod session;
mod socket;
pub mod state;
pub mod worker;
pub use session::DesktopSession;

use crate::resource_limits::CODEX_DESKTOP_CONNECT_WAIT;
use framing::{FrameReader, FrameWriter};
use protocol::{Method, QUEUE_VERSION, STREAM_VERSION};
use serde_json::{json, Value};
use std::{fmt, path::Path, time::Duration};
use tokio::{io::AsyncWriteExt, net::UnixStream, time::timeout};

/// Fixed classifications only: never include native errors, paths or content.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DesktopError {
    InvalidIdentity,
    Unavailable,
    UnsafeSocket,
    Timeout,
    Disconnected,
    InvalidFrame,
    FrameTooLarge,
    UnsupportedProtocol,
    OwnerUnavailable,
    OutOfSync,
    ReadOnly,
    Busy,
    InvalidInput,
    RequestExpired,
    Rejected,
    OutcomeUnknown,
}

impl fmt::Display for DesktopError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidIdentity => "Codex desktop thread identity is invalid",
            Self::Unavailable => "Codex desktop is unavailable",
            Self::UnsafeSocket => "Codex desktop connection identity could not be verified",
            Self::Timeout => "Codex desktop connection timed out",
            Self::Disconnected => "Codex desktop connection closed",
            Self::InvalidFrame => "Codex desktop returned an invalid frame",
            Self::FrameTooLarge => "Codex desktop frame exceeded the size limit",
            Self::UnsupportedProtocol => "Codex desktop protocol is not supported",
            Self::OwnerUnavailable => "Codex desktop thread owner is unavailable",
            Self::OutOfSync => "Codex desktop thread state needs to be refreshed",
            Self::ReadOnly => "Codex desktop thread is read-only",
            Self::Busy => "Codex desktop thread is busy",
            Self::InvalidInput => "Codex desktop input is invalid",
            Self::RequestExpired => "Codex desktop request is no longer pending",
            Self::Rejected => "Codex desktop rejected the request",
            Self::OutcomeUnknown => "Codex desktop submission outcome is unknown",
        })
    }
}

impl std::error::Error for DesktopError {}

impl DesktopError {
    /// Safe user-facing classification shared by command errors and stream
    /// updates. Display remains a fixed English classification for diagnostics.
    pub const fn user_message(self) -> &'static str {
        match self {
            Self::OwnerUnavailable => "请先在 Codex 桌面打开同一线程，再重试连接",
            Self::Unavailable => "Codex 桌面当前不可用，请打开 Codex 后连接",
            Self::ReadOnly => "此线程仅支持阅读",
            Self::Busy => "线程正在处理其他请求，请稍后重试",
            Self::InvalidInput => "输入内容无效或超出限制",
            Self::InvalidIdentity | Self::UnsafeSocket => "无法确认 Codex 桌面或线程身份",
            Self::Timeout => "Codex 桌面未及时响应，请重试连接",
            Self::RequestExpired => "该请求已失效，请查看最新状态",
            Self::Rejected => "Codex 未接受此操作",
            Self::OutcomeUnknown => "发送结果未确认，请先在 Codex 中核对，勿重复发送",
            Self::Disconnected => "Codex 桌面连接已断开，请重新连接",
            Self::UnsupportedProtocol => "此操作与当前 Codex 桌面协议不兼容，请在 Codex 中继续处理",
            Self::OutOfSync => "Codex 桌面状态已失步，请重新连接；仍可继续阅读",
            Self::InvalidFrame | Self::FrameTooLarge => {
                "当前 Codex 桌面状态无法安全处理，请重新连接或在 Codex 中继续"
            }
        }
    }
}

type Result<T> = std::result::Result<T, DesktopError>;

/// Native payloads stay inside the Rust integration. They are not public DTOs
/// and intentionally do not implement Debug or Serialize.
pub enum DesktopEvent {
    Snapshot { revision: u64, state: Value },
    Patches { revision: u64, change: Value },
    Queue { messages: Vec<Value> },
}

/// One selected local thread and one verified owner per connection. Dropping
/// the session closes the socket, so cancelled connection attempts cannot leave
/// a background follower or an execution process behind.
pub struct DesktopReadSession {
    stream: Option<UnixStream>,
    reader: FrameReader,
    writer: FrameWriter,
    conversation_id: String,
    client_id: String,
    owner_id: String,
    revision: Option<u64>,
}

impl DesktopReadSession {
    /// `codex_home` comes from native configuration, never a WebUI path input.
    /// Attaching does not load a thread into a desktop window or change focus.
    pub async fn attach(codex_home: &Path, conversation_id: &str) -> Result<Self> {
        Self::attach_with_timeout(codex_home, conversation_id, CODEX_DESKTOP_CONNECT_WAIT).await
    }

    async fn attach_with_timeout(
        codex_home: &Path,
        conversation_id: &str,
        wait: Duration,
    ) -> Result<Self> {
        uuid::Uuid::parse_str(conversation_id).map_err(|_| DesktopError::InvalidIdentity)?;
        let connect = async {
            let stream = socket::connect(codex_home).await?;
            let mut session = Self {
                stream: Some(stream),
                reader: FrameReader::default(),
                writer: FrameWriter::default(),
                conversation_id: conversation_id.into(),
                client_id: String::new(),
                owner_id: String::new(),
                revision: None,
            };
            let response = session
                .request(Method::Initialize, json!({"clientType":"agenthub"}))
                .await?;
            session.client_id = identity(&response["result"]["clientId"])?;
            let response = session
                .request(
                    Method::DiscoverOwner,
                    json!({"hostId":"local","conversationId":conversation_id}),
                )
                .await?;
            session.owner_id = identity(&response["handledByClientId"])?;
            if session.owner_id == session.client_id {
                return Err(DesktopError::OwnerUnavailable);
            }
            session.follow(true).await?;
            Ok(session)
        };
        timeout(wait, connect)
            .await
            .map_err(|_| DesktopError::Timeout)?
    }

    /// Cancellation is safe: partially read headers and bodies remain owned
    /// by FrameReader. There is no event backlog or unbounded channel here.
    /// A revision gap fails closed; reconnect to obtain a fresh snapshot.
    pub async fn next_event(&mut self) -> Result<DesktopEvent> {
        let result = self.next_verified_event().await;
        if result.is_err() {
            self.stream.take();
            self.reader = FrameReader::default();
            self.writer = FrameWriter::default();
            self.revision = None;
        }
        result
    }

    pub async fn close(mut self) {
        // Explicit unsubscribe is best effort; EOF also removes this client.
        let _ = timeout(Duration::from_secs(1), self.follow(false)).await;
        if let Some(mut stream) = self.stream.take() {
            let _ = stream.shutdown().await;
        }
    }

    async fn next_verified_event(&mut self) -> Result<DesktopEvent> {
        loop {
            let message = self.receive().await?;
            if let Some(event) = self.event_from_message(message)? {
                return Ok(event);
            }
        }
    }

    fn event_from_message(&mut self, message: Value) -> Result<Option<DesktopEvent>> {
        let method = match message["method"].as_str() {
            Some("thread-stream-state-changed") => "thread-stream-state-changed",
            Some("thread-queued-followups-changed") => "thread-queued-followups-changed",
            _ => "connect",
        };
        let version = message["version"].as_u64();
        self.event_from_message_inner(message).inspect_err(|error| {
            diagnostics::record(
                diagnostics::Source::Ipc,
                method,
                version,
                None,
                None,
                *error,
            );
        })
    }

    fn event_from_message_inner(&mut self, mut message: Value) -> Result<Option<DesktopEvent>> {
        if message["type"] != "broadcast" {
            return Ok(None);
        }
        if message["method"] == "ipc-connection-reset" {
            return Err(DesktopError::Disconnected);
        }
        let params = &message["params"];
        if message["sourceClientId"].as_str() != Some(&self.owner_id)
            || params["conversationId"].as_str() != Some(&self.conversation_id)
            || params
                .get("hostId")
                .is_some_and(|host| host.as_str() != Some("local"))
        {
            return Ok(None);
        }
        match message["method"].as_str() {
            Some("thread-stream-state-changed") => {
                if message["version"].as_u64() != Some(STREAM_VERSION) {
                    return Err(DesktopError::UnsupportedProtocol);
                }
                let change = &mut message["params"]["change"];
                let revision = revision(&change["revision"])?;
                match change["type"].as_str() {
                    Some("snapshot") => {
                        if change["conversationState"]["id"].as_str() != Some(&self.conversation_id)
                        {
                            return Err(DesktopError::InvalidIdentity);
                        }
                        if self.revision.is_some_and(|previous| revision < previous) {
                            return Err(DesktopError::OutOfSync);
                        }
                        self.revision = Some(revision);
                        return Ok(Some(DesktopEvent::Snapshot {
                            revision,
                            state: change["conversationState"].take(),
                        }));
                    }
                    Some("patches") => {
                        let base = revision_value(change.get("baseRevision"))?;
                        if self.revision != Some(base) || revision <= base {
                            return Err(DesktopError::OutOfSync);
                        }
                        if !change["patches"].is_array() {
                            return Err(DesktopError::InvalidFrame);
                        }
                        self.revision = Some(revision);
                        return Ok(Some(DesktopEvent::Patches {
                            revision,
                            change: change.take(),
                        }));
                    }
                    _ => return Err(DesktopError::UnsupportedProtocol),
                }
            }
            Some("thread-queued-followups-changed") => {
                if message["version"].as_u64() != Some(QUEUE_VERSION) {
                    return Err(DesktopError::UnsupportedProtocol);
                }
                let Value::Array(messages) = message["params"]["messages"].take() else {
                    return Err(DesktopError::InvalidFrame);
                };
                return Ok(Some(DesktopEvent::Queue { messages }));
            }
            _ => {}
        }
        Ok(None)
    }

    async fn request(&mut self, method: Method, params: Value) -> Result<Value> {
        let (method, version) = method.wire();
        let request_id = uuid::Uuid::new_v4().to_string();
        let source = (!self.client_id.is_empty()).then_some(self.client_id.as_str());
        self.send(&json!({
            "type":"request", "requestId":request_id, "sourceClientId":source,
            "method":method, "version":version, "params":params,
            "timeoutMs":CODEX_DESKTOP_CONNECT_WAIT.as_millis() as u64,
        }))
        .await?;
        loop {
            let response = self.receive().await?;
            if response["type"] != "response" || response["requestId"] != request_id {
                continue;
            }
            if response["resultType"] == "error" {
                diagnostics::record(
                    diagnostics::Source::Ipc,
                    method,
                    Some(version),
                    None,
                    response["error"].as_str(),
                    protocol::response_error(response["error"].as_str()),
                );
            }
            return match response["resultType"].as_str() {
                Some("success") => Ok(response),
                Some("error") => Err(match protocol::response_error(response["error"].as_str()) {
                    DesktopError::Rejected => DesktopError::Unavailable,
                    error => error,
                }),
                _ => Err(DesktopError::InvalidFrame),
            };
        }
    }

    async fn follow(&mut self, following: bool) -> Result<()> {
        let (method, version) = Method::Follow.wire();
        self.send(&json!({
            "type":"broadcast", "sourceClientId":self.client_id,
            "method":method, "version":version,
            "targetClientIds":[self.owner_id],
            "params":{"hostId":"local","conversationId":self.conversation_id,"following":following}
        }))
        .await
    }

    async fn send(&mut self, value: &Value) -> Result<()> {
        self.writer
            .send(
                self.stream.as_mut().ok_or(DesktopError::Disconnected)?,
                value,
            )
            .await
    }

    async fn receive(&mut self) -> Result<Value> {
        loop {
            // A cancelled receive may have been answering client discovery.
            // Finish its frame before reading or writing another message.
            self.writer
                .flush(self.stream.as_mut().ok_or(DesktopError::Disconnected)?)
                .await?;
            let message = self
                .reader
                .read(self.stream.as_mut().ok_or(DesktopError::Disconnected)?)
                .await?;
            if message["type"] == "client-discovery-request" {
                let request_id = identity(&message["requestId"])?;
                self.send(&json!({
                    "type":"client-discovery-response", "requestId":request_id,
                    "response":{"canHandle":false}
                }))
                .await?;
            } else {
                return Ok(message);
            }
        }
    }
}

fn identity(value: &Value) -> Result<String> {
    let value = value.as_str().ok_or(DesktopError::InvalidIdentity)?;
    if value.is_empty() || value.len() > 128 || value.chars().any(char::is_control) {
        return Err(DesktopError::InvalidIdentity);
    }
    Ok(value.into())
}

fn revision(value: &Value) -> Result<u64> {
    revision_value(Some(value))
}

fn revision_value(value: Option<&Value>) -> Result<u64> {
    value
        .and_then(Value::as_u64)
        .filter(|value| *value < 1_u64 << 53)
        .ok_or(DesktopError::InvalidFrame)
}

#[cfg(test)]
#[path = "desktop/tests.rs"]
mod tests;
