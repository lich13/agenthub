//! A bounded replica of one owner's private state. Never serialize the replica
//! to WebUI: consumers must project explicit fields and keep native permissions
//! and filesystem paths on the Rust side.
use super::{identity, DesktopError, DesktopEvent, Result};
use crate::resource_limits::CODEX_DESKTOP_FRAME_BYTES;
use serde_json::Value;

const MAX_PATCHES: usize = 16_384;
const MAX_PATH_DEPTH: usize = 64;

pub struct NativeState {
    thread_id: String,
    revision: Option<u64>,
    value: Value,
    native_queue_len: usize,
    retained_bytes: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeState {
    Unavailable,
    Idle,
    Active,
    Waiting,
}

impl NativeState {
    pub fn new(thread_id: &str) -> Result<Self> {
        uuid::Uuid::parse_str(thread_id).map_err(|_| DesktopError::InvalidIdentity)?;
        Ok(Self {
            thread_id: thread_id.into(),
            revision: None,
            value: Value::Null,
            native_queue_len: 0,
            retained_bytes: 0,
        })
    }

    /// A failed patch invalidates the whole replica. In-place application avoids
    /// copying large histories; no partially applied state can be used or sent.
    pub fn apply(&mut self, event: DesktopEvent) -> Result<()> {
        let result = self.apply_inner(event);
        if result.is_err() {
            self.value = Value::Null;
            self.revision = None;
            self.native_queue_len = 0;
            self.retained_bytes = 0;
        }
        result
    }

    fn apply_inner(&mut self, event: DesktopEvent) -> Result<()> {
        match event {
            DesktopEvent::Snapshot { revision, state } => {
                if self.revision.is_some_and(|previous| revision < previous) {
                    return Err(DesktopError::OutOfSync);
                }
                self.value = state;
                self.retained_bytes = retained_size(&self.value)?;
                self.validate()?;
                self.revision = Some(revision);
            }
            DesktopEvent::Patches { revision, change } => {
                if self.revision != change["baseRevision"].as_u64()
                    || self.revision.is_none_or(|previous| revision <= previous)
                {
                    return Err(DesktopError::OutOfSync);
                }
                let patches = change["patches"]
                    .as_array()
                    .filter(|patches| patches.len() <= MAX_PATCHES)
                    .ok_or(DesktopError::InvalidFrame)?;
                for patch in patches {
                    let (removed, added) = apply_patch(&mut self.value, patch)?;
                    self.retained_bytes = self
                        .retained_bytes
                        .saturating_sub(removed)
                        .checked_add(added)
                        .ok_or(DesktopError::FrameTooLarge)?;
                    if self.retained_bytes > CODEX_DESKTOP_FRAME_BYTES {
                        return Err(DesktopError::FrameTooLarge);
                    }
                }
                self.validate()?;
                self.revision = Some(revision);
            }
            DesktopEvent::Queue { messages } => self.native_queue_len = messages.len(),
        }
        Ok(())
    }

    fn validate(&self) -> Result<()> {
        if self.value["id"].as_str() != Some(&self.thread_id) {
            return Err(DesktopError::InvalidIdentity);
        }
        Ok(())
    }

    pub fn revision(&self) -> Option<u64> {
        self.revision
    }

    pub fn thread_id(&self) -> &str {
        &self.thread_id
    }

    pub fn project_directory(&self) -> Result<std::path::PathBuf> {
        let path =
            std::path::PathBuf::from(self.value["cwd"].as_str().ok_or(DesktopError::ReadOnly)?);
        if !path.is_absolute() {
            return Err(DesktopError::InvalidIdentity);
        }
        Ok(path)
    }

    pub fn runtime(&self) -> RuntimeState {
        match self.value["threadRuntimeStatus"]["type"].as_str() {
            Some("idle") if self.revision.is_some() => RuntimeState::Idle,
            Some("active") => {
                if self.pending_requests().next().is_some() {
                    RuntimeState::Waiting
                } else {
                    RuntimeState::Active
                }
            }
            _ => RuntimeState::Unavailable,
        }
    }

