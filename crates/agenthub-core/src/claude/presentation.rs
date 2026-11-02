//! Classify native user records before changing prompt or notification state.
use crate::user_message::{parse_user_message, UserContextBlock, UserContextKind, UserMessage};
use serde_json::Value;

// Only a complete native command envelope is rewritten. Fenced, quoted,
// incomplete and mixed prose examples remain literal user content.
fn command(text: &str) -> Option<String> {
    let first_line = text.trim_start_matches(['\r', '\n']);
    if first_line.starts_with("    ") || first_line.starts_with('\t') {
        return None;
    }
    let mut rest = text.trim();
    let mut name = None;
    let mut message = None;
    let mut args = None;
    while !rest.is_empty() {
        let (tag, slot) = if rest.starts_with("<command-name>") {
            ("command-name", &mut name)
        } else if rest.starts_with("<command-message>") {
            ("command-message", &mut message)
        } else if rest.starts_with("<command-args>") {
            ("command-args", &mut args)
        } else {
            return None;
        };
        if slot.is_some() {
            return None;
        }
        let body = rest.strip_prefix(&format!("<{tag}>"))?;
        let (value, tail) = body.split_once(&format!("</{tag}>"))?;
        *slot = Some(value);
        rest = tail.trim_start();
    }
    let name = name?.trim();
    let args = args?;
    message?;
    if !name.starts_with('/') || name.chars().any(char::is_whitespace) {
        return None;
    }
    Some(if args.is_empty() {
        name.into()
    } else {
        format!("{name}\n{args}")
    })
}

pub(super) fn user_content(key: &str, record: &Value, parts: &[Value]) -> (UserMessage, bool) {
    let projected: Vec<_> = parts
        .iter()
        .filter(|part| part["type"] != "tool_result")
        .map(|part| {
            let mut part = part.clone();
            if part["type"] == "text" {
                if let Some(text) = part["text"].as_str().and_then(command) {
                    part["text"] = text.into();
                }
            }
            part
        })
        .collect();
    let mut user = parse_user_message(key, &Value::Array(projected));
    let compact = record["isCompactSummary"] == true;
    if (record["isMeta"] == true || compact) && !user.text.is_empty() {
        user.context_blocks.push(UserContextBlock {
            kind: UserContextKind::Runtime,
            text: std::mem::take(&mut user.text),
        });
    }
    (user, compact)
}
