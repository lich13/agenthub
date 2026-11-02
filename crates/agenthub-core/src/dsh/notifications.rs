//! Read-only v4 event folding. Compressed logs append independent Zstandard frames.
use super::*;
use crate::native_probe::{
    self, NativeProvider, NativeScan, NativeStreamSnapshot, NativeTurnEvent,
};
use sha2::{Digest, Sha256};
use std::{
    collections::VecDeque,
    io::{Seek, SeekFrom},
    sync::Mutex,
};

const FILE_LIMIT: u64 = 128 * 1024 * 1024;
const LINE_LIMIT: usize = 8 * 1024 * 1024;
const EVENT_LIMIT: usize = 1024;
static CACHE: Mutex<VecDeque<Cached>> = Mutex::new(VecDeque::new());

struct Cached {
    path: PathBuf,
    stamp: FileStamp,
    identity: String,
    offset: u64,
    anchor: Vec<u8>,
    state: State,
    last_accessed: std::time::Instant,
}

pub(crate) fn evict_idle_cache(max_idle: std::time::Duration) {
    if let Ok(mut cache) = CACHE.lock() {
        let now = std::time::Instant::now();
        cache.retain(|entry| now.saturating_duration_since(entry.last_accessed) < max_idle);
    }
}
#[derive(Default)]
struct State {
    header: Option<Value>,
    next_seq: u64,
    inherited: u64,
    open_turn: Option<u64>,
    last_text: String,
    title: String,
    timed: bool,
    pending: BTreeMap<String, (NativeTurnEvent, bool, bool)>,
    events: VecDeque<NativeTurnEvent>,
}

