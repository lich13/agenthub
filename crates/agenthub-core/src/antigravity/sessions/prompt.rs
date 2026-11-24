use serde_json::Value;
use std::io::{self, BufRead, Read};
use std::ops::ControlFlow;

pub(crate) const TRANSCRIPT_LINE_LIMIT: usize = 8 * 1024 * 1024;

const SYSTEM_MESSAGE_PREAMBLE: &str =
    "The following is a <SYSTEM_MESSAGE> not actually sent by the user.";

pub(crate) fn drain_until_newline<R: BufRead + ?Sized>(reader: &mut R) -> io::Result<()> {
    loop {
        let buf = reader.fill_buf()?;
        if buf.is_empty() {
            return Ok(());
        }
        if let Some(pos) = buf.iter().position(|&b| b == b'\n') {
            reader.consume(pos + 1);
            return Ok(());
        }
        let len = buf.len();
        reader.consume(len);
    }
}

pub(crate) fn for_each_bounded_jsonl_line<R, F>(reader: &mut R, mut on_line: F) -> io::Result<()>
where
    R: BufRead,
    F: FnMut(&[u8]) -> ControlFlow<()>,
{
    let mut line = Vec::new();
    loop {
        line.clear();
        let read = reader
            .take(TRANSCRIPT_LINE_LIMIT as u64 + 1)
            .read_until(b'\n', &mut line)?;
        if read == 0 {
            break;
        }
        if line.len() > TRANSCRIPT_LINE_LIMIT {
            if line.last() != Some(&b'\n') {
                drain_until_newline(reader)?;
            }
            continue;
        }
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        if on_line(&line).is_break() {
            break;
        }
    }
    Ok(())
}

pub(crate) fn for_each_bounded_jsonl_row<R, F>(reader: &mut R, mut on_row: F) -> io::Result<()>
where
    R: BufRead,
    F: FnMut(Value) -> ControlFlow<()>,
{
    for_each_bounded_jsonl_line(reader, |line| {
        if let Ok(row) = serde_json::from_slice::<Value>(line) {
            on_row(row)
        } else {
            ControlFlow::Continue(())
        }
    })
}

fn starts_with_truncated_marker(s: &str) -> bool {
    let Some(rest) = s.strip_prefix("<truncated ") else {
        return false;
    };
    let digits_len = rest.bytes().take_while(u8::is_ascii_digit).count();
    if digits_len == 0 {
        return false;
    }
    let after_digits = &rest[digits_len..];
    after_digits.starts_with(" bytes>") || after_digits.starts_with(" chars>")
}

fn is_antigravity_trailing_suffix(s: &str) -> bool {
    let trimmed = s.trim_start();
    trimmed.is_empty()
        || trimmed.starts_with("<ADDITIONAL_METADATA>")
        || trimmed.starts_with("<USER_SETTINGS_CHANGE>")
        || trimmed.starts_with("<SYSTEM_MESSAGE>")
        || trimmed.starts_with("<EPHEMERAL_MESSAGE>")
        || trimmed.starts_with(SYSTEM_MESSAGE_PREAMBLE)
        || starts_with_truncated_marker(trimmed)
}

