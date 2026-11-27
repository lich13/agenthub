use super::*;

#[derive(Debug, Default, Clone)]
pub(crate) struct AnnotationOverlay {
    pub(crate) title: Option<String>,
    pub(crate) archived: Option<bool>,
}

fn unescape_prototext_string(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'\\' || i + 1 >= bytes.len() {
            out.push(bytes[i]);
            i += 1;
            continue;
        }
        let next = bytes[i + 1];
        match next {
            b'n' => {
                out.push(b'\n');
                i += 2;
            }
            b'r' => {
                out.push(b'\r');
                i += 2;
            }
            b't' => {
                out.push(b'\t');
                i += 2;
            }
            b'\\' => {
                out.push(b'\\');
                i += 2;
            }
            b'"' => {
                out.push(b'"');
                i += 2;
            }
            b'\'' => {
                out.push(b'\'');
                i += 2;
            }
            b'0'..=b'7' => {
                let mut val = (next - b'0') as u32;
                let mut consumed = 2;
                for offset in 2..=3 {
                    if i + offset < bytes.len() && (b'0'..=b'7').contains(&bytes[i + offset]) {
                        val = val * 8 + (bytes[i + offset] - b'0') as u32;
                        consumed += 1;
                    } else {
                        break;
                    }
                }
                out.push((val & 0xff) as u8);
                i += consumed;
            }
            b'x' | b'X' if i + 3 < bytes.len() => {
                if let Ok(hex_byte) =
                    u8::from_str_radix(std::str::from_utf8(&bytes[i + 2..i + 4]).unwrap_or(""), 16)
                {
                    out.push(hex_byte);
                    i += 4;
                } else {
                    out.push(next);
                    i += 2;
                }
            }
            _ => {
                out.push(next);
                i += 2;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn escape_prototext_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 8);
    for &byte in value.as_bytes() {
        match byte {
            b'\\' => out.push_str("\\\\"),
            b'"' => out.push_str("\\\""),
            b'\n' => out.push_str("\\n"),
            b'\r' => out.push_str("\\r"),
            b'\t' => out.push_str("\\t"),
            0x20..=0x7e => out.push(byte as char),
            _ => out.push_str(&format!("\\{:03o}", byte)),
        }
    }
    out
}

fn parse_pbtxt_annotations(content: &str) -> AnnotationOverlay {
    let mut overlay = AnnotationOverlay::default();
    let mut depth = 0usize;
    for line in content.lines() {
        let trimmed = line.trim();
        if depth == 0 {
            if let Some(rest) = trimmed.strip_prefix("title:") {
                let rest = rest.trim();
                if rest.len() >= 2 && rest.starts_with('"') && rest.ends_with('"') {
                    let parsed = unescape_prototext_string(&rest[1..rest.len() - 1]);
                    if !parsed.trim().is_empty() {
                        overlay.title = Some(parsed);
                    }
                }
            } else if let Some(rest) = trimmed.strip_prefix("archived:") {
                match rest.trim() {
                    "true" => overlay.archived = Some(true),
                    "false" => overlay.archived = Some(false),
                    _ => {}
                }
            }
        }
        for ch in trimmed.chars() {
            if ch == '{' {
                depth = depth.saturating_add(1);
            } else if ch == '}' {
                depth = depth.saturating_sub(1);
            }
        }
    }
    overlay
}

pub(super) fn read_annotation_overlay(paths: &AntigravityPaths, id: &str) -> AnnotationOverlay {
    let path = paths.annotations_dir().join(format!("{id}.pbtxt"));
    fs::read_to_string(path)
        .map(|text| parse_pbtxt_annotations(&text))
        .unwrap_or_default()
}

pub(super) fn write_annotation_overlay(
    paths: &AntigravityPaths,
    id: &str,
    new_title: Option<&str>,
    new_archived: Option<bool>,
) -> Result<()> {
    fs::create_dir_all(paths.annotations_dir())?;
    let path = paths.annotations_dir().join(format!("{id}.pbtxt"));
    let existing = fs::read_to_string(&path).unwrap_or_default();
    let current = parse_pbtxt_annotations(&existing);
    let title = new_title.map(str::to_owned).or(current.title);
    let archived = new_archived.or(current.archived);

    let mut preserved = Vec::new();
    let mut skip_block_depth = 0usize;
    for line in existing.lines() {
        let trimmed = line.trim();
        if skip_block_depth > 0 {
            for ch in trimmed.chars() {
                if ch == '{' {
                    skip_block_depth += 1;
                } else if ch == '}' {
                    skip_block_depth = skip_block_depth.saturating_sub(1);
                }
            }
            continue;
        }
        if trimmed.starts_with("title:") || trimmed.starts_with("archived:") {
            continue;
        }
        if trimmed.starts_with("archival_status_timestamp") && new_archived.is_some() {
            let opens = trimmed.chars().filter(|&c| c == '{').count();
            let closes = trimmed.chars().filter(|&c| c == '}').count();
            skip_block_depth = opens.saturating_sub(closes);
            continue;
        }
        if !trimmed.is_empty() {
            preserved.push(line.to_owned());
        }
    }

    let mut output = String::new();
    if let Some(title) = title.filter(|t| !t.trim().is_empty()) {
        output.push_str(&format!("title: \"{}\"\n", escape_prototext_string(&title)));
    }
    if let Some(flag) = archived {
        output.push_str(&format!(
            "archived: {}\n",
            if flag { "true" } else { "false" }
        ));
        if new_archived.is_some() {
            let secs = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            output.push_str(&format!(
                "archival_status_timestamp {{\n  seconds: {secs}\n}}\n"
            ));
        }
    }
    for line in preserved {
        output.push_str(&line);
        output.push('\n');
    }

    let existing_meta = match fs::symlink_metadata(&path) {
        Ok(meta) => {
            ensure!(
                meta.is_file() && !meta.file_type().is_symlink(),
                "拒绝覆盖非普通文件注解"
            );
            Some(meta)
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
        Err(err) => return Err(err.into()),
    };
    let existing_perms = existing_meta.as_ref().map(fs::Metadata::permissions);
    session_delete::atomic_replace_file(
        &path,
        "agenthub-ann",
        None,
        existing_perms,
        |file| {
            file.write_all(output.as_bytes())?;
            Ok(())
        },
        || {
            match fs::symlink_metadata(&path) {
                Ok(after) => {
                    ensure!(
                        after.is_file() && !after.file_type().is_symlink(),
                        "拒绝覆盖非普通文件注解"
                    );
                    if let Some(before) = &existing_meta {
                        #[cfg(unix)]
                        {
                            use std::os::unix::fs::MetadataExt;
                            ensure!(
                                (before.dev(), before.ino()) == (after.dev(), after.ino()),
                                "注解文件在写入前已被替换，请重试"
                            );
                        }
                    }
                }
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                    ensure!(existing_meta.is_none(), "注解文件在写入前已被移除，请重试");
                }
                Err(err) => return Err(err.into()),
            }
            Ok(())
        },
    )?;
    Ok(())
}

pub(super) const PROTO_CACHE_FILES: [&str; 4] = [
    "agyhub_summaries_proto.pb",
    "jetbox_summaries_proto.pb",
    "agyhub_summaries.pb",
    "jetbox_summaries.pb",
];

#[derive(Clone, Copy)]
struct WireField<'a> {
    number: u64,
    wire_type: u8,
    raw: &'a [u8],
    payload: &'a [u8],
}