fn text(value: &Value) -> String {
    let s = value.as_str().map(str::to_owned).unwrap_or_else(|| {
        value
            .as_array()
            .into_iter()
            .flatten()
            .filter(|b| b["type"] == "text")
            .filter_map(|b| b["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n")
    });
    crate::security::redact_output(&s)
        .chars()
        .take(1200)
        .collect()
}
fn question_body(args: &Value) -> Result<String> {
    let args = if let Some(s) = args.as_str() {
        serde_json::from_str(s)?
    } else {
        args.clone()
    };
    let questions = args["questions"]
        .as_array()
        .context("invalid DSH questions")?;
    ensure!(!questions.is_empty(), "empty DSH questions");
    let body = questions
        .iter()
        .map(|q| q["question"].as_str().context("invalid DSH question"))
        .collect::<Result<Vec<_>>>()?
        .join("\n");
    Ok(text(&Value::String(body)))
}
fn pending_result(content: &Value) -> bool {
    content
        .as_array()
        .into_iter()
        .flatten()
        .find(|b| b["type"] == "text")
        .and_then(|b| b["text"].as_str())
        .and_then(|s| serde_json::from_str::<Value>(s).ok())
        .is_some_and(|v| v["pending"] == true)
}
impl State {
    fn consume(&mut self, line: &[u8]) -> Result<()> {
        let row: Value = serde_json::from_slice(line).context("invalid DSH event")?;
        if self.header.is_none() {
            ensure!(
                row["type"] == "session" && row["version"] == 4,
                "unsupported DSH format"
            );
            valid_id(row["id"].as_str().context("missing DSH identity")?)?;
            ensure!(
                row.get("origin").is_none() || row["origin"] == "subagent",
                "unknown DSH origin"
            );
            self.inherited = if row["isSeeded"] == true {
                row["inheritedEventCount"]
                    .as_u64()
                    .context("missing DSH inherited boundary")?
            } else {
                0
            };
            self.header = Some(row);
            return Ok(());
        }
        let seq = row["seq"].as_u64().context("missing DSH sequence")?;
        ensure!(seq < 200_000, "DSH source exceeds record limit");
        ensure!(seq == self.next_seq, "DSH sequence changed");
        self.next_seq = seq.checked_add(1).context("invalid DSH sequence")?;
        if seq < self.inherited {
            return Ok(());
        }
        let kind = row["type"].as_str().context("missing DSH event type")?;
        let data = &row["data"];
        let turn = data["turn"].as_u64();
        let make = |category: &str, identity: String, body: String| -> Result<NativeTurnEvent> {
            Ok(NativeTurnEvent {
                prompt_id: None,
                position: seq + 1,
                turn_id: identity,
                kind: category.into(),
                body,
                timestamp_ms: native_probe::timestamp(&row["time"])
                    .context("missing DSH event time")?,
            })
        };
        match kind {
            "turn/start" => {
                let turn = turn.context("missing DSH turn")?;
                ensure!(self.open_turn.is_none(), "overlapping DSH turns");
                self.open_turn = Some(turn);
                self.last_text.clear();
                self.events.retain(|e| e.kind != "failure");
            }
            "user/message" => {
                self.events.retain(|e| e.kind != "failure");
                if data["source"]["kind"] == "user-question-reply" {
                    if let Some(id) = data["source"]["callId"].as_str() {
                        self.pending.remove(&format!("question:{id}"));
                    }
                }
            }
            "step/start" | "llm/retry-started" => {
                self.events.retain(|e| e.kind != "failure");
            }
            "assistant/message" if turn == self.open_turn && turn.is_some() => {
                let body = text(&data["message"]["content"]);
                if !body.trim().is_empty() {
                    self.last_text = body;
                }
            }
            "session/title" => {
                self.title = text(&data["title"]);
            }
            "request/header" => {
                self.timed = data["header"]["tools"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|t| {
                        t["name"] == "ask_user_question"
                            && t["parameters"]["properties"].get("timeout").is_some()
                    });
            }
            "tool/call" if data["name"] == "ask_user_question" => {
                ensure!(
                    turn.is_some() && turn == self.open_turn,
                    "DSH question outside turn"
                );
                let id = data["callId"]
                    .as_str()
                    .context("missing DSH question identity")?;
                let event = make(
                    "reply_needed",
                    format!("turn:{}:question:{id}", turn.unwrap()),
                    question_body(&data["arguments"])?,
                )?;
                self.pending
                    .insert(format!("question:{id}"), (event, self.timed, false));
            }
            "tool/result" => {
                if let Some(id) = data["message"]["toolCallId"].as_str() {
                    let key = format!("question:{id}");
                    let continued = self.pending.get(&key).is_some_and(|(_, timed, _)| *timed)
                        && (pending_result(&data["message"]["content"])
                            || data["error"]["code"] == "TOOL_OUTCOME_UNKNOWN");
                    if continued {
                        if let Some((_, _, was_continued)) = self.pending.get_mut(&key) {
                            *was_continued = true;
                        }
                    } else {
                        self.pending.remove(&key);
                    }
                }
            }
            "tool/ptc-dispatch"
                if data["name"] == "ask_user_question"
                    && data["isError"] != true
                    && pending_result(&data["content"]) =>
            {
                let id = data["subCallId"]
                    .as_str()
                    .context("missing DSH question identity")?;
                let turn = self.open_turn.context("DSH question outside turn")?;
                let event = make(
                    "reply_needed",
                    format!("turn:{turn}:question:{id}"),
                    question_body(&data["arguments"])?,
                )?;
                self.pending
                    .insert(format!("question:{id}"), (event, true, true));
            }
            "approval/asked" => {
                let turn = self.open_turn.context("DSH approval outside turn")?;
                let id = data["id"]
                    .as_str()
                    .context("missing DSH approval identity")?;
                let body = if let Some(reason) = data["reason"].as_str().filter(|s| !s.is_empty()) {
                    text(&Value::String(reason.into()))
                } else {
                    "工具操作需要授权，请打开 DSH 处理。".into()
                };
                let event = make("authorization", format!("turn:{turn}:approval:{id}"), body)?;
                self.pending
                    .insert(format!("approval:{id}"), (event, false, false));
            }
            "approval/decided" => {
                let id = data["id"]
                    .as_str()
                    .context("missing DSH approval identity")?;
                self.pending.remove(&format!("approval:{id}"));
            }
            "turn/end" => {
                ensure!(
                    turn.is_some() && turn == self.open_turn,
                    "unmatched DSH turn end"
                );
                let turn = turn.unwrap();
                self.open_turn = None;
                self.pending.retain(|_, (_, _, continued)| *continued);
                let reason = data["reason"]["kind"]
                    .as_str()
                    .context("missing DSH turn outcome")?;
                let event = match reason {
                    "completed" => Some(make(
                        "completion",
                        format!("turn:{turn}"),
                        self.last_text.clone(),
                    )?),
                    "error" | "max-tokens" => Some(make(
                        "failure",
                        format!("turn:{turn}"),
                        "主回合未能完成，请打开 DSH 查看详情。".into(),
                    )?),
                    "aborted" | "blocked" => None,
                    _ => anyhow::bail!("unknown DSH turn outcome"),
                };
                if let Some(event) = event {
                    self.events.push_back(event);
                }
                while self.events.len() > EVENT_LIMIT {
                    self.events.pop_front();
                }
            }
            // Released v4 events that do not alter these four notification states.
            "agent-preset/selected"
            | "agent/inbox/spliced"
            | "approval/policy"
            | "assistant/attempt"
            | "assistant/message"
            | "command/done"
            | "command/run"
            | "compaction/end"
            | "compaction/prune"
            | "compaction/start"
            | "compaction/summary"
            | "deliverables/presented"
            | "developer/message"
            | "feedback/message-delete"
            | "feedback/message-put"
            | "feedback/record"
            | "goal/change"
            | "hook/invoked"
            | "hook/result"
            | "image/offload"
            | "job/registered"
            | "job/settled"
            | "llm/retry"
            | "model/selection"
            | "permission/preset"
            | "plan/mode"
            | "request/context"
            | "sandbox/mode"
            | "schedule/change"
            | "session-log-deepseek/delivery-accepted"
            | "session/end-seed"
            | "session/title-llm-request"
            | "step/end"
            | "subagent/catalog"
            | "subagent/descriptor"
            | "subagent/model-selection-policy"
            | "system/message"
            | "team/member"
            | "team/message/delivered"
            | "team/message/queued"
            | "team/task"
            | "todo/write"
            | "tool-workflow/agent-end"
            | "tool-workflow/agent-start"
            | "tool-workflow/run-end"
            | "tool-workflow/run-start"
            | "tool/call"
            | "tool/ptc-dispatch"
            | "tool/ptc-dispatch-start"
            | "web/deepseek-search-llm-request"
            | "working-directory/change"
            | "workspace/changes" => {}
            _ => ensure!(row["ignorable"] == true, "unknown required DSH event"),
        }
        ensure!(
            self.pending.len() <= EVENT_LIMIT,
            "too many DSH pending requests"
        );
        Ok(())
    }
    fn snapshot(
        &self,
        id: &str,
        session_key: &str,
        identity: &str,
    ) -> Result<Option<NativeStreamSnapshot>> {
        let header = self.header.as_ref().context("missing DSH header")?;
        ensure!(header["id"] == id, "DSH identity mismatch");
        if header["origin"] == "subagent" {
            return Ok(None);
        }
        let title =
            native_probe::notification_title(&self.title, id, header["cwd"].as_str().unwrap_or(""));
        let mut events = self.events.iter().cloned().collect::<Vec<_>>();
        events.extend(self.pending.values().map(|(e, _, _)| e.clone()));
        Ok(Some(NativeStreamSnapshot {
            provider: NativeProvider::Dsh,
            session_key: session_key.into(),
            id: id.into(),
            title,
            identity: identity.into(),
            record_count: self.next_seq,
            settled_count: self.next_seq,
            events,
        }))
    }
}

fn anchor(file: &mut File, offset: u64) -> Result<Vec<u8>> {
    let mut hash = Sha256::new();
    let n = offset.min(4096) as usize;
    let mut b = vec![0; n];
    file.seek(SeekFrom::Start(0))?;
    file.read_exact(&mut b)?;
    hash.update(&b);
    file.seek(SeekFrom::Start(offset.saturating_sub(n as u64)))?;
    file.read_exact(&mut b)?;
    hash.update(&b);
    Ok(hash.finalize().to_vec())
}
fn read(path: &Path, id: &str, key: &str) -> Result<Option<NativeStreamSnapshot>> {
    let before = file_stamp(path)?;
    ensure!(
        before.len <= FILE_LIMIT,
        "DSH notification source exceeds size limit"
    );
    let identity = native_probe::file_identity(path)?;
    let mut cache = CACHE
        .lock()
        .map_err(|_| anyhow::anyhow!("DSH reader unavailable"))?;
    let old = cache
        .iter()
        .position(|c| c.path == path)
        .and_then(|i| cache.remove(i));
    if old
        .as_ref()
        .is_some_and(|c| c.stamp == before && c.identity == identity)
    {
        let mut old = old.unwrap();
        old.last_accessed = std::time::Instant::now();
        let result = old.state.snapshot(id, key, &identity);
        cache.push_back(old);
        return result;
    }
    let mut file = regular_file(path)?;
    let append = old.as_ref().is_some_and(|c| {
        c.identity == identity
            && before.len > c.stamp.len
            && anchor(&mut file, c.offset).ok().as_ref() == Some(&c.anchor)
    });
    let mut current = if append {
        old.unwrap()
    } else {
        Cached {
            path: path.into(),
            stamp: before.clone(),
            identity: identity.clone(),
            offset: 0,
            anchor: vec![],
            state: State::default(),
            last_accessed: std::time::Instant::now(),
        }
    };
    file.seek(SeekFrom::Start(current.offset))?;
    let mut bytes = Vec::new();
    file.take(FILE_LIMIT + 1).read_to_end(&mut bytes)?;
    let compressed = path.extension().is_some_and(|x| x == "zstd");
    let mut consumed = 0;
    let mut decoded_total = 0;
    if compressed {
        while consumed < bytes.len() {
            let tail = &bytes[consumed..];
            // A writer may have flushed only part of its next frame. Publish
            // no bytes from that frame; consume it once its checksum is complete.
            let magic = [0x28, 0xb5, 0x2f, 0xfd];
            ensure!(
                tail[..tail.len().min(4)] == magic[..tail.len().min(4)],
                "invalid DSH compressed frame"
            );
            let size = match zstd::zstd_safe::find_frame_compressed_size(tail) {
                Ok(size) => size,
                Err(error) if zstd::zstd_safe::get_error_name(error) == "Src size is incorrect" => {
                    break
                }
                Err(_) => anyhow::bail!("invalid DSH compressed frame"),
            };
            ensure!(
                size > 0 && size <= tail.len(),
                "invalid DSH compressed frame"
            );
            let mut decoded = Vec::new();
            zstd::stream::read::Decoder::new(&tail[..size])?
                .take(FILE_LIMIT + 1)
                .read_to_end(&mut decoded)?;
            ensure!(
                decoded.len() as u64 <= FILE_LIMIT && decoded.ends_with(b"\n"),
                "invalid DSH compressed records"
            );
            decoded_total += decoded.len();
            ensure!(
                decoded_total as u64 <= FILE_LIMIT,
                "DSH decoded source exceeds limit"
            );
            consume_lines(&mut current.state, &decoded)?;
            consumed += size;
        }
    } else if let Some(last) = bytes.iter().rposition(|b| *b == b'\n') {
        consumed = last + 1;
        consume_lines(&mut current.state, &bytes[..consumed])?;
    }
    current.offset += consumed as u64;
    let mut file = regular_file(path)?;
    current.anchor = anchor(&mut file, current.offset)?;
    let after = file_stamp(path)?;
    ensure!(
        after == before && native_probe::file_identity(path)? == identity,
        "DSH source changed during read"
    );
    current.stamp = after;
    current.last_accessed = std::time::Instant::now();
    let result = current.state.snapshot(id, key, &identity)?;
    cache.push_back(current);
    while cache.len() > crate::resource_limits::SOURCE_CACHE_ENTRIES
        || cache
            .iter()
            .map(|c| {
                c.state
                    .events
                    .iter()
                    .chain(c.state.pending.values().map(|(e, _, _)| e))
                    .map(|e| e.body.len() + e.turn_id.len() + 256)
                    .sum::<usize>()
                    + c.state.header.as_ref().map_or(0, |h| h.to_string().len())
            })
            .sum::<usize>()
            > crate::resource_limits::NATIVE_PROBE_CACHE_BYTES as usize
    {
        cache.pop_front();
    }
    Ok(result)
}
fn consume_lines(state: &mut State, bytes: &[u8]) -> Result<()> {
    for line in bytes.split(|b| *b == b'\n').filter(|l| !l.is_empty()) {
        ensure!(line.len() <= LINE_LIMIT, "DSH record exceeds size limit");
        state.consume(line)?;
    }
    Ok(())
}

pub fn scan(paths: &DshPaths) -> Result<NativeScan> {
    let index = registry(paths)?;
    let root = paths.home.join("sessions");
    if !directory(&root)? {
        return Ok(NativeScan::default());
    }
    let mut scan = NativeScan::default();
    let mut ids = BTreeSet::new();
    let mut duplicates = BTreeSet::new();
    let mut count = 0;
    for project in fs::read_dir(&root)? {
        let project = project?;
        if !project.file_type()?.is_dir() {
            continue;
        }
        for entry in fs::read_dir(project.path())? {
            let entry = entry?;
            count += 1;
            ensure!(count <= 10_000, "DSH source inventory exceeds limit");
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let name = entry.file_name();
            let Some(id) = name.to_str().filter(|s| valid_id(s).is_ok()) else {
                continue;
            };
            if index
                .as_ref()
                .is_some_and(|index| index.archived.contains(id))
            {
                continue;
            }
            if !ids.insert(id.to_owned()) {
                duplicates.insert(id.to_owned());
                scan.errors += 1;
                continue;
            }
            let key = format!("{}/{id}", project.file_name().to_string_lossy());
            let paths = [
                entry.path().join("session.v4.jsonl"),
                entry.path().join("session.v4.jsonl.zstd"),
            ];
            let existing = paths.iter().filter(|p| p.exists()).collect::<Vec<_>>();
            if existing.len() != 1 {
                scan.errors += 1;
                continue;
            }
            match read(existing[0], id, &key) {
                Ok(Some(stream)) => scan.streams.push(stream),
                Ok(None) => {}
                Err(_) => scan.errors += 1,
            }
        }
    }
    scan.streams.retain(|s| !duplicates.contains(&s.id));
    Ok(scan)
}

#[cfg(test)]
mod tests;