fn strip_leading_system_envelopes(mut s: &str) -> (&str, bool) {
    let mut stripped_any = false;
    loop {
        let trimmed = s.trim_start();
        if trimmed.starts_with("<USER_REQUEST>") {
            return (trimmed, stripped_any);
        }
        if let Some(after_preamble) = trimmed.strip_prefix(SYSTEM_MESSAGE_PREAMBLE) {
            stripped_any = true;
            if let Some(open_idx) = after_preamble.find("<SYSTEM_MESSAGE>") {
                let from_tag = &after_preamble[open_idx..];
                if let Some(close_idx) = from_tag.find("</SYSTEM_MESSAGE>") {
                    s = &from_tag[close_idx + "</SYSTEM_MESSAGE>".len()..];
                    continue;
                }
            }
            return ("", true);
        }
        let mut matched_tag = false;
        for (open, close) in [
            ("<USER_SETTINGS_CHANGE>", "</USER_SETTINGS_CHANGE>"),
            ("<ADDITIONAL_METADATA>", "</ADDITIONAL_METADATA>"),
            ("<SYSTEM_MESSAGE>", "</SYSTEM_MESSAGE>"),
            ("<EPHEMERAL_MESSAGE>", "</EPHEMERAL_MESSAGE>"),
        ] {
            if trimmed.starts_with(open) {
                stripped_any = true;
                matched_tag = true;
                if let Some(close_idx) = trimmed.find(close) {
                    s = &trimmed[close_idx + close.len()..];
                } else {
                    return ("", true);
                }
                break;
            }
        }
        if !matched_tag {
            return (if stripped_any { trimmed } else { s }, stripped_any);
        }
    }
}

fn find_line_start_marker(s: &str, marker: &str) -> Option<usize> {
    s.match_indices(marker)
        .map(|(idx, _)| idx)
        .find(|&idx| idx == 0 || s.as_bytes()[idx - 1] == b'\n')
}

fn strip_trailing_antigravity_metadata(s: &str) -> &str {
    let mut cut = s.len();
    for marker in [
        "<ADDITIONAL_METADATA>",
        "<USER_SETTINGS_CHANGE>",
        "<EPHEMERAL_MESSAGE>",
        SYSTEM_MESSAGE_PREAMBLE,
    ] {
        if let Some(idx) = find_line_start_marker(s, marker) {
            cut = cut.min(idx);
        }
    }
    if cut == s.len() {
        return s;
    }
    strip_single_trailing_newline(&s[..cut])
}

fn strip_unclosed_truncated_tail(s: &str) -> &str {
    for (idx, _) in s.match_indices("<truncated ") {
        if starts_with_truncated_marker(&s[idx..]) {
            let after = &s[idx..];
            let close_pos = after.find('>').map_or(after.len(), |p| p + 1);
            let tail = after[close_pos..].trim();
            if tail.is_empty()
                || tail.ends_with("</ADDITIONAL_METADATA>")
                || tail.ends_with("</USER_SETTINGS_CHANGE>")
                || tail.ends_with("</SYSTEM_MESSAGE>")
                || tail.contains("<ADDITIONAL_METADATA>")
                || tail.contains("<USER_SETTINGS_CHANGE>")
            {
                return strip_single_trailing_newline(&s[..idx]);
            }
        }
    }
    s
}

fn strip_single_trailing_newline(s: &str) -> &str {
    s.strip_suffix("\r\n")
        .or_else(|| s.strip_suffix('\n'))
        .unwrap_or(s)
}

fn strip_single_leading_newline(s: &str) -> &str {
    s.strip_prefix("\r\n")
        .or_else(|| s.strip_prefix('\n'))
        .unwrap_or(s)
}

pub(crate) fn is_transcript_user_row(row: &Value) -> bool {
    let st = row["type"].as_str().unwrap_or("");
    let src = row["source"].as_str().unwrap_or("");
    st != "SYSTEM_MESSAGE"
        && (matches!(st, "USER_INPUT" | "REQUEST") || matches!(src, "USER_EXPLICIT" | "USER"))
}

pub(crate) fn transcript_row_has_user_payload(row: &Value) -> bool {
    let has_text = row["content"]
        .as_str()
        .map(extract_antigravity_user_request)
        .is_some_and(|s| !s.trim().is_empty());
    let has_media = row["media"]
        .as_array()
        .is_some_and(|arr| arr.iter().any(|item| item["uri"].as_str().is_some()));
    has_text || has_media
}