    pub fn check_writable(&self) -> Result<()> {
        if self.revision.is_none() || self.runtime() == RuntimeState::Unavailable {
            return Err(DesktopError::OutOfSync);
        }
        if self.value["hostId"].as_str() != Some("local")
            || !self.value["parentThreadId"].is_null()
            || self.value["archived"] == true
            || self.value["source"].get("subagent").is_some()
            || self.value["source"].get("subAgent").is_some()
            || self.value["threadSource"].get("subagent").is_some()
            || self.value["threadSource"].get("subAgent").is_some()
            || matches!(self.value["source"].as_str(), Some("subagent" | "subAgent"))
            || matches!(
                self.value["threadSource"].as_str(),
                Some("subagent" | "subAgent")
            )
        {
            return Err(DesktopError::ReadOnly);
        }
        Ok(())
    }

    pub fn check_idle(&self) -> Result<()> {
        self.check_writable()?;
        if self.runtime() != RuntimeState::Idle
            || self.native_queue_len != 0
            || self.pending_requests().next().is_some()
        {
            return Err(DesktopError::Busy);
        }
        Ok(())
    }

    pub fn pending_requests(&self) -> impl Iterator<Item = &Value> {
        self.value["requests"].as_array().into_iter().flatten()
    }

    pub fn pending_request(&self, request_id: &Value, method: &str) -> Result<&Value> {
        self.check_writable()?;
        if request_id.as_i64().is_none() && identity(request_id).is_err() {
            return Err(DesktopError::InvalidIdentity);
        }
        let request = self
            .pending_requests()
            .find(|request| &request["id"] == request_id && request["method"] == method)
            .ok_or(DesktopError::RequestExpired)?;
        if request["params"]["threadId"].as_str() != Some(&self.thread_id) {
            return Err(DesktopError::InvalidIdentity);
        }
        Ok(request)
    }

    /// Current desktop builds keep canonical history in entitiesByKey, with an
    /// empty legacy `turns` array. Do not mistake that for an empty thread.
    pub fn turns(&self) -> impl Iterator<Item = &Value> {
        let canonical = (self.value["turnHistory"]["kind"] == "canonical")
            .then(|| self.value["turnHistory"]["history"]["entitiesByKey"].as_object())
            .flatten();
        canonical
            .into_iter()
            .flat_map(|entities| entities.values())
            .chain(
                canonical
                    .is_none()
                    .then(|| self.value["turns"].as_array())
                    .flatten()
                    .into_iter()
                    .flatten(),
            )
    }

    pub fn active_turn_id(&self) -> Result<String> {
        self.check_writable()?;
        if !matches!(self.runtime(), RuntimeState::Active | RuntimeState::Waiting) {
            return Err(DesktopError::RequestExpired);
        }
        let mut active = self.turns().filter(|turn| turn["status"] == "inProgress");
        let turn = active.next().ok_or(DesktopError::OutOfSync)?;
        if active.next().is_some() {
            return Err(DesktopError::OutOfSync);
        }
        identity(turn.get("turnId").unwrap_or(&turn["id"]))
    }

    pub fn contains_submission(&self, message_id: &str) -> bool {
        self.submission_turn_id(message_id).is_some()
    }

    pub fn submission_turn_id(&self, message_id: &str) -> Option<String> {
        self.turns().find_map(|turn| {
            let present = turn["items"].as_array().into_iter().flatten().any(|item| {
                item["type"] == "userMessage" && item["clientId"].as_str() == Some(message_id)
            });
            present
                .then(|| identity(turn.get("turnId").unwrap_or(&turn["id"])).ok())
                .flatten()
        })
    }

    pub fn turn_finished(&self, turn_id: &str) -> bool {
        self.turns().any(|turn| {
            turn.get("turnId").unwrap_or(&turn["id"]).as_str() == Some(turn_id)
                && matches!(
                    turn["status"].as_str(),
                    Some("completed" | "interrupted" | "failed")
                )
        })
    }

