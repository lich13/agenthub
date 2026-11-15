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
    let mut lines = Vec::new();
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
        let lower = line.to_ascii_lowercase();
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
        .any(|s| lower.starts_with(s))
        {
            continue;
        }
        line = line.trim_start_matches(['#', '>', '-', '*', '+', ' ']);
        if let Some((number, rest)) = line.split_once(". ") {
            if number.chars().all(|c| c.is_ascii_digit()) {
                line = rest;
            }
        }
        let mut text = String::new();
        let mut chars = line.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '!' && chars.peek() == Some(&'[') {
                chars.next();
                for ch in chars.by_ref() {
                    if ch == ']' {
                        break;
                    }
                }
                skip_link_target(&mut chars);
            } else if c == '[' {
                for ch in chars.by_ref() {
                    if ch == ']' {
                        break;
                    }
                    text.push(ch);
                }
                skip_link_target(&mut chars);
            } else if !matches!(c, '*' | '`' | '~') && (!c.is_control() || c == '\t') {
                text.push(c);
            }
        }
        let text = text.replace("__", "");
        if !text.trim().is_empty() {
            lines.push(text.trim().to_string());
        }
    }
    let truncated_lines = lines.len() > 4;
    let text = lines.into_iter().take(4).collect::<Vec<_>>().join("\n");
    if truncated_lines || text.chars().count() > 300 {
        format!("{}…", text.chars().take(299).collect::<String>().trim_end())
    } else {
        text
    }
}

fn skip_link_target(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) {
    if chars.peek() != Some(&'(') {
        return;
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
                Some('<') | None => return,
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
                '<' => return,
                '(' => depth += 1,
                ')' if depth == 0 => break,
                ')' => depth -= 1,
                c if c.is_whitespace() => {
                    if depth != 0 {
                        return;
                    }
                    break;
                }
                _ => {}
            }
            candidate.next();
        }
        if depth != 0 {
            return;
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
                    None => return,
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