fn decode_varint(bytes: &[u8], pos: &mut usize) -> Option<u64> {
    let mut value = 0u64;
    let mut shift = 0u32;
    while *pos < bytes.len() && shift < 64 {
        let byte = bytes[*pos];
        *pos += 1;
        if shift == 63 && byte > 1 {
            return None;
        }
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Some(value);
        }
        shift += 7;
    }
    None
}

fn encode_varint(mut value: u64, out: &mut Vec<u8>) {
    while value >= 0x80 {
        out.push(((value as u8) & 0x7f) | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}

fn encode_len_field(field_number: u64, payload: &[u8], out: &mut Vec<u8>) {
    encode_varint((field_number << 3) | 2, out);
    encode_varint(payload.len() as u64, out);
    out.extend_from_slice(payload);
}

fn parse_wire_fields(bytes: &[u8]) -> Option<Vec<WireField<'_>>> {
    let mut fields = Vec::new();
    let mut pos = 0usize;
    while pos < bytes.len() {
        let field_start = pos;
        let tag = decode_varint(bytes, &mut pos)?;
        let number = tag >> 3;
        if number == 0 {
            return None;
        }
        let wire_type = (tag & 0x07) as u8;
        let payload = match wire_type {
            0 => {
                let val_start = pos;
                decode_varint(bytes, &mut pos)?;
                &bytes[val_start..pos]
            }
            1 => {
                let end = pos.checked_add(8)?;
                if end > bytes.len() {
                    return None;
                }
                let slice = &bytes[pos..end];
                pos = end;
                slice
            }
            2 => {
                let len = usize::try_from(decode_varint(bytes, &mut pos)?).ok()?;
                let end = pos.checked_add(len)?;
                if end > bytes.len() {
                    return None;
                }
                let slice = &bytes[pos..end];
                pos = end;
                slice
            }
            5 => {
                let end = pos.checked_add(4)?;
                if end > bytes.len() {
                    return None;
                }
                let slice = &bytes[pos..end];
                pos = end;
                slice
            }
            _ => return None,
        };
        fields.push(WireField {
            number,
            wire_type,
            raw: &bytes[field_start..pos],
            payload,
        });
    }
    Some(fields)
}

/// Reads `.exa.jetski_cortex_pb.ConversationAnnotations` (field 15 of `CascadeTrajectorySummary`):
/// field 1 = `title` (string), field 4 = `archived` (bool), field 5 = `archival_status_timestamp`.
#[cfg(test)]
pub(super) fn read_raw_summary_annotations(raw: &[u8]) -> AnnotationOverlay {
    parse_raw_summary(raw).annotations
}

#[derive(Debug, Default, Clone)]
pub(crate) struct ParsedWaitingStep {
    pub(crate) step_index: u64,
    pub(crate) kind: &'static str,
    pub(crate) body: String,
    pub(crate) proof_hash: String,
}

#[derive(Debug, Default, Clone)]
pub(crate) struct ParsedTrajectoryMeta {
    pub(crate) parent_conversation_id: Option<String>,
    pub(crate) root_conversation_id: Option<String>,
    pub(crate) workspace_uris: Vec<String>,
    pub(crate) has_subagent_spec: bool,
    pub(crate) nesting_depth: u64,
}

impl ParsedTrajectoryMeta {
    pub(crate) fn is_subagent(&self, self_id: &str) -> bool {
        self.parent_conversation_id
            .as_deref()
            .is_some_and(|p| !p.is_empty() && p != self_id)
            || self.nesting_depth > 0
            || self.has_subagent_spec
            || self
                .root_conversation_id
                .as_deref()
                .is_some_and(|r| !r.is_empty() && r != self_id)
    }

    pub(crate) fn effective_parent(&self, self_id: &str) -> Option<&str> {
        self.parent_conversation_id
            .as_deref()
            .filter(|p| !p.is_empty() && *p != self_id)
            .or_else(|| {
                self.root_conversation_id
                    .as_deref()
                    .filter(|r| !r.is_empty() && *r != self_id)
            })
    }
}

#[derive(Debug, Default, Clone)]
pub(crate) struct ParsedRawSummary {
    pub(crate) trajectory_id: Option<String>,
    pub(crate) status: u64,
    pub(crate) created_time_ms: Option<i64>,
    pub(crate) waiting_steps: Vec<ParsedWaitingStep>,
    pub(crate) workspaces: Vec<String>,
    pub(crate) annotations: AnnotationOverlay,
    pub(crate) trajectory_meta: ParsedTrajectoryMeta,
    pub(crate) has_active_children: bool,
    pub(crate) not_fully_idle: bool,
    pub(crate) killed: bool,
    pub(crate) interrupted: bool,
    pub(crate) latest_executor_termination_reason: u64,
    pub(crate) background_task_count: u64,
    pub(crate) has_running_subagents: bool,
}

impl ParsedRawSummary {
    pub(crate) fn is_termination_failure(&self) -> bool {
        matches!(self.latest_executor_termination_reason, 1 | 3 | 6 | 11)
    }

    pub(crate) fn is_user_canceled_or_killed(&self) -> bool {
        self.killed || self.interrupted || self.latest_executor_termination_reason == 2
    }

    pub(crate) fn active_status(&self) -> Option<&'static str> {
        if !self.waiting_steps.is_empty() {
            return Some("waiting");
        }
        if matches!(self.status, 2..=4)
            || self.not_fully_idle
            || self.has_active_children
            || self.has_running_subagents
            || self.background_task_count > 0
        {
            return Some("running");
        }
        if self.is_termination_failure() {
            return Some("failed");
        }
        None
    }
}

