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
    let input = agenthub_core::security::redact_output(input);
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
                if chars.peek() == Some(&'(') {
                    for ch in chars.by_ref() {
                        if ch == ')' {
                            break;
                        }
                    }
                }
            } else if c == '[' {
                for ch in chars.by_ref() {
                    if ch == ']' {
                        break;
                    }
                    text.push(ch);
                }
                if chars.peek() == Some(&'(') {
                    for ch in chars.by_ref() {
                        if ch == ')' {
                            break;
                        }
                    }
                }
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
