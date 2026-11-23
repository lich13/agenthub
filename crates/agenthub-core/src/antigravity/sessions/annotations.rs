use super::*;

#[derive(Debug, Default, Clone)]
pub(super) struct AnnotationOverlay {
    pub(super) title: Option<String>,
    pub(super) archived: Option<bool>,
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

    let tmp = paths
        .annotations_dir()
        .join(format!(".agenthub-ann-{}.tmp", uuid::Uuid::new_v4()));
    {
        let mut file = OpenOptions::new().write(true).create_new(true).open(&tmp)?;
        file.write_all(output.as_bytes())?;
        file.sync_all()?;
    }
    fs::rename(&tmp, &path)?;
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
pub(super) fn read_raw_summary_annotations(raw: &[u8]) -> AnnotationOverlay {
    let Some(top_fields) = parse_wire_fields(raw) else {
        return AnnotationOverlay::default();
    };
    let mut overlay = AnnotationOverlay::default();
    for field in top_fields {
        if field.number != 15 || field.wire_type != 2 {
            continue;
        }
        let Some(subfields) = parse_wire_fields(field.payload) else {
            return AnnotationOverlay::default();
        };
        for sub in subfields {
            match (sub.number, sub.wire_type) {
                (1, 2) => {
                    if let Ok(text) = std::str::from_utf8(sub.payload) {
                        if !text.trim().is_empty() {
                            overlay.title = Some(text.to_owned());
                        }
                    }
                }
                (4, 0) => {
                    let mut p = 0usize;
                    if let Some(v) = decode_varint(sub.payload, &mut p) {
                        overlay.archived = Some(v != 0);
                    }
                }
                _ => {}
            }
        }
    }
    overlay
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