fn is_bare_wrapper_placeholder(trimmed: &str) -> bool {
    matches!(
        trimmed,
        "<USER_REQUEST>"
            | "</USER_REQUEST>"
            | "<ADDITIONAL_METADATA>"
            | "</ADDITIONAL_METADATA>"
            | "<USER_SETTINGS_CHANGE>"
            | "</USER_SETTINGS_CHANGE>"
            | "<SYSTEM_MESSAGE>"
            | "</SYSTEM_MESSAGE>"
            | "<EPHEMERAL_MESSAGE>"
            | "</EPHEMERAL_MESSAGE>"
            | "<original_task>"
            | "</original_task>"
            | "<prior_attempt>"
            | "<instructions>"
    )
}

pub(crate) fn clean_antigravity_auto_title(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || is_bare_wrapper_placeholder(trimmed) {
        return None;
    }
    if !trimmed.starts_with('<')
        && !trimmed.starts_with(SYSTEM_MESSAGE_PREAMBLE)
        && !trimmed.contains('\n')
        && !trimmed.contains("<ADDITIONAL_METADATA>")
        && !trimmed.contains("<USER_SETTINGS_CHANGE>")
        && !trimmed.contains("<truncated ")
    {
        return Some(trimmed.to_owned());
    }
    let cleaned = extract_antigravity_user_request(trimmed);
    cleaned
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !is_bare_wrapper_placeholder(line))
        .map(|line| line.chars().take(80).collect())
}

pub(crate) fn clean_antigravity_subagent_title(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || is_bare_wrapper_placeholder(trimmed) {
        return None;
    }
    if !trimmed.starts_with('<')
        && !trimmed.starts_with("[Message] ")
        && !trimmed.starts_with(SYSTEM_MESSAGE_PREAMBLE)
        && !trimmed.contains('\n')
        && !trimmed.contains("<original_task>")
    {
        return Some(trimmed.to_owned());
    }
    let cleaned = clean_antigravity_delegation(trimmed);
    cleaned
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !is_bare_wrapper_placeholder(line))
        .map(|line| line.chars().take(80).collect())
}

pub(crate) fn extract_antigravity_user_request(raw: &str) -> String {
    let without_bom = raw.strip_prefix('\u{feff}').unwrap_or(raw);
    let trimmed_start = without_bom.trim_start();
    if is_bare_wrapper_placeholder(trimmed_start.trim_end()) {
        return String::new();
    }
    let candidate = if trimmed_start.starts_with("<USER_REQUEST>") {
        trimmed_start
    } else {
        let (after_envelopes, stripped_any) = strip_leading_system_envelopes(without_bom);
        if after_envelopes.is_empty() && stripped_any {
            return String::new();
        }
        after_envelopes
    };

    if let Some(after_open) = candidate.strip_prefix("<USER_REQUEST>") {
        let body = strip_single_leading_newline(after_open);
        for (end, _) in body.rmatch_indices("</USER_REQUEST>") {
            let preceded_by_backtick = end > 0 && body.as_bytes()[end - 1] == b'`';
            let after_close = &body[end + "</USER_REQUEST>".len()..];
            if !preceded_by_backtick && is_antigravity_trailing_suffix(after_close) {
                return strip_single_trailing_newline(&body[..end]).to_owned();
            }
        }
        let without_meta = strip_trailing_antigravity_metadata(body);
        return strip_unclosed_truncated_tail(without_meta).to_owned();
    }

    let without_meta = strip_trailing_antigravity_metadata(candidate);
    if starts_with_truncated_marker(without_meta.trim())
        || is_bare_wrapper_placeholder(without_meta.trim())
    {
        return String::new();
    }
    without_meta.to_owned()
}

