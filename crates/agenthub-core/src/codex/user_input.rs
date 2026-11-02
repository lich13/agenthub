//! Native structured questions. An asynchronous acknowledgement is not an answer.
use super::{
    rollout_events::{
        event_call_id, event_turn_id, normalize_canonical_turn, parse_raw_message_event,
        rollout_event_type,
    },
    PendingElicitation, UserInputOption, UserInputQuestion,
};
use anyhow::{ensure, Result};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs::File,
    io::{BufRead, BufReader},
    path::Path,
};

pub fn is_async_question_tool(name: &str) -> bool {
    matches!(
        name,
        "request_user_input_async" | "functions.request_user_input_async"
    )
}

pub fn is_question_tool(name: &str) -> bool {
    is_async_question_tool(name)
        || matches!(
            name,
            "RequestUserInput"
                | "request_user_input"
                | "functions.request_user_input"
                | "requestUserInput"
                | "item/tool/requestUserInput"
        )
}

pub(crate) fn question_tool(value: &Value) -> Option<&str> {
    let payload = value.get("payload").unwrap_or(value);
    [value, payload]
        .into_iter()
        .flat_map(|v| ["name", "toolName", "tool_name", "method", "type"].map(|key| v.get(key)))
        .flatten()
        .filter_map(Value::as_str)
        .find(|name| is_question_tool(name))
}

/// Normalize both native wire shapes without changing the public question DTO.
pub fn normalize_user_input_questions(args: &Value) -> Option<Vec<UserInputQuestion>> {
    let items = args.get("questions")?.as_array()?;
    if items.is_empty() {
        return None;
    }
    items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let text = item
                .get("question")
                .or_else(|| item.get("title"))?
                .as_str()?
                .trim();
            if text.is_empty() {
                return None;
            }
            let options = match item.get("options") {
                None | Some(Value::Null) => Vec::new(),
                Some(Value::Array(options)) => options
                    .iter()
                    .map(|option| {
                        let label = option
                            .as_str()
                            .or_else(|| {
                                option
                                    .get("label")
                                    .or_else(|| option.get("text"))
                                    .or_else(|| option.get("value"))
                                    .and_then(Value::as_str)
                            })?
                            .trim();
                        if label.is_empty() {
                            return None;
                        }
                        Some(UserInputOption {
                            label: label.to_owned(),
                            description: option
                                .get("description")
                                .and_then(Value::as_str)
                                .map(str::to_owned),
                        })
                    })
                    .collect::<Option<Vec<_>>>()?,
                _ => return None,
            };
            Some(UserInputQuestion {
                id: item
                    .get("id")
                    .and_then(Value::as_str)
                    .filter(|id| !id.trim().is_empty())
                    .map(str::to_owned)
                    .unwrap_or_else(|| format!("q{}", index + 1)),
                header: item
                    .get("header")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                question: text.to_owned(),
                options,
            })
        })
        .collect()
}

pub(crate) fn question_arguments(value: &Value) -> Option<Value> {
    let payload = value.get("payload").unwrap_or(value);
    let args = payload
        .get("arguments")
        .or_else(|| payload.pointer("/input/arguments"))
        .or_else(|| payload.get("params"))
        .or_else(|| value.get("params"))
        .unwrap_or(payload);
    match args {
        Value::String(text) => serde_json::from_str(text).ok(),
        other => Some(other.clone()),
    }
}

#[derive(Debug, Clone)]
pub struct AsyncQuestionCall {
    pub session_id: Option<String>,
    pub turn_id: String,
    pub call_id: String,
    pub created_at_ms: Option<i64>,
    pub line: usize,
    pub content_hash: String,
    questions: Vec<UserInputQuestion>,
    answered: HashSet<usize>,
    cancelled: bool,
}

impl AsyncQuestionCall {
    pub fn pending(&self) -> Option<PendingElicitation> {
        let questions: Vec<_> = self
            .questions
            .iter()
            .enumerate()
            .filter(|(i, _)| !self.answered.contains(i))
            .map(|(_, q)| q.clone())
            .collect();
        (!self.cancelled && !questions.is_empty()).then(|| PendingElicitation {
            turn_id: Some(self.turn_id.clone()),
            item_id: Some(self.call_id.clone()),
            questions,
        })
    }
}

#[derive(Default, Clone)]
pub(crate) struct AsyncQuestionTracker {
    pub calls: Vec<AsyncQuestionCall>,
    session_id: Option<String>,
}