fn decode_field_varint(field: &WireField<'_>) -> Option<u64> {
    if field.wire_type != 0 {
        return None;
    }
    let mut p = 0usize;
    decode_varint(field.payload, &mut p)
}

fn decode_clean_utf8(bytes: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(bytes).ok()?;
    if text
        .chars()
        .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
    {
        return None;
    }
    (!text.trim().is_empty()).then(|| text.to_owned())
}

/// Decodes `CortexStepUserInput` (`step_payload` field 19) from offline `.db` steps:
/// prefers `f19.f2` (`user_response`), falling back to `f19.f1.f1` (`items[].text`),
/// and never reads `f19.f3` (expanded workflow prompts) or `f19.f12` (prompt section names).
pub(crate) fn extract_db_user_input_text(blob: &[u8]) -> Option<String> {
    let top_fields = parse_wire_fields(blob)?;
    let mut user_response: Option<String> = None;
    let mut item_texts: Vec<String> = Vec::new();
    for field in top_fields {
        if field.number != 19 || field.wire_type != 2 {
            continue;
        }
        let Some(subfields) = parse_wire_fields(field.payload) else {
            continue;
        };
        for sub in subfields {
            if sub.wire_type != 2 {
                continue;
            }
            match sub.number {
                2 => {
                    if let Some(s) = decode_clean_utf8(sub.payload) {
                        user_response = Some(s);
                    }
                }
                1 => {
                    if let Some(item_fields) = parse_wire_fields(sub.payload) {
                        for item_f in item_fields {
                            if item_f.number == 1 && item_f.wire_type == 2 {
                                if let Some(s) = decode_clean_utf8(item_f.payload) {
                                    item_texts.push(s);
                                }
                            }
                        }
                    } else if let Some(s) = decode_clean_utf8(sub.payload) {
                        item_texts.push(s);
                    }
                }
                _ => {}
            }
        }
    }
    if let Some(raw) = user_response {
        let cleaned = super::prompt::extract_antigravity_user_request(&raw);
        if !cleaned.trim().is_empty() {
            return Some(cleaned);
        }
    }
    if !item_texts.is_empty() {
        let cleaned = super::prompt::extract_antigravity_user_request(&item_texts.join(""));
        if !cleaned.trim().is_empty() {
            return Some(cleaned);
        }
    }
    None
}

