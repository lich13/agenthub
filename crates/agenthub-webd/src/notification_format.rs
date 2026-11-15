//! Presentation only. Provider event identities and delivery claims stay intact.
use super::ProbeBarkRequest;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct Presentation {
    pub provider: String,
    pub urgent: bool,
}

pub(super) fn request(
    provider: &str,
    kind: &str,
    title: &str,
    project: &str,
    body: &str,
    key: &str,
) -> ProbeBarkRequest {
    let status = match kind {
        "completion" | "task_complete" | "hook_stop" => "完成",
        "reply_needed" => "需要回复",
        "authorization" => "需要授权",
        _ => "失败",
    };
    let title = [title, project]
        .into_iter()
        .map(summary)
        .find(|s| !s.trim().is_empty() && !matches!(s.trim(), "未命名会话" | "未命名线程"))
        .unwrap_or_else(|| "未命名线程".into());
    let title = title
        .lines()
        .next()
        .unwrap_or("未命名线程")
        .chars()
        .take(120)
        .collect::<String>();
    let body = summary(body);
    ProbeBarkRequest {
        url: None,
        title: format!("{provider}｜{status}｜{title}"),
        body: if body.is_empty() {
            format!("{status}，请打开线程查看详情。")
        } else {
            body
        },
        dedupe_key: key.into(),
        presentation: Some(Presentation {
            provider: provider.into(),
            urgent: matches!(kind, "reply_needed" | "authorization"),
        }),
    }
}

pub(super) fn summary(input: &str) -> String {
    redacted_summary(&agenthub_core::security::redact_output(input))
}

pub(super) fn redacted_summary(input: &str) -> String {
    let mut text = String::new();
    let mut count = 0;
    let mut lines = 0;
    let mut fenced = false;
    let mut hidden = false;
    for line in input.lines() {
        let indented = line.starts_with("    ") || line.starts_with('\t');
        let mut line = line.trim();
        if line.starts_with("```") || line.starts_with("~~~") {
            fenced = !fenced;
            continue;
        }
        if [
            "<tool_output",
            "<tool_result",
            "<system-reminder",
            "<internal",
            "<metadata",
        ]
        .iter()
        .any(|tag| line.starts_with(tag))
        {
            hidden = true;
        }
        if hidden {
            if line.contains("</") {
                hidden = false;
            }
            continue;
        }
        if fenced || indented {
            continue;
        }
        if [
            "thread_id",
            "turn_id",
            "session_id",
            "prompt_id",
            "tool_output",
            "tool_result",
            "线程 id",
            "回合 id",
            "[redacted",
            "{\"type\"",
            "{\"tool",
        ]
        .iter()
        .any(|s| {
            line.get(..s.len())
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case(s))
        }) {
            continue;
        }
        line = line.trim_start_matches(['#', '>', '-', '*', '+', ' ']);
        let number_end = line
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(line.len());
        if let Some(rest) = line[number_end..].strip_prefix(". ") {
            line = rest;
        }
        let separator = usize::from(lines > 0);
        // One extra visible character proves truncation. Trailing whitespace
        // and hidden lines do not consume the existing summary budget.
        let (plain, truncated) = plain_line(
            line,
            if lines == 4 {
                1
            } else {
                (301usize - count - separator).max(1)
            },
        );
        if plain.is_empty() {
            continue;
        }
        if lines == 4 {
            return truncated_summary(&text);
        }
        if separator > 0 {
            text.push('\n');
        }
        count += separator + plain.chars().count();
        text.push_str(&plain);
        if truncated || count > 300 {
            return truncated_summary(&text);
        }
        lines += 1;
    }
    text
}

fn truncated_summary(text: &str) -> String {
    format!("{}…", text.chars().take(299).collect::<String>().trim_end())
}