    /// Internal native state access for strictly typed projections/builders.
    pub(super) fn value(&self) -> &Value {
        &self.value
    }
}

// Immer sends path components as arrays, not RFC 6901 pointers. Reject unknown
// operations and paths instead of guessing at a changed desktop protocol.
fn apply_patch(root: &mut Value, patch: &Value) -> Result<(usize, usize)> {
    let path = patch["path"]
        .as_array()
        .filter(|path| path.len() <= MAX_PATH_DEPTH)
        .ok_or(DesktopError::InvalidFrame)?;
    let op = patch["op"].as_str().ok_or(DesktopError::InvalidFrame)?;
    if !matches!(op, "add" | "replace" | "remove")
        || (op != "remove" && patch.get("value").is_none())
    {
        return Err(DesktopError::UnsupportedProtocol);
    }
    let Some((last, parents)) = path.split_last() else {
        if op == "remove" {
            return Err(DesktopError::InvalidFrame);
        }
        let sizes = (retained_size(root)?, retained_size(&patch["value"])?);
        *root = patch["value"].clone();
        return Ok(sizes);
    };
    let mut node = root;
    for key in parents {
        node = match node {
            Value::Object(object) => key.as_str().and_then(|key| object.get_mut(key)),
            Value::Array(array) => array_index(key).and_then(|index| array.get_mut(index)),
            _ => None,
        }
        .ok_or(DesktopError::OutOfSync)?;
    }
    let sizes = match node {
        Value::Object(object) => {
            let key = last.as_str().ok_or(DesktopError::InvalidFrame)?;
            if op != "add" && !object.contains_key(key) {
                return Err(DesktopError::OutOfSync);
            }
            let key_bytes = key.len() + 24;
            let removed = object
                .get(key)
                .map(retained_size)
                .transpose()?
                .map_or(0, |size| size + key_bytes);
            let added = if op == "remove" {
                0
            } else {
                retained_size(&patch["value"])? + key_bytes
            };
            if op == "remove" {
                object.remove(key);
            } else {
                object.insert(key.to_owned(), patch["value"].clone());
            }
            (removed, added)
        }
        Value::Array(array) => {
            let index = array_index(last).ok_or(DesktopError::InvalidFrame)?;
            if index > array.len() || (op != "add" && index == array.len()) {
                return Err(DesktopError::OutOfSync);
            }
            let removed = if op == "add" {
                0
            } else {
                retained_size(&array[index])?
            };
            let added = if op == "remove" {
                0
            } else {
                retained_size(&patch["value"])?
            };
            match op {
                "add" => array.insert(index, patch["value"].clone()),
                "remove" => {
                    array.remove(index);
                }
                _ => array[index] = patch["value"].clone(),
            }
            (removed, added)
        }
        _ => return Err(DesktopError::OutOfSync),
    };
    Ok(sizes)
}

fn array_index(value: &Value) -> Option<usize> {
    value.as_u64().and_then(|value| usize::try_from(value).ok())
}

// A retained-payload budget, not an RSS promise. Count initial state once; each
// patch charges only its removed/added subtree, never rescans the full history.
fn retained_size(value: &Value) -> Result<usize> {
    fn add(total: &mut usize, value: usize) -> Result<()> {
        *total = total
            .checked_add(value)
            .filter(|size| *size <= CODEX_DESKTOP_FRAME_BYTES)
            .ok_or(DesktopError::FrameTooLarge)?;
        Ok(())
    }
    let mut total = 24;
    match value {
        Value::String(text) => add(&mut total, text.len())?,
        Value::Array(array) => {
            for value in array {
                add(&mut total, retained_size(value)?)?;
            }
        }
        Value::Object(object) => {
            for (key, value) in object {
                add(&mut total, key.len() + 24)?;
                add(&mut total, retained_size(value)?)?;
            }
        }
        _ => {}
    }
    Ok(total)
}

#[cfg(test)]
#[path = "state_tests.rs"]
mod tests;