/// Decodes `CortexStepPlannerResponse` (`step_payload` field 20) from offline `.db` steps:
/// returns `(reply, thinking)` where `reply` prefers `f20.f8` (`modified_response`) over `f20.f1` (`response`),
/// and `thinking` reads `f20.f3`, excluding tool call payloads.
pub(crate) fn extract_db_planner_texts(blob: &[u8]) -> (Option<String>, Option<String>) {
    let Some(top_fields) = parse_wire_fields(blob) else {
        return (None, None);
    };
    let mut response: Option<String> = None;
    let mut modified_response: Option<String> = None;
    let mut thinking: Option<String> = None;
    for field in top_fields {
        if field.number != 20 || field.wire_type != 2 {
            continue;
        }
        let Some(subfields) = parse_wire_fields(field.payload) else {
            continue;
        };
        for sub in subfields {
            if sub.wire_type != 2 {
                continue;
            }
            match sub.number {
                1 => {
                    if let Some(s) = decode_clean_utf8(sub.payload) {
                        response = Some(s.trim().to_owned());
                    }
                }
                3 => {
                    if let Some(s) = decode_clean_utf8(sub.payload) {
                        thinking = Some(s.trim().to_owned());
                    }
                }
                8 => {
                    if let Some(s) = decode_clean_utf8(sub.payload) {
                        modified_response = Some(s.trim().to_owned());
                    }
                }
                _ => {}
            }
        }
    }
    (modified_response.or(response), thinking)
}

