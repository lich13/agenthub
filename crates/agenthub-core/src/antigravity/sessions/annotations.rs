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

pub(super) fn invalidate_proto_cache(paths: &AntigravityPaths) {
    let cache = paths.proto_cache();
    if cache.exists() {
        let _ = fs::remove_file(cache);
    }
}
