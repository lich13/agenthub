use super::{envelope, FileMention, UserContextBlock, UserContextKind};
use std::ops::Range;

pub(super) struct Projection {
    pub text: String,
    pub context_blocks: Vec<UserContextBlock>,
    pub files: Vec<FileMention>,
}

// These are native context envelopes, not arbitrary XML. Actions, cancellations
// and question replies have their own presentation and are deliberately absent.
const TAGS: &[(&str, UserContextKind)] = &[
    ("local-command-stdout", UserContextKind::Runtime),
    ("local-command-stderr", UserContextKind::Runtime),
    ("local-command-caveat", UserContextKind::Runtime),
    ("task-notification", UserContextKind::Runtime),
    ("system-reminder", UserContextKind::Runtime),
    ("system_reminder", UserContextKind::Runtime),
    ("environment_context", UserContextKind::Environment),
    ("permissions instructions", UserContextKind::Runtime),
    ("app-context", UserContextKind::Runtime),
    ("collaboration_mode", UserContextKind::Runtime),
    ("skills_instructions", UserContextKind::Runtime),
    ("plugins_instructions", UserContextKind::Runtime),
    ("subagent_notification", UserContextKind::Runtime),
    ("subagent_context", UserContextKind::Runtime),
    ("codex_internal_context", UserContextKind::Runtime),
    ("goal_context", UserContextKind::Runtime),
    ("additional_context", UserContextKind::Runtime),
    ("user_instructions", UserContextKind::Instructions),
    (
        "legacy_unified_exec_process_limit_warning",
        UserContextKind::Runtime,
    ),
    (
        "legacy_apply_patch_exec_command_warning",
        UserContextKind::Runtime,
    ),
    ("legacy_model_mismatch_warning", UserContextKind::Runtime),
];

pub(super) fn project(text: &str) -> Projection {
    let mut result = Projection {
        text: String::new(),
        context_blocks: Vec::new(),
        files: Vec::new(),
    };
    if !text.contains('<') && !text.contains("# Files mentioned by the user:") {
        result.text = text.to_owned();
        return result;
    }
    let literal = literal_ranges(text);
    let lower = text.to_ascii_lowercase();
    let mut cursor = 0;
    let mut kept = 0;
    let mut adjacent = false;
    let mut line_start = 0;
    while cursor < text.len() {
        if let Some(range) = protected(&literal, cursor) {
            advance(text, &mut cursor, range.end, &mut line_start);
            adjacent = false;
            continue;
        }
        if adjacent
            || (cursor - line_start <= 3
                && text[line_start..cursor].bytes().all(|byte| byte == b' '))
        {
            let rest = &text[cursor..];
            if rest.starts_with("# Files mentioned by the user:") {
                if let Some((header_end, files)) = envelope(rest) {
                    let context_start = if kept == 0
                        && text[..cursor].chars().all(|ch| matches!(ch, '\r' | '\n'))
                    {
                        0
                    } else {
                        cursor
                    };
                    result.text.push_str(&text[kept..context_start]);
                    result.context_blocks.push(UserContextBlock {
                        kind: UserContextKind::Files,
                        text: text[context_start..cursor + header_end].to_owned(),
                    });
                    result.files.extend(files);
                    let end = cursor + header_end;
                    advance(text, &mut cursor, end, &mut line_start);
                    kept = cursor;
                    adjacent = true;
                    continue;
                }
                break;
            }
            let instruction_open = instruction_header_end(text, cursor).and_then(|end| {
                let offset = text[end..].len() - text[end..].trim_start().len();
                let opening = end + offset;
                opening_end(&lower, opening, "instructions").map(|end| (opening, end))
            });
            let matched = instruction_open
                .map(|(_, end)| ("instructions", UserContextKind::Instructions, end))
                .or_else(|| {
                    TAGS.iter().find_map(|&(name, kind)| {
                        opening_end(&lower, cursor, name).map(|end| (name, kind, end))
                    })
                });
            if let Some((name, kind, opening_end)) = matched {
                if let Some(end) = closing_end(&lower, opening_end, name, &literal) {
                    result.text.push_str(&text[kept..cursor]);
                    result.context_blocks.push(UserContextBlock {
                        kind,
                        text: text[cursor..end].to_owned(),
                    });
                    advance(text, &mut cursor, end, &mut line_start);
                    kept = end;
                    adjacent = true;
                    continue;
                }
                // An unfinished native envelope remains wholly literal; do not
                // extract a complete-looking child from an unfinished parent.
                break;
            }
            // A known tag inside an ordinary XML example is still an example.
            if let Some(rest) = lower[cursor..].strip_prefix('<') {
                let name: String = rest
                    .chars()
                    .take_while(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | ':'))
                    .collect();
                if name.starts_with(|ch: char| ch.is_ascii_alphabetic()) {
                    if let Some(opening) = opening_end(&lower, cursor, &name) {
                        let Some(end) = closing_end(&lower, opening, &name, &literal) else {
                            break;
                        };
                        advance(text, &mut cursor, end, &mut line_start);
                        adjacent = false;
                        continue;
                    }
                    break;
                }
            }
        }
        let ch = text[cursor..].chars().next().unwrap();
        if !ch.is_whitespace() {
            adjacent = false;
        }
        if ch == '\n' {
            line_start = cursor + 1;
        }
        cursor += ch.len_utf8();
    }
    result.text.push_str(&text[kept..]);
    result
}