/// Decodes `CortexStepErrorMessage` (`step_payload` field 24) from offline `.db` steps:
/// reads `f24.f3.f2` (user-facing short error) with fallback to `f24.f3.f1` (or direct `f24.f1`),
/// excluding `f24.f3.f3` internal stack traces.
pub(crate) fn extract_db_error_text(blob: &[u8]) -> Option<String> {
    let top_fields = parse_wire_fields(blob)?;
    let mut user_error: Option<String> = None;
    let mut fallback_error: Option<String> = None;
    let mut direct_error: Option<String> = None;
    for field in top_fields {
        if field.number != 24 || field.wire_type != 2 {
            continue;
        }
        let Some(subfields) = parse_wire_fields(field.payload) else {
            continue;
        };
        for sub in subfields {
            if sub.wire_type != 2 {
                continue;
            }
            match sub.number {
                3 => {
                    if let Some(err_fields) = parse_wire_fields(sub.payload) {
                        for ef in err_fields {
                            if ef.wire_type != 2 {
                                continue;
                            }
                            match ef.number {
                                2 => {
                                    if let Some(s) = decode_clean_utf8(ef.payload) {
                                        user_error = Some(s.trim().to_owned());
                                    }
                                }
                                1 => {
                                    if let Some(s) = decode_clean_utf8(ef.payload) {
                                        fallback_error = Some(s.trim().to_owned());
                                    }
                                }
                                _ => {}
                            }
                        }
                    } else if let Some(s) = decode_clean_utf8(sub.payload) {
                        fallback_error = Some(s.trim().to_owned());
                    }
                }
                1 | 2 => {
                    if let Some(s) = decode_clean_utf8(sub.payload) {
                        direct_error = Some(s.trim().to_owned());
                    }
                }
                _ => {}
            }
        }
    }
    user_error.or(fallback_error).or(direct_error)
}

fn extract_waiting_prompt_texts(bytes: &[u8]) -> Vec<String> {
    if let Some(direct) = decode_clean_utf8(bytes) {
        return vec![direct.trim().to_owned()];
    }
    let Some(fields) = parse_wire_fields(bytes) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for f in fields {
        if f.wire_type != 2 || !matches!(f.number, 1 | 2) {
            continue;
        }
        if let Some(s) = decode_clean_utf8(f.payload) {
            let t = s.trim();
            if uuid::Uuid::parse_str(t).is_err() {
                out.push(t.to_owned());
            }
            continue;
        }
        if let Some(subfields) = parse_wire_fields(f.payload) {
            for sub in subfields {
                if sub.wire_type == 2 && matches!(sub.number, 1 | 2) {
                    if let Some(s) = decode_clean_utf8(sub.payload) {
                        let t = s.trim();
                        if uuid::Uuid::parse_str(t).is_err() {
                            out.push(t.to_owned());
                        }
                    }
                }
            }
        }
    }
    out
}

pub(crate) fn parse_trajectory_metadata_blob(raw: &[u8]) -> ParsedTrajectoryMeta {
    let Some(fields) = parse_wire_fields(raw) else {
        return ParsedTrajectoryMeta::default();
    };
    let mut meta = ParsedTrajectoryMeta::default();
    for f in fields {
        match (f.number, f.wire_type) {
            (5, 2) => {
                if let Ok(s) = std::str::from_utf8(f.payload) {
                    let s = s.trim();
                    if !s.is_empty() {
                        meta.parent_conversation_id = Some(s.to_owned());
                    }
                }
            }
            (6, 2) => {
                if let Ok(s) = std::str::from_utf8(f.payload) {
                    let s = s.trim();
                    if !s.is_empty() {
                        meta.root_conversation_id = Some(s.to_owned());
                    }
                }
            }
            (7, 2) => {
                if let Ok(s) = std::str::from_utf8(f.payload) {
                    let s = s.trim();
                    if !s.is_empty() {
                        meta.workspace_uris.push(s.to_owned());
                    }
                }
            }
            (8, 2) if !f.payload.is_empty() => {
                meta.has_subagent_spec = true;
            }
            (17, 0) => {
                if let Some(v) = decode_field_varint(&f) {
                    meta.nesting_depth = v;
                }
            }
            _ => {}
        }
    }
    meta
}