impl AsyncQuestionTracker {
    pub fn push(&mut self, value: &Value, line: usize) {
        let payload = value.get("payload").unwrap_or(value);
        if value.get("type").and_then(Value::as_str) == Some("session_meta") {
            self.session_id = payload.get("id").and_then(Value::as_str).map(str::to_owned);
            return;
        }
        let event_kind = rollout_event_type(value);
        let kind = if matches!(event_kind, "response_item" | "") {
            payload
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or(event_kind)
        } else {
            event_kind
        };
        let turn = event_turn_id(value);
        if matches!(kind, "task_started" | "turn_started" | "turn/started") {
            for call in &mut self.calls {
                if turn.as_deref() != Some(&call.turn_id) {
                    call.cancelled = true;
                }
            }
        }
        if matches!(
            kind,
            "turn_aborted" | "turn/aborted" | "turn_cancelled" | "turn/cancelled" | "turn_error"
        ) || (matches!(kind, "task_complete" | "turn_completed")
            && matches!(
                payload.get("status").and_then(Value::as_str),
                Some("cancelled" | "canceled" | "aborted" | "failed")
            ))
        {
            for call in &mut self.calls {
                if turn.as_deref() == Some(&call.turn_id) {
                    call.cancelled = true;
                }
            }
        }
        if question_tool(value).is_some_and(is_async_question_tool) {
            let Some((turn_id, call_id)) = turn.zip(event_call_id(value)) else {
                return;
            };
            if turn_id.is_empty()
                || call_id.is_empty()
                || self
                    .calls
                    .iter()
                    .any(|c| c.turn_id == turn_id && c.call_id == call_id)
            {
                return;
            }
            let Some(questions) = question_arguments(value)
                .as_ref()
                .and_then(normalize_user_input_questions)
            else {
                return;
            };
            let content_hash = hex::encode(Sha256::digest(
                serde_json::to_vec(&questions).unwrap_or_default(),
            ));
            self.calls.push(AsyncQuestionCall {
                session_id: self.session_id.clone(),
                turn_id,
                call_id,
                questions,
                content_hash,
                line,
                created_at_ms: value
                    .get("timestamp")
                    .and_then(Value::as_str)
                    .and_then(|v| chrono::DateTime::parse_from_rfc3339(v).ok())
                    .map(|v| v.timestamp_millis()),
                answered: HashSet::new(),
                cancelled: false,
            });
            return;
        }
        if matches!(kind, "function_call_output" | "custom_tool_call_output") {
            let payload = value.get("payload").unwrap_or(value);
            let output = payload.get("output").and_then(|v| match v {
                Value::String(s) => serde_json::from_str::<Value>(s).ok(),
                v => Some(v.clone()),
            });
            if output.as_ref().is_some_and(|v| {
                v.get("accepted") == Some(&Value::Bool(false))
                    || v.get("cancelled") == Some(&Value::Bool(true))
                    || v.get("error").is_some()
            }) {
                for call in &mut self.calls {
                    if event_call_id(value).as_deref() == Some(&call.call_id)
                        && turn.as_deref() == Some(&call.turn_id)
                    {
                        call.cancelled = true;
                    }
                }
            }
        }
        let Some(message) = parse_raw_message_event(value).filter(|m| m.role == "user") else {
            return;
        };
        let Some(answers) = native_question_answers(&message.text) else {
            return;
        };
        for (call_id, index) in answers {
            if let Some(call) = self
                .calls
                .iter_mut()
                .find(|c| c.call_id == call_id && turn.as_deref().is_none_or(|t| t == c.turn_id))
            {
                if index < call.questions.len() {
                    call.answered.insert(index);
                }
            }
        }
    }

    pub fn pending(&self) -> impl Iterator<Item = &AsyncQuestionCall> {
        self.calls.iter().filter(|call| call.pending().is_some())
    }
}

/// Only the complete native user envelope is executable metadata. Examples stay text.
fn native_question_answers(text: &str) -> Option<Vec<(String, usize)>> {
    let body = text
        .trim()
        .strip_prefix("<send_user_message_question_reply>")?
        .strip_suffix("</send_user_message_question_reply>")?;
    let items: Vec<Value> = serde_json::from_str(body).ok()?;
    if items.is_empty() {
        return None;
    }
    items
        .iter()
        .map(|item| {
            item.get("answer")?.as_str()?;
            item.get("question")?.as_str()?;
            let identity: Vec<Value> =
                serde_json::from_str(item.get("questionItemId")?.as_str()?).ok()?;
            if identity.len() != 3 || !is_async_question_tool(identity[0].as_str()?) {
                return None;
            }
            Some((
                identity[1].as_str()?.to_owned(),
                usize::try_from(identity[2].as_u64()?).ok()?,
            ))
        })
        .collect()
}

pub fn rollout_async_questions(path: &Path) -> Result<Vec<AsyncQuestionCall>> {
    static CACHE: crate::read_cache::ReadCache<Vec<AsyncQuestionCall>> =
        crate::read_cache::ReadCache::new(crate::resource_limits::CODEX_QUESTION_CACHE_BYTES);
    CACHE.read(
        path,
        |calls, _| {
            calls
                .iter()
                .map(|c| 512 + serde_json::to_vec(&c.questions).map_or(0, |v| v.len() as u64))
                .sum()
        },
        || {
            let file = File::open(path)?;
            let before = file.metadata()?;
            let mut reader = BufReader::new(file);
            let mut tracker = AsyncQuestionTracker::default();
            let mut turn = None;
            let mut line = Vec::new();
            let mut index = 0;
            loop {
                line.clear();
                if reader.read_until(b'\n', &mut line)? == 0 {
                    break;
                }
                index += 1;
                // Parsing the entire line rejects an unfinished answer instead of using stale state.
                if line.iter().all(u8::is_ascii_whitespace) {
                    continue;
                }
                let mut value: Value = serde_json::from_slice(&line)?;
                normalize_canonical_turn(&mut value, &mut turn);
                tracker.push(&value, index);
            }
            let after = std::fs::metadata(path)?;
            ensure!(
                before.len() == after.len() && before.modified()? == after.modified()?,
                "question transcript changed while reading"
            );
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                ensure!(
                    before.dev() == after.dev() && before.ino() == after.ino(),
                    "question transcript was replaced"
                );
            }
            Ok(tracker.calls)
        },
    )
}

#[cfg(test)]
mod tests;