fn advance(text: &str, cursor: &mut usize, end: usize, line_start: &mut usize) {
    if let Some(newline) = text[*cursor..end].rfind('\n') {
        *line_start = *cursor + newline + 1;
    }
    *cursor = end;
}

fn instruction_header_end(text: &str, start: usize) -> Option<usize> {
    let end = text[start..].find('\n').map(|index| start + index + 1)?;
    let line = text[start..end].trim();
    let label = line.trim_start_matches('#').trim().trim_matches('\x60');
    let lower = label.to_ascii_lowercase();
    (lower == "agents.md"
        || lower == "agents.md instructions"
        || lower.starts_with("agents.md instructions for ")
        || (!lower.chars().any(char::is_whitespace)
            && (lower.ends_with("/agents.md") || lower.ends_with("\\agents.md"))))
    .then_some(end)
}

fn opening_end(lower: &str, start: usize, name: &str) -> Option<usize> {
    let rest = lower.get(start..)?.strip_prefix('<')?.strip_prefix(name)?;
    if !rest.starts_with('>') && !rest.starts_with(char::is_whitespace) {
        return None;
    }
    let length = rest.find('>')?;
    let attributes = &rest[..length];
    if attributes.contains('<') || attributes.trim_end().ends_with('/') {
        return None;
    }
    Some(start + name.len() + length + 2)
}

fn closing_end(
    lower: &str,
    mut cursor: usize,
    name: &str,
    literal: &[Range<usize>],
) -> Option<usize> {
    let close = format!("</{name}");
    let mut depth = 1;
    while cursor < lower.len() {
        let start = cursor + lower[cursor..].find('<')?;
        if let Some(range) = protected(literal, start) {
            cursor = range.end;
            continue;
        }
        if let Some(rest) = lower[start..].strip_prefix(&close) {
            let whitespace = rest.len() - rest.trim_start().len();
            if rest[whitespace..].starts_with('>') {
                depth -= 1;
                let end = start + close.len() + whitespace + 1;
                if depth == 0 {
                    return Some(end);
                }
                cursor = end;
                continue;
            }
        }
        if let Some(end) = opening_end(lower, start, name) {
            depth += 1;
            cursor = end;
        } else {
            cursor = start + 1;
        }
    }
    None
}

fn protected(ranges: &[Range<usize>], at: usize) -> Option<&Range<usize>> {
    let index = ranges.partition_point(|range| range.end <= at);
    ranges.get(index).filter(|range| range.start <= at)
}

// Preserve source spans for fences, block quotes, indented examples and inline
// code. No Markdown serialization or native file rewrite is involved.
pub(super) fn literal_ranges(text: &str) -> Vec<Range<usize>> {
    let mut blocks: Vec<Range<usize>> = Vec::new();
    let mut offset = 0;
    let mut fence: Option<(u8, usize)> = None;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_start_matches(' ');
        let indent = line.len() - trimmed.len();
        let marker = trimmed.as_bytes().first().copied().unwrap_or_default();
        let count = trimmed.bytes().take_while(|byte| *byte == marker).count();
        let fence_line = indent <= 3 && matches!(marker, b'\x60' | b'~') && count >= 3;
        let was_fenced = fence.is_some();
        if let Some((active, length)) = fence {
            if fence_line
                && marker == active
                && count >= length
                && trimmed[count..].trim().is_empty()
            {
                fence = None;
            }
        } else if fence_line {
            fence = Some((marker, count));
        }
        if was_fenced || fence_line || indent >= 4 || trimmed.starts_with(['>', '\t']) {
            if let Some(last) = blocks.last_mut().filter(|last| last.end == offset) {
                last.end += line.len();
            } else {
                blocks.push(offset..offset + line.len());
            }
        }
        offset += line.len();
    }
    let mut inline = Vec::new();
    let bytes = text.as_bytes();
    let mut cursor = 0;
    while cursor < bytes.len() {
        if let Some(range) = protected(&blocks, cursor) {
            cursor = range.end;
            continue;
        }
        if bytes[cursor] != b'\x60' {
            cursor += 1;
            continue;
        }
        let run = bytes[cursor..]
            .iter()
            .take_while(|&&byte| byte == b'\x60')
            .count();
        let mut end = cursor + run;
        let mut matched = None;
        while end < bytes.len() && protected(&blocks, end).is_none() {
            if bytes[end] == b'\x60' {
                let next = bytes[end..]
                    .iter()
                    .take_while(|&&byte| byte == b'\x60')
                    .count();
                if next == run {
                    matched = Some(end + next);
                    break;
                }
                end += next;
            } else {
                end += 1;
            }
        }
        if let Some(end) = matched {
            inline.push(cursor..end);
            cursor = end;
        } else {
            cursor += run;
        }
    }
    blocks.extend(inline);
    let mut comment_cursor = 0;
    while let Some(start) = text[comment_cursor..]
        .find("<!--")
        .map(|start| comment_cursor + start)
    {
        let end = text[start + 4..]
            .find("-->")
            .map_or(text.len(), |end| start + 4 + end + 3);
        blocks.push(start..end);
        comment_cursor = end;
    }
    blocks.sort_unstable_by_key(|range| range.start);
    let mut merged: Vec<Range<usize>> = Vec::new();
    for range in blocks {
        if let Some(last) = merged.last_mut().filter(|last| last.end >= range.start) {
            last.end = last.end.max(range.end);
        } else {
            merged.push(range);
        }
    }
    merged
}