fn unwrap_system_message_blocks(raw: &str) -> Option<String> {
    let mut cursor = raw.trim_start_matches('\u{feff}').trim_start();
    let mut blocks = Vec::new();
    loop {
        let inner_start = if let Some(after_preamble) = cursor.strip_prefix(SYSTEM_MESSAGE_PREAMBLE)
        {
            let open_idx = after_preamble.find("<SYSTEM_MESSAGE>")?;
            &after_preamble[open_idx + "<SYSTEM_MESSAGE>".len()..]
        } else if let Some(after_open) = cursor.strip_prefix("<SYSTEM_MESSAGE>") {
            after_open
        } else {
            break;
        };
        if let Some(close_idx) = inner_start.find("</SYSTEM_MESSAGE>") {
            blocks.push(strip_single_trailing_newline(strip_single_leading_newline(
                &inner_start[..close_idx],
            )));
            cursor = inner_start[close_idx + "</SYSTEM_MESSAGE>".len()..].trim_start();
            if cursor.is_empty() {
                break;
            }
        } else {
            blocks.push(strip_unclosed_truncated_tail(strip_single_leading_newline(
                inner_start,
            )));
            cursor = "";
            break;
        }
    }
    if blocks.is_empty() {
        return None;
    }
    if !cursor.is_empty() {
        return Some(cursor.to_owned());
    }
    blocks
        .iter()
        .rev()
        .find(|b| {
            let t = b.trim_start();
            !t.is_empty()
                && !t.starts_with(
                    "The following system message was injected by a SessionStart hook:",
                )
        })
        .map(|s| (*s).to_owned())
}

fn strip_agent_message_header(s: &str) -> &str {
    let trimmed = s.trim_start();
    if let Some(rest) = trimmed.strip_prefix("[Message] ") {
        let first_line_end = rest.find('\n').unwrap_or(rest.len());
        if let Some(content_idx) = rest[..first_line_end].find(" content=") {
            return &rest[content_idx + " content=".len()..];
        }
    }
    s
}

pub(crate) fn clean_antigravity_delegation(raw: &str) -> String {
    let unwrapped_sys = unwrap_system_message_blocks(raw);
    let initial = unwrapped_sys.as_deref().unwrap_or(raw);
    let without_msg_hdr = strip_agent_message_header(initial);
    let base = extract_antigravity_user_request(without_msg_hdr);
    let source = strip_agent_message_header(base.as_str());

    let task_slice = if let Some(open_idx) = find_line_start_marker(source, "<original_task>")
        .or_else(|| {
            source
                .find("content=<original_task>")
                .map(|i| i + "content=".len())
        }) {
        let after_open =
            strip_single_leading_newline(&source[open_idx + "<original_task>".len()..]);
        let mut section_cut = after_open.len();
        for marker in ["<prior_attempt>", "<instructions>"] {
            if let Some(idx) = find_line_start_marker(after_open, marker) {
                section_cut = section_cut.min(idx);
            }
        }
        let before_sections = strip_single_trailing_newline(&after_open[..section_cut]);
        let mut closed = None;
        for (close_idx, _) in before_sections.rmatch_indices("</original_task>") {
            let preceded_by_backtick =
                close_idx > 0 && before_sections.as_bytes()[close_idx - 1] == b'`';
            let after_close = &before_sections[close_idx + "</original_task>".len()..];
            if !preceded_by_backtick && after_close.trim().is_empty() {
                closed = Some(strip_single_trailing_newline(&before_sections[..close_idx]));
                break;
            }
        }
        closed
            .unwrap_or_else(|| strip_unclosed_truncated_tail(before_sections))
            .to_owned()
    } else {
        let mut cut = source.len();
        for marker in ["<prior_attempt>", "<instructions>"] {
            if let Some(idx) = find_line_start_marker(source, marker) {
                cut = cut.min(idx);
            }
        }
        strip_unclosed_truncated_tail(&source[..cut]).to_owned()
    };

    let unwrapped = extract_antigravity_user_request(&task_slice);
    let trimmed = unwrapped.trim();
    if is_bare_wrapper_placeholder(trimmed) {
        return String::new();
    }
    let without_label = trimmed
        .strip_prefix("**Task**:")
        .map(|rest| {
            let rest = rest.trim_start_matches([' ', '\t']);
            strip_single_leading_newline(rest).trim()
        })
        .unwrap_or(trimmed);
    if is_bare_wrapper_placeholder(without_label) {
        String::new()
    } else {
        without_label.to_owned()
    }
}