fn plain_line(line: &str, limit: usize) -> (String, bool) {
    let mut text = String::new();
    let mut count = 0;
    let mut append = |c: char| {
        if count == 0 && c.is_whitespace() {
            return true;
        }
        count += 1;
        if count <= limit {
            text.push(c);
        }
        c.is_whitespace() || count < limit
    };
    let mut underscore = false;
    let complete = {
        let mut clean = |c: char| {
            if matches!(c, '*' | '`' | '~') || (c.is_control() && c != '\t') {
                return true;
            }
            if c == '_' {
                underscore = !underscore;
                return true;
            }
            if underscore {
                underscore = false;
                if !append('_') {
                    return false;
                }
            }
            append(c)
        };
        inline_text(line, &mut clean)
    };
    let complete = complete && (!underscore || append('_'));
    (text.trim_end().to_owned(), !complete)
}

fn inline_text(line: &str, emit: &mut impl FnMut(char) -> bool) -> bool {
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '!' && chars.peek() == Some(&'[') {
            let mut candidate = chars.clone();
            candidate.next();
            if candidate.by_ref().any(|c| c == ']') && skip_link_target(&mut candidate) {
                chars = candidate;
                continue;
            }
            // A macro, array expression or incomplete image remains ordinary text.
            if !emit('!') || !emit(chars.next().expect("opening bracket")) {
                return false;
            }
            for ch in chars.by_ref() {
                if !emit(ch) {
                    return false;
                }
                if ch == ']' {
                    break;
                }
            }
        } else if c == '[' {
            for ch in chars.by_ref() {
                if ch == ']' {
                    break;
                }
                if !emit(ch) {
                    return false;
                }
            }
            skip_link_target(&mut chars);
        } else if !emit(c) {
            return false;
        }
    }
    true
}

