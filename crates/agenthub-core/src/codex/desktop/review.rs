//! Bounded, borrowed approval projection. These are presentation limits, not
//! permission grants; the historical 16/64 KiB approval guards still apply.
use crate::services::codex_desktop::{DesktopFileReview, DesktopPermissionsReview, DesktopReview};
use serde::{ser::SerializeMap, Serialize, Serializer};
use serde_json::Value;
use std::io::{self, Write};

pub(super) const BODY_LIMIT: usize = 16 * 1024;
pub(super) const PARAMS_LIMIT: usize = 64 * 1024;
const REVIEW_LIMIT: usize = 64 * 1024;
const DIFF_LIMIT: usize = 32 * 1024;
const FILE_LIMIT: usize = 100;

/// Overlay native item changes without cloning potentially large histories.
pub(super) struct Params<'a> {
    pub original: &'a Value,
    pub changes: Option<&'a Value>,
    pub omit: &'a [&'a str],
}
impl Params<'_> {
    pub fn get(&self, key: &str) -> &Value {
        if key == "changes" {
            self.changes.unwrap_or(&self.original[key])
        } else {
            &self.original[key]
        }
    }
}
impl Serialize for Params<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let Some(object) = self.original.as_object() else {
            return self.original.serialize(serializer);
        };
        let mut map = serializer.serialize_map(None)?;
        let mut changes = self.changes.filter(|_| !self.omit.contains(&"changes"));
        for (key, value) in object {
            // Match serde_json's sorted map order, including an added changes key.
            if key.as_str() >= "changes" {
                if let Some(value) = changes.take() {
                    map.serialize_entry("changes", value)?;
                }
            }
            if self.omit.contains(&key.as_str()) || key == "changes" && self.changes.is_some() {
                continue;
            }
            map.serialize_entry(key, value)?;
        }
        if let Some(value) = changes {
            map.serialize_entry("changes", value)?;
        }
        map.end()
    }
}

struct LimitedWriter {
    bytes: Vec<u8>,
    limit: usize,
}
impl Write for LimitedWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let count = bytes.len().min(self.limit.saturating_sub(self.bytes.len()));
        self.bytes.extend_from_slice(&bytes[..count]);
        if count < bytes.len() {
            return Err(io::Error::other("approval projection limit"));
        }
        Ok(count)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub(super) fn json(value: &impl Serialize, limit: usize, pretty: bool) -> (String, bool) {
    let mut writer = LimitedWriter {
        bytes: Vec::new(),
        limit,
    };
    let complete = if pretty {
        serde_json::to_writer_pretty(&mut writer, value).is_ok()
    } else {
        serde_json::to_writer(&mut writer, value).is_ok()
    };
    if let Err(error) = std::str::from_utf8(&writer.bytes) {
        writer.bytes.truncate(error.valid_up_to());
    }
    (
        String::from_utf8(writer.bytes).unwrap_or_default(),
        !complete,
    )
}

struct Budget {
    remaining: usize,
    truncated: bool,
}
impl Budget {
    fn text(&mut self, value: &str, limit: usize) -> (String, bool) {
        let mut end = value.len().min(limit).min(self.remaining);
        while !value.is_char_boundary(end) {
            end -= 1;
        }
        let truncated = end < value.len();
        self.truncated |= truncated;
        self.remaining -= end;
        (value[..end].to_owned(), truncated)
    }
    fn string(&mut self, value: &Value) -> Option<String> {
        value.as_str().map(|text| self.text(text, REVIEW_LIMIT).0)
    }
    fn value(&mut self, value: &Value) -> Option<String> {
        if value.is_null() {
            return None;
        }
        let (text, truncated) = json(value, self.remaining, true);
        self.remaining -= text.len();
        self.truncated |= truncated;
        Some(text)
    }
}

pub(super) fn project(params: &Params<'_>, kind: &str) -> DesktopReview {
    let mut budget = Budget {
        remaining: REVIEW_LIMIT,
        truncated: false,
    };
    let command = (kind == "command")
        .then(|| budget.string(params.get("command")))
        .flatten();
    let cwd = budget.string(params.get("cwd"));
    let reason = budget.string(params.get("reason"));
    let mut changes = Vec::new();
    if kind == "file" {
        if let Some(files) = params.get("changes").as_array() {
            budget.truncated |= files.len() > FILE_LIMIT;
            for file in files.iter().take(FILE_LIMIT) {
                let (path, path_cut) =
                    budget.text(file["path"].as_str().unwrap_or(""), REVIEW_LIMIT);
                let (kind, kind_cut) = if let Some(kind) = file["kind"].as_str() {
                    budget.text(kind, REVIEW_LIMIT)
                } else {
                    let (kind, cut) = json(&file["kind"], budget.remaining, false);
                    budget.remaining -= kind.len();
                    budget.truncated |= cut;
                    (kind, cut)
                };
                let (diff, diff_cut) = budget.text(file["diff"].as_str().unwrap_or(""), DIFF_LIMIT);
                changes.push(DesktopFileReview {
                    path,
                    kind,
                    diff,
                    truncated: path_cut || kind_cut || diff_cut,
                });
            }
        }
    }
    let permissions = (kind == "permissions").then(|| DesktopPermissionsReview {
        network: budget.value(&params.get("permissions")["network"]),
        file_system: budget.value(&params.get("permissions")["fileSystem"]),
    });
    // Preserve every field not fully represented above. In particular file
    // changes and permissions may contain native fields we do not understand:
    // retain the bounded original under disclosure instead of silently omitting.
    let mut omit = Vec::new();
    if command.is_some() {
        omit.push("command");
    }
    if cwd.is_some() {
        omit.push("cwd");
    }
    if reason.is_some() {
        omit.push("reason");
    }
    if kind == "file"
        && params.get("changes").as_array().is_some_and(|files| {
            files.iter().all(|file| {
                file.as_object().is_some_and(|fields| {
                    fields
                        .keys()
                        .all(|key| matches!(key.as_str(), "path" | "kind" | "diff"))
                })
            })
        })
    {
        omit.push("changes");
    }
    if kind == "permissions"
        && params.get("permissions").as_object().is_some_and(|fields| {
            fields
                .keys()
                .all(|key| matches!(key.as_str(), "network" | "fileSystem"))
        })
    {
        omit.push("permissions");
    }
    let extra_params = Params {
        original: params.original,
        changes: params.changes,
        omit: &omit,
    };
    let (extra, cut) = json(&extra_params, budget.remaining, true);
    budget.truncated |= cut;
    DesktopReview {
        command,
        cwd,
        reason,
        changes,
        permissions,
        extra: (extra != "{}" && extra != "null").then_some(extra),
        truncated: budget.truncated,
    }
}