pub(crate) fn is_slash_meta_command_only(raw: &str) -> bool {
    let trimmed = raw.trim();
    let Some(rest) = trimmed.strip_prefix('/') else {
        return false;
    };
    let mut parts = rest.split_whitespace();
    let Some(cmd) = parts.next() else {
        return true;
    };
    let has_args = parts.next().is_some();
    if !has_args {
        return matches!(
            cmd.to_ascii_lowercase().as_str(),
            "clear"
                | "compact"
                | "help"
                | "exit"
                | "quit"
                | "reset"
                | "new"
                | "status"
                | "login"
                | "logout"
                | "config"
                | "settings"
                | "model"
                | "theme"
                | "doctor"
                | "cost"
                | "usage"
                | "history"
                | "resume"
        );
    }
    matches!(
        cmd.to_ascii_lowercase().as_str(),
        "model" | "theme" | "config" | "settings" | "resume"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::Cursor;

    #[test]
    fn extract_user_request_strips_slash_envelopes_metadata_and_preserves_literal_mentions() {
        let wrapped = concat!(
            "<USER_REQUEST>\n",
            "/boost /plan 用 $de-gpt 深度清理老项目\n",
            "  保留缩进与行内 `</USER_REQUEST>` 以及 <ADDITIONAL_METADATA> 字面量说明\n",
            "</USER_REQUEST>\n",
            "<ADDITIONAL_METADATA>\n",
            "<ORCHESTRATOR>system orchestrator prompt</ORCHESTRATOR>\n",
            "<PLAN>system plan prompt</PLAN>\n",
            "</ADDITIONAL_METADATA>\n",
            "<USER_SETTINGS_CHANGE>model=gemini-3.1-pro</USER_SETTINGS_CHANGE>\n",
            "The following is a <SYSTEM_MESSAGE> not actually sent by the user.\n",
            "<SYSTEM_MESSAGE>internal reminder</SYSTEM_MESSAGE>"
        );
        assert_eq!(
            extract_antigravity_user_request(wrapped),
            "/boost /plan 用 $de-gpt 深度清理老项目\n  保留缩进与行内 `</USER_REQUEST>` 以及 <ADDITIONAL_METADATA> 字面量说明"
        );

        let truncated = concat!(
            "<USER_REQUEST>\n",
            "排查紧凑转录截断场景\n",
            "<truncated 18432 bytes></ADDITIONAL_METADATA>"
        );
        assert_eq!(
            extract_antigravity_user_request(truncated),
            "排查紧凑转录截断场景"
        );

        let leading_settings = concat!(
            "<USER_SETTINGS_CHANGE>model=claude-opus-4.6</USER_SETTINGS_CHANGE>\n",
            "<USER_REQUEST>\n切换模型后继续执行\n</USER_REQUEST>\n",
            "<ADDITIONAL_METADATA>ts=1</ADDITIONAL_METADATA>"
        );
        assert_eq!(
            extract_antigravity_user_request(leading_settings),
            "切换模型后继续执行"
        );

        assert_eq!(
            extract_antigravity_user_request(
                "<USER_SETTINGS_CHANGE>model=claude-opus-4.6</USER_SETTINGS_CHANGE>"
            ),
            ""
        );
        assert_eq!(
            extract_antigravity_user_request(
                "<USER_REQUEST>\n</USER_REQUEST>\n<ADDITIONAL_METADATA>ts=1</ADDITIONAL_METADATA>"
            ),
            ""
        );
        assert_eq!(
            extract_antigravity_user_request(
                "<USER_REQUEST>\n同尾闭合无换行标题</USER_REQUEST>\n<ADDITIONAL_METADATA>ts=1</ADDITIONAL_METADATA>"
            ),
            "同尾闭合无换行标题"
        );
        assert_eq!(clean_antigravity_auto_title("<USER_REQUEST>"), None);
        assert_eq!(
            clean_antigravity_auto_title(
                "<USER_REQUEST>\n/boost /plan 深度清理项目\n</USER_REQUEST>"
            ),
            Some("/boost /plan 深度清理项目".into())
        );
    }

    #[test]
    fn clean_delegation_extracts_original_task_and_removes_boost_wrappers() {
        let boost_prompt = concat!(
            "<USER_REQUEST>\n",
            "<original_task>\n",
            "**Task**: 核验 SQLite .db 精确字段解码\n",
            "\n",
            "补充上下文说明（含行内 `</original_task>` 字面量）\n",
            "</original_task>\n",
            "<prior_attempt>\n",
            "Cleaned `<original_task>...</original_task>` in prompt.rs.\n",
            "</prior_attempt>\n",
            "<instructions>\nsystem worker rules mentioning </original_task>\n</instructions>\n",
            "</USER_REQUEST>\n",
            "<ADDITIONAL_METADATA>meta</ADDITIONAL_METADATA>"
        );
        assert_eq!(
            clean_antigravity_delegation(boost_prompt),
            "核验 SQLite .db 精确字段解码\n\n补充上下文说明（含行内 `</original_task>` 字面量）"
        );

        let inline_content = "timestamp=2026-11-24T00:00:00Z content=<original_task>\n<USER_REQUEST>\n**Task**: 清理重复分页循环\n</USER_REQUEST>\n</original_task>";
        assert_eq!(
            clean_antigravity_delegation(inline_content),
            "清理重复分页循环"
        );

        let system_enveloped = concat!(
            "The following is a <SYSTEM_MESSAGE> not actually sent by the user.\n\n",
            "<SYSTEM_MESSAGE>\n",
            "The following system message was injected by a SessionStart hook:\n",
            "[startup snapshot] Workspace: /workspace/demo\n",
            "</SYSTEM_MESSAGE>",
            "The following is a <SYSTEM_MESSAGE> not actually sent by the user.\n\n",
            "<SYSTEM_MESSAGE>\n",
            "[Message] timestamp=2026-11-24T00:00:00Z sender=00000000-0000-0000-0000-000000000001 priority=MESSAGE_PRIORITY_HIGH content=排查 Grok 死代码孤岛\n",
            "</SYSTEM_MESSAGE>"
        );
        assert_eq!(
            clean_antigravity_delegation(system_enveloped),
            "排查 Grok 死代码孤岛"
        );
        assert_eq!(clean_antigravity_subagent_title("<original_task>"), None);
        assert_eq!(
            clean_antigravity_subagent_title(
                "[Message] timestamp=2026-11-24T00:00:00Z sender=abc priority=HIGH content=梳理子代理图"
            ),
            Some("梳理子代理图".into())
        );
    }

    #[test]
    fn bounded_jsonl_row_drains_overlong_line_without_misparsing_tail_fragment() {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"{\"step_index\":0,\"ok\":1}\n");
        buf.extend(std::iter::repeat_n(b'x', TRANSCRIPT_LINE_LIMIT + 128));
        buf.extend_from_slice(b"{\"step_index\":99,\"tail_fragment\":true}\n");
        buf.extend_from_slice(b"{\"step_index\":2,\"ok\":2}\n");

        let mut reader = Cursor::new(buf);
        let mut indices = Vec::new();
        for_each_bounded_jsonl_row(&mut reader, |row| {
            if let Some(idx) = row["step_index"].as_i64() {
                indices.push(idx);
            }
            ControlFlow::Continue(())
        })
        .unwrap();
        assert_eq!(indices, vec![0, 2]);
        let _ = json!(indices);
    }

    #[test]
    fn slash_meta_command_filter_preserves_parameterized_prompts_and_skills() {
        assert!(is_slash_meta_command_only("/clear"));
        assert!(is_slash_meta_command_only("  /compact  "));
        assert!(is_slash_meta_command_only("/model gemini-3.1-pro"));
        assert!(!is_slash_meta_command_only("/boost /plan 深度清理项目"));
        assert!(!is_slash_meta_command_only("/de-gpt"));
        assert!(!is_slash_meta_command_only("常规对话标题"));
    }
}