fn parse_waiting_step(raw: &[u8]) -> Option<ParsedWaitingStep> {
    let fields = parse_wire_fields(raw)?;
    let mut step_bytes: Option<&[u8]> = None;
    let mut step_index = 0u64;
    for f in fields {
        match (f.number, f.wire_type) {
            (1, 2) => step_bytes = Some(f.payload),
            (2, 0) => {
                if let Some(idx) = decode_field_varint(&f) {
                    step_index = idx;
                }
            }
            _ => {}
        }
    }
    let step_payload = step_bytes?;
    let step_fields = parse_wire_fields(step_payload)?;
    let mut ask_payload: Option<&[u8]> = None;
    let mut has_permissions = false;
    let mut req_payload: Option<&[u8]> = None;
    for sf in step_fields {
        match (sf.number, sf.wire_type) {
            (154, 2) => ask_payload = Some(sf.payload),
            (133, 2) if !sf.payload.is_empty() => has_permissions = true,
            (56, 2) if !sf.payload.is_empty() => req_payload = Some(sf.payload),
            _ => {}
        }
    }
    let proof_hash = hex::encode(Sha256::digest(step_payload));
    if let Some(ask) = ask_payload {
        let texts = extract_waiting_prompt_texts(ask);
        let body = if texts.is_empty() {
            "需要回复，请打开 Antigravity 处理。".into()
        } else {
            texts.join("\n")
        };
        return Some(ParsedWaitingStep {
            step_index,
            kind: "reply_needed",
            body,
            proof_hash,
        });
    }
    if has_permissions {
        return Some(ParsedWaitingStep {
            step_index,
            kind: "authorization",
            body: "操作需要授权，请打开 Antigravity 处理。".into(),
            proof_hash,
        });
    }
    if let Some(req) = req_payload {
        let texts = extract_waiting_prompt_texts(req);
        let looks_like_question = texts.iter().any(|t| t.contains('?') || t.contains('？'));
        let (kind, body) = if looks_like_question {
            ("reply_needed", texts.join("\n"))
        } else {
            (
                "authorization",
                "操作需要授权，请打开 Antigravity 处理。".into(),
            )
        };
        return Some(ParsedWaitingStep {
            step_index,
            kind,
            body,
            proof_hash,
        });
    }
    None
}