fn skip_link_target(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> bool {
    if chars.peek() != Some(&'(') {
        return false;
    }
    let mut candidate = chars.clone();
    candidate.next();
    while candidate.peek().is_some_and(|c| c.is_whitespace()) {
        candidate.next();
    }
    if candidate.peek() == Some(&'<') {
        candidate.next();
        loop {
            match candidate.next() {
                Some('>') => break,
                Some('\\') => {
                    candidate.next();
                }
                Some('<') | None => return false,
                _ => {}
            }
        }
    } else {
        let mut depth = 0;
        while let Some(&ch) = candidate.peek() {
            match ch {
                '\\' => {
                    candidate.next();
                    candidate.next();
                    continue;
                }
                '<' => return false,
                '(' => depth += 1,
                ')' if depth == 0 => break,
                ')' => depth -= 1,
                c if c.is_whitespace() => {
                    if depth != 0 {
                        return false;
                    }
                    break;
                }
                _ => {}
            }
            candidate.next();
        }
        if depth != 0 {
            return false;
        }
    }
    let separated = candidate.peek().is_some_and(|c| c.is_whitespace());
    while candidate.peek().is_some_and(|c| c.is_whitespace()) {
        candidate.next();
    }
    if separated {
        if let Some(&quote @ ('"' | '\'' | '(')) = candidate.peek() {
            candidate.next();
            let end = if quote == '(' { ')' } else { quote };
            loop {
                match candidate.next() {
                    Some('\\') => {
                        candidate.next();
                    }
                    Some(ch) if ch == end => break,
                    None => return false,
                    _ => {}
                }
            }
            while candidate.peek().is_some_and(|c| c.is_whitespace()) {
                candidate.next();
            }
        }
    }
    if candidate.next() == Some(')') {
        *chars = candidate;
        true
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn summary_preserves_unicode_and_omits_code_metadata_and_tool_output() {
        let text = summary("# **完成**\n[项目](https://example.invalid) `ready`\n```rust\nsecret code\n```\n<tool_output>\ninternal output\n</tool_output>\nthread_id: fixture\n- 下一步 🧑‍💻\n第四行\n第五行");
        assert_eq!(text, "完成\n项目 ready\n下一步 🧑‍💻\n第四行…");
        let long = summary(&"中🙂".repeat(200));
        assert_eq!(long.chars().count(), 300);
        assert!(long.ends_with('…'));
        let r = request(
            "Claude Code",
            "authorization",
            "",
            "Fixture project",
            "**Allow?**",
            "unchanged",
        );
        assert_eq!(r.title, "Claude Code｜需要授权｜Fixture project");
        assert!(r.presentation.unwrap().urgent);
        assert_eq!(r.dedupe_key, "unchanged");
        assert_eq!(
            request(
                "Codex",
                "completion",
                "未命名线程",
                "Fixture project",
                "done",
                "key"
            )
            .title,
            "Codex｜完成｜Fixture project"
        );
        assert_eq!(
            request("Codex", "completion", "", "", "done", "key").title,
            "Codex｜完成｜未命名线程"
        );
        assert_eq!(
            request(
                "Grok",
                "completion",
                "```\ninternal\n```",
                "Fixture project",
                "done",
                "key"
            )
            .title,
            "Grok｜完成｜Fixture project"
        );
    }
}

#[cfg(test)]
#[test]
fn summary_strips_balanced_and_angle_wrapped_link_targets() {
    assert_eq!(
        summary("[报告](</fixture/报告(终版).pdf>) 已完成"),
        "报告 已完成"
    );
    assert_eq!(
        summary("[报告](https://example.invalid/a(b(c)).pdf) 已完成"),
        "报告 已完成"
    );
    assert_eq!(
        summary("![图](</fixture/image(1).png>) [保留](https://example.invalid/a\\)b)"),
        "保留"
    );
}

#[test]
fn summary_preserves_nonlinks_and_incomplete_targets() {
    assert_eq!(
        summary("结果见 [表1](误差 <0.01) 部署完成"),
        "结果见 表1(误差 <0.01) 部署完成"
    );
    assert_eq!(summary("状态 [0](x <1"), "状态 0(x <1");
    assert_eq!(summary("[表1](误差 0.01) 完成"), "表1(误差 0.01) 完成");
    assert_eq!(
        summary("[报告](https://example.invalid/a \"报告标题\") 完成"),
        "报告 完成"
    );
    assert_eq!(
        summary("[报告](https://example.invalid/a(b) 未完成"),
        "报告(https://example.invalid/a(b) 未完成"
    );
}

#[test]
fn summary_preserves_nonimage_expressions_and_cleans_link_labels() {
    assert_eq!(
        summary("vec![1, 2, 3] ![unfinished ![alt](target"),
        "vec![1, 2, 3] ![unfinished ![alt](target"
    );
    assert_eq!(
        summary("![not an image](x <1) tail"),
        "![not an image](x <1) tail"
    );
    assert_eq!(
        summary(
            "![image](</fixture/image(1).png>) [**清理** `标题` __完成__](https://example.invalid)"
        ),
        "清理 标题 完成"
    );
    assert_eq!(
        redacted_summary("[a\u{7}**b**](https://example.invalid)"),
        "ab"
    );
}

#[test]
fn summary_budget_preserves_exact_boundaries_and_ignored_tails() {
    let exact = "中".repeat(300);
    assert_eq!(redacted_summary(&exact), exact);
    assert_eq!(redacted_summary(&format!("{exact} **__**   ")), exact);
    assert_eq!(
        redacted_summary(&format!("{exact}\n```\nhidden\n```")),
        exact
    );
    assert_eq!(
        redacted_summary(&format!("{exact}\nextra")),
        format!("{}…", "中".repeat(299))
    );
    assert_eq!(redacted_summary(&format!("a{}b", " ".repeat(1000))), "a…");
    assert_eq!(
        redacted_summary(&format!(
            "{}\n__**__\n![image](target)\n```\nhidden\n```",
            "a\nb\nc\nd"
        )),
        "a\nb\nc\nd"
    );
    assert_eq!(redacted_summary("a\nb\nc\nd\n__**extra**__"), "a\nb\nc\nd…");
    assert_eq!(
        redacted_summary(&format!("{}__**__\n", "🧑‍💻".repeat(100))),
        "🧑‍💻".repeat(100)
    );
}