pub(crate) fn parse_raw_summary(raw: &[u8]) -> ParsedRawSummary {
    let Some(top_fields) = parse_wire_fields(raw) else {
        return ParsedRawSummary::default();
    };
    let mut parsed = ParsedRawSummary::default();
    for field in top_fields {
        match (field.number, field.wire_type) {
            (4, 2) => {
                if let Ok(s) = std::str::from_utf8(field.payload) {
                    let s = s.trim();
                    if !s.is_empty() {
                        parsed.trajectory_id = Some(s.to_owned());
                    }
                }
            }
            (5, 0) => {
                if let Some(v) = decode_field_varint(&field) {
                    parsed.status = v;
                }
            }
            (7, 2) => {
                if let Some(ts_fields) = parse_wire_fields(field.payload) {
                    let mut secs: i64 = 0;
                    let mut nanos: i64 = 0;
                    for tf in ts_fields {
                        match tf.number {
                            1 => {
                                if let Some(v) = decode_field_varint(&tf) {
                                    secs = v as i64;
                                }
                            }
                            2 => {
                                if let Some(v) = decode_field_varint(&tf) {
                                    nanos = v as i64;
                                }
                            }
                            _ => {}
                        }
                    }
                    if secs > 86_400 {
                        parsed.created_time_ms = Some(
                            secs.saturating_mul(1000)
                                .saturating_add(nanos.clamp(0, 999_999_999) / 1_000_000),
                        );
                    }
                }
            }
            (8, 2) => {
                if let Some(ws) = parse_waiting_step(field.payload) {
                    parsed.waiting_steps.push(ws);
                }
            }
            (9, 2) => {
                if let Some(ws_fields) = parse_wire_fields(field.payload) {
                    for wf in ws_fields {
                        if wf.number == 1 && wf.wire_type == 2 {
                            if let Ok(uri) = std::str::from_utf8(wf.payload) {
                                let uri = uri.trim();
                                if !uri.is_empty() {
                                    parsed.workspaces.push(uri.to_owned());
                                }
                            }
                        }
                    }
                }
            }
            (15, 2) => {
                if let Some(subfields) = parse_wire_fields(field.payload) {
                    for sub in subfields {
                        match (sub.number, sub.wire_type) {
                            (1, 2) => {
                                if let Ok(text) = std::str::from_utf8(sub.payload) {
                                    if !text.trim().is_empty() {
                                        parsed.annotations.title = Some(text.to_owned());
                                    }
                                }
                            }
                            (4, 0) => {
                                if let Some(v) = decode_field_varint(&sub) {
                                    parsed.annotations.archived = Some(v != 0);
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
            (17, 2) => parsed.trajectory_meta = parse_trajectory_metadata_blob(field.payload),
            (18, 0) => parsed.has_active_children = decode_field_varint(&field).unwrap_or(0) != 0,
            (21, 0) => parsed.not_fully_idle = decode_field_varint(&field).unwrap_or(0) != 0,
            (23, 0) => parsed.killed = decode_field_varint(&field).unwrap_or(0) != 0,
            (25, 0) => parsed.interrupted = decode_field_varint(&field).unwrap_or(0) != 0,
            (28, 0) => {
                parsed.latest_executor_termination_reason =
                    decode_field_varint(&field).unwrap_or(0);
            }
            (29, 0) => parsed.background_task_count = decode_field_varint(&field).unwrap_or(0),
            (30, 0) => parsed.has_running_subagents = decode_field_varint(&field).unwrap_or(0) != 0,
            _ => {}
        }
    }
    parsed
}

/// Losslessly patches `CascadeTrajectorySummary` field 15 (`ConversationAnnotations`):
/// updates field 1 (`title`), field 4 (`archived`), and field 5 (`archival_status_timestamp`),
/// preserving all other top-level and annotation subfields byte-for-byte.
/// Returns `None` if `raw` is empty or is not valid protobuf wire data.
pub(super) fn patch_raw_summary_annotations(
    raw: &[u8],
    new_title: Option<&str>,
    new_archived: Option<bool>,
    now_secs: u64,
) -> Option<Vec<u8>> {
    if raw.is_empty() || (new_title.is_none() && new_archived.is_none()) {
        return None;
    }
    let top_fields = parse_wire_fields(raw)?;
    let mut preserved_top = Vec::with_capacity(raw.len() + 64);
    let mut existing_ann_subfields = Vec::new();
    for field in top_fields {
        if field.number == 15 {
            if field.wire_type != 2 {
                return None;
            }
            existing_ann_subfields.extend(parse_wire_fields(field.payload)?);
        } else {
            preserved_top.extend_from_slice(field.raw);
        }
    }

    let mut new_ann = Vec::new();
    if let Some(title) = new_title {
        if !title.trim().is_empty() {
            encode_len_field(1, title.as_bytes(), &mut new_ann);
        }
    }
    if let Some(archived) = new_archived {
        encode_varint(4 << 3, &mut new_ann);
        encode_varint(u64::from(archived), &mut new_ann);
        let mut ts_bytes = Vec::with_capacity(12);
        encode_varint(1 << 3, &mut ts_bytes);
        encode_varint(now_secs, &mut ts_bytes);
        encode_len_field(5, &ts_bytes, &mut new_ann);
    }
    for sub in existing_ann_subfields {
        if new_title.is_some() && sub.number == 1 {
            continue;
        }
        if new_archived.is_some() && matches!(sub.number, 4 | 5) {
            continue;
        }
        new_ann.extend_from_slice(sub.raw);
    }

    encode_len_field(15, &new_ann, &mut preserved_top);
    Some(preserved_top)
}

pub(super) fn invalidate_proto_cache(paths: &AntigravityPaths) {
    for name in PROTO_CACHE_FILES {
        let cache = paths.home.join(name);
        if session_delete::within(&paths.home, &cache) {
            let _ = fs::remove_file(cache);
        }
    }
}
