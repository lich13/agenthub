use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{claude, codex, grok, platform::PlatformPaths, services::sessions::SessionProvider};

const MAX_QUERY_BYTES: usize = 512;
const MAX_RESULTS: usize = 200;
const MAX_SESSIONS: usize = 200;
const MAX_CLAUDE_PAGES: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SearchScope {
    Thread,
    Provider,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionSearchRequest {
    pub provider: SessionProvider,
    pub scope: SearchScope,
    pub session_key: Option<String>,
    pub query: String,
    pub cursor: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SessionSearchResult {
    pub result_id: String,
    pub session_key: String,
    pub native_id: String,
    pub title: String,
    pub cwd: Option<String>,
    pub match_kind: String,
    pub position_key: String,
    pub snippet: String,
    pub timestamp: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSearchResponse {
    pub provider: SessionProvider,
    pub scope: SearchScope,
    pub query: String,
    pub results: Vec<SessionSearchResult>,
    pub next_cursor: Option<String>,
    pub truncated: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct SearchUseCases {
    codex: codex::CodexPaths,
    grok: grok::GrokPaths,
    claude: claude::ClaudePaths,
    antigravity: crate::antigravity::sessions::AntigravityPaths,
    dsh: crate::dsh::DshPaths,
    grokbot: Option<crate::grokbot::GrokBotPaths>,
}

impl SearchUseCases {
    pub fn new(platform: &PlatformPaths, codex: codex::CodexPaths) -> Result<Self> {
        Ok(Self {
            codex,
            grok: grok::GrokPaths::default_for_user()?,
            claude: claude::ClaudePaths::default_for_user()?,
            antigravity: crate::antigravity::sessions::AntigravityPaths::default_for_user()?,
            dsh: crate::dsh::DshPaths::default_for_user()?,
            grokbot: (platform.kind == crate::platform::PlatformKind::Macos)
                .then(crate::grokbot::GrokBotPaths::default_for_user)
                .transpose()?,
        })
    }

    #[cfg(test)]
    pub(crate) fn with_grokbot_for_test(
        root: &std::path::Path,
        grokbot: crate::grokbot::GrokBotPaths,
    ) -> Self {
        Self {
            codex: codex::CodexPaths::new(root.join("codex")),
            grok: grok::GrokPaths {
                home: root.join("grok"),
            },
            claude: claude::ClaudePaths {
                config: root.join("claude"),
                projects: root.join("claude/projects"),
            },
            antigravity: crate::antigravity::sessions::AntigravityPaths::offline(root.join("ag")),
            dsh: crate::dsh::DshPaths {
                home: root.join("dsh"),
            },
            grokbot: Some(grokbot),
        }
    }

    pub fn search(&self, request: SessionSearchRequest) -> Result<SessionSearchResponse> {
        validate_request(&request)?;
        let needle = request.query.trim().to_lowercase();
        let mut matches = Vec::new();
        let mut warnings = Vec::new();
        match request.provider {
            SessionProvider::Grokbot => {
                self.search_grokbot(&request, &needle, &mut matches, &mut warnings)?
            }
            SessionProvider::Codex => {
                self.search_codex(&request, &needle, &mut matches, &mut warnings)?
            }
            SessionProvider::Claude => {
                self.search_claude(&request, &needle, &mut matches, &mut warnings)?
            }
            SessionProvider::Grok => {
                self.search_grok(&request, &needle, &mut matches, &mut warnings)?
            }
            SessionProvider::Antigravity => {
                self.search_antigravity(&request, &needle, &mut matches, &mut warnings)?
            }
            SessionProvider::Dsh => {
                self.search_dsh(&request, &needle, &mut matches, &mut warnings)?
            }
        }
        let offset = request
            .cursor
            .as_deref()
            .unwrap_or("0")
            .parse::<usize>()
            .context("无效的搜索游标")?;
        ensure!(offset <= matches.len(), "搜索游标已失效，请重新搜索");
        let limit = request.limit.unwrap_or(50).clamp(1, MAX_RESULTS);
        let end = (offset + limit).min(matches.len());
        let next_cursor = (end < matches.len()).then(|| end.to_string());
        Ok(SessionSearchResponse {
            provider: request.provider,
            scope: request.scope,
            query: request.query.trim().to_string(),
            results: matches[offset..end].to_vec(),
            next_cursor,
            truncated: matches.len() >= MAX_RESULTS,
            warnings,
        })
    }

    fn search_codex(
        &self,
        request: &SessionSearchRequest,
        needle: &str,
        output: &mut Vec<SessionSearchResult>,
        warnings: &mut Vec<String>,
    ) -> Result<()> {
        let summaries = if request.scope == SearchScope::Thread {
            codex::local_thread_summary(&self.codex, request.session_key.as_deref().unwrap())?
                .into_iter()
                .collect()
        } else {
            codex::list_threads_without_storage(
                &self.codex,
                false,
                request.session_key.as_deref(),
                MAX_SESSIONS,
            )?
        };
        for summary in summaries {
            if output.len() >= MAX_RESULTS {
                break;
            }
            let metadata = strip_memory_metadata(&format!(
                "{} {} {}",
                summary.title,
                summary.id,
                summary.cwd.as_deref().unwrap_or_default()
            ));
            if contains_folded(&metadata, needle) {
                push_metadata(
                    output,
                    &summary.id,
                    &summary.id,
                    &summary.title,
                    summary.cwd.clone(),
                    &metadata,
                    needle,
                    summary.updated_at.clone(),
                );
            }
            if output.len() >= MAX_RESULTS {
                break;
            }
            if codex::visit_search_blocks(&summary, |block| {
                if output.len() >= MAX_RESULTS {
                    return false;
                }
                let corpus = codex_block_corpus(block);
                if let Some(snippet) = find_snippet(&corpus, needle) {
                    push_result(
                        output,
                        &summary.id,
                        &summary.id,
                        &summary.title,
                        summary.cwd.clone(),
                        &block.kind,
                        block.id.clone(),
                        snippet,
                        block.created_at.clone(),
                    );
                }
                output.len() < MAX_RESULTS
            })
            .is_err()
            {
                push_warning(warnings);
            }
        }
        Ok(())
    }

    fn search_grok(
        &self,
        request: &SessionSearchRequest,
        needle: &str,
        output: &mut Vec<SessionSearchResult>,
        warnings: &mut Vec<String>,
    ) -> Result<()> {
        let summaries = if request.scope == SearchScope::Thread {
            vec![grok::grok_session_summary(
                &self.grok,
                request.session_key.as_deref().unwrap(),
            )?]
        } else {
            grok::list_grok_sessions_without_storage(&self.grok, MAX_SESSIONS, None)?
        };
        for summary in summaries {
            if output.len() >= MAX_RESULTS {
                break;
            }
            let metadata =
                strip_memory_metadata(&format!("{} {} {}", summary.title, summary.id, summary.cwd));
            if contains_folded(&metadata, needle) {
                push_metadata(
                    output,
                    &summary.id,
                    &summary.id,
                    &summary.title,
                    Some(summary.cwd.clone()),
                    &metadata,
                    needle,
                    summary.updated_at.clone(),
                );
            }
            if output.len() >= MAX_RESULTS {
                break;
            }
            if grok::visit_search_events(&summary.path, |index, event| {
                if output.len() >= MAX_RESULTS {
                    return false;
                }
                let corpus = grok_event_corpus(event);
                if let Some(snippet) = find_snippet(&corpus, needle) {
                    let position = event.call_id.clone().unwrap_or_else(|| {
                        if event.id.is_empty() {
                            format!("event:{index}")
                        } else {
                            event.id.clone()
                        }
                    });
                    push_result(
                        output,
                        &summary.id,
                        &summary.id,
                        &summary.title,
                        Some(summary.cwd.clone()),
                        &event.kind,
                        position,
                        snippet,
                        event.timestamp.clone(),
                    );
                }
                output.len() < MAX_RESULTS
            })
            .is_err()
            {
                push_warning(warnings);
            }
        }
        Ok(())
    }

    fn search_claude(
        &self,
        request: &SessionSearchRequest,
        needle: &str,
        output: &mut Vec<SessionSearchResult>,
        warnings: &mut Vec<String>,
    ) -> Result<()> {
        let summaries = if request.scope == SearchScope::Thread {
            vec![claude::claude_session_summary_without_storage(
                &self.claude,
                request.session_key.as_deref().unwrap(),
            )?]
        } else {
            claude::list_claude_sessions_without_storage(&self.claude, MAX_SESSIONS, None)?
        };
        for summary in summaries {
            if output.len() >= MAX_RESULTS {
                break;
            }
            let metadata =
                strip_memory_metadata(&format!("{} {} {}", summary.title, summary.id, summary.cwd));
            if contains_folded(&metadata, needle) {
                push_metadata(
                    output,
                    &summary.session_key,
                    &summary.id,
                    &summary.title,
                    Some(summary.cwd.clone()),
                    &metadata,
                    needle,
                    summary.updated_at.clone(),
                );
            }
            let read = claude::visit_search_events(
                &self.claude,
                &summary.session_key,
                MAX_CLAUDE_PAGES,
                |event| {
                    if output.len() >= MAX_RESULTS {
                        return Ok(false);
                    }
                    let corpus = claude_event_corpus(event);
                    if let Some(snippet) = find_snippet(&corpus, needle) {
                        push_result(
                            output,
                            &summary.session_key,
                            &summary.id,
                            &summary.title,
                            Some(summary.cwd.clone()),
                            &event.kind,
                            event.id.clone(),
                            snippet,
                            event.timestamp.clone(),
                        );
                    }
                    Ok(output.len() < MAX_RESULTS)
                },
            );
            if read.is_err() {
                push_warning(warnings);
            }
        }
        Ok(())
    }

    fn search_antigravity(
        &self,
        request: &SessionSearchRequest,
        needle: &str,
        output: &mut Vec<SessionSearchResult>,
        warnings: &mut Vec<String>,
    ) -> Result<()> {
        let summaries = if request.scope == SearchScope::Thread {
            vec![
                crate::antigravity::sessions::session_summary_without_storage(
                    &self.antigravity,
                    request.session_key.as_deref().unwrap(),
                )?,
            ]
        } else {
            crate::antigravity::sessions::list_sessions_without_storage(
                &self.antigravity,
                &crate::provider_sessions::ProviderListRequest {
                    limit: Some(MAX_SESSIONS),
                    ..Default::default()
                },
            )?
        };
        self.search_provider_events(
            request,
            needle,
            summaries,
            |key| crate::antigravity::sessions::load_all_events(&self.antigravity, key),
            true,
            output,
            warnings,
        )
    }

    fn search_dsh(
        &self,
        request: &SessionSearchRequest,
        needle: &str,
        output: &mut Vec<SessionSearchResult>,
        warnings: &mut Vec<String>,
    ) -> Result<()> {
        let summaries = if request.scope == SearchScope::Thread {
            vec![crate::dsh::sessions::session_summary_without_storage(
                &self.dsh,
                request.session_key.as_deref().unwrap(),
            )?]
        } else {
            crate::dsh::sessions::list_sessions_without_storage(
                &self.dsh,
                &crate::provider_sessions::ProviderListRequest {
                    limit: Some(MAX_SESSIONS),
                    ..Default::default()
                },
            )?
        };
        self.search_provider_events(
            request,
            needle,
            summaries,
            |key| crate::dsh::sessions::load_all_events(&self.dsh, key),
            false,
            output,
            warnings,
        )
    }

    fn search_grokbot(
        &self,
        request: &SessionSearchRequest,
        needle: &str,
        output: &mut Vec<SessionSearchResult>,
        warnings: &mut Vec<String>,
    ) -> Result<()> {
        let paths = self
            .grokbot
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Grok Bot 仅限本机"))?;
        let thread_key =
            (request.scope == SearchScope::Thread).then(|| request.session_key.as_deref().unwrap());
        for _ in 0..crate::grokbot::GROKBOT_VERIFY_ATTEMPTS {
            let (summaries, mut search_roster) =
                crate::grokbot::prepare_search(paths, thread_key, MAX_SESSIONS)?;
            let mut attempt_matches = Vec::new();
            let mut attempt_warnings = Vec::new();
            match self.search_provider_events(
                request,
                needle,
                summaries,
                |key| crate::grokbot::load_all_events_for_search(paths, &mut search_roster, key),
                false,
                &mut attempt_matches,
                &mut attempt_warnings,
            ) {
                Ok(()) => {}
                Err(err) if err.is::<crate::grokbot::GrokBotAccountChanged>() => continue,
                Err(err) => return Err(err),
            }
            if crate::grokbot::verify_search_scope_unchanged(paths, &search_roster)? {
                *output = attempt_matches;
                *warnings = attempt_warnings;
                return Ok(());
            }
        }
        anyhow::bail!("Grok Bot 当前账户已切换，请重新搜索")
    }

    #[allow(clippy::too_many_arguments)]
    fn search_provider_events(
        &self,
        _request: &SessionSearchRequest,
        needle: &str,
        summaries: Vec<crate::provider_sessions::ProviderSessionSummary>,
        mut load: impl FnMut(
            &str,
        ) -> Result<
            std::sync::Arc<Vec<crate::provider_sessions::ProviderHistoryEvent>>,
        >,
        literal_user: bool,
        output: &mut Vec<SessionSearchResult>,
        warnings: &mut Vec<String>,
    ) -> Result<()> {
        for summary in summaries {
            if output.len() >= MAX_RESULTS {
                break;
            }
            let metadata =
                strip_memory_metadata(&format!("{} {} {}", summary.title, summary.id, summary.cwd));
            if contains_folded(&metadata, needle) {
                push_metadata(
                    output,
                    &summary.session_key,
                    &summary.id,
                    &summary.title,
                    (!summary.cwd.is_empty()).then(|| summary.cwd.clone()),
                    &metadata,
                    needle,
                    summary.updated_at.clone(),
                );
            }
            if output.len() >= MAX_RESULTS {
                break;
            }
            let events = match load(&summary.session_key) {
                Ok(events) => events,
                Err(err) if err.is::<crate::grokbot::GrokBotAccountChanged>() => {
                    return Err(err);
                }
                Err(_) => {
                    push_warning(warnings);
                    continue;
                }
            };
            for event in events.iter() {
                if output.len() >= MAX_RESULTS {
                    break;
                }
                let corpus = provider_event_corpus(event, literal_user);
                if let Some(snippet) = find_snippet(&corpus, needle) {
                    push_result(
                        output,
                        &summary.session_key,
                        &summary.id,
                        &summary.title,
                        (!summary.cwd.is_empty()).then(|| summary.cwd.clone()),
                        &event.kind,
                        event.id.clone(),
                        snippet,
                        event.timestamp.clone(),
                    );
                }
            }
        }
        Ok(())
    }
}

fn validate_request(request: &SessionSearchRequest) -> Result<()> {
    ensure!(!request.query.trim().is_empty(), "搜索内容不能为空");
    ensure!(request.query.len() <= MAX_QUERY_BYTES, "搜索内容过长");
    if request.scope == SearchScope::Thread {
        let key = request
            .session_key
            .as_deref()
            .context("当前线程搜索需要线程定位键")?;
        ensure!(
            !key.is_empty() && key.len() <= 512 && !key.chars().any(char::is_control),
            "无效的线程定位键"
        );
    }
    if let Some(key) = request.session_key.as_deref() {
        ensure!(
            key.len() <= 512 && !key.chars().any(char::is_control),
            "无效的线程定位键"
        );
    }
    Ok(())
}

fn provider_event_corpus(
    event: &crate::provider_sessions::ProviderHistoryEvent,
    literal_user: bool,
) -> String {
    if matches!(event.kind.as_str(), "compaction" | "branch_summary") {
        let text = event
            .detail
            .as_deref()
            .or(event.text.as_deref())
            .unwrap_or_default();
        return strip_memory_metadata(text);
    }
    let mut buf = String::new();
    if let Some(user) = &event.user_message {
        let text = if literal_user {
            crate::user_message::literal_searchable_text(&user.text)
        } else {
            crate::user_message::searchable_text(&user.text)
        };
        buf.push_str(&text);
        for block in &user.context_blocks {
            if block.kind == crate::user_message::UserContextKind::Instructions {
                push_non_empty(&mut buf, block.text.as_str());
            }
        }
        for attachment in &user.attachments {
            push_non_empty(&mut buf, attachment.name.as_str());
        }
    } else if event.role.as_deref() == Some("user")
        || matches!(event.kind.as_str(), "user_message" | "user_message_chunk")
    {
        let raw = event.text.as_deref().unwrap_or("");
        buf.push_str(&if literal_user {
            crate::user_message::literal_searchable_text(raw)
        } else {
            crate::user_message::searchable_text(raw)
        });
    } else {
        if matches!(event.kind.as_str(), "tool_call" | "tool_result") && event.role != event.text {
            push_non_empty(&mut buf, event.role.as_deref());
        }
        push_non_empty(&mut buf, event.text.as_deref());
        push_non_empty(&mut buf, event.detail.as_deref());
        push_non_empty(&mut buf, event.result.as_deref());
        if let Some(sub) = &event.subagent {
            push_subagent_corpus(&mut buf, sub);
        }
    }
    strip_memory_metadata(&buf)
}

fn push_non_empty<'a>(buf: &mut String, s: impl Into<Option<&'a str>>) {
    if let Some(text) = s.into() {
        if !text.trim().is_empty() {
            if !buf.is_empty() {
                buf.push('\n');
            }
            buf.push_str(text);
        }
    }
}

fn push_user_message_corpus(buf: &mut String, user: &crate::user_message::UserMessage) {
    let text = crate::user_message::searchable_text(&user.text);
    buf.push_str(&text);
    for block in &user.context_blocks {
        if block.kind == crate::user_message::UserContextKind::Instructions {
            if !buf.is_empty() {
                buf.push('\n');
            }
            buf.push_str(&block.text);
        }
    }
    for attachment in &user.attachments {
        if !buf.is_empty() {
            buf.push('\n');
        }
        buf.push_str(&attachment.name);
    }
}

fn push_subagent_corpus(buf: &mut String, sub: &codex::subagents::SubagentActivity) {
    push_non_empty(buf, sub.name.as_str());
    push_non_empty(buf, sub.role.as_deref());
    push_non_empty(buf, sub.unavailable_reason.as_deref());
    push_non_empty(buf, sub.delegation.as_deref());
}

fn codex_block_corpus(block: &codex::MessageBlock) -> String {
    if matches!(block.kind.as_str(), "compaction" | "branch_summary") {
        return strip_memory_metadata(block.text.as_deref().unwrap_or_default());
    }
    let mut buf = String::new();
    if let Some(user) = &block.user_message {
        push_user_message_corpus(&mut buf, user);
    } else if block.role == "user"
        || matches!(block.kind.as_str(), "user_message" | "user_message_chunk")
    {
        buf.push_str(&crate::user_message::searchable_text(
            block.text.as_deref().unwrap_or(""),
        ));
    } else {
        if let Some(sub) = &block.subagent {
            push_subagent_corpus(&mut buf, sub);
        }
        push_non_empty(&mut buf, block.tool_name.as_deref());
        push_non_empty(&mut buf, block.text.as_deref());
        push_non_empty(&mut buf, block.summary.as_deref());
        push_non_empty(&mut buf, block.input.as_deref());
        for ans in &block.answers {
            for a in &ans.answers {
                push_non_empty(&mut buf, a.as_str());
            }
            push_non_empty(&mut buf, ans.note.as_deref());
        }
        for q in &block.questions {
            push_non_empty(&mut buf, q.header.as_deref());
            push_non_empty(&mut buf, q.question.as_str());
            for opt in &q.options {
                push_non_empty(&mut buf, opt.label.as_str());
                push_non_empty(&mut buf, opt.description.as_deref());
            }
        }
    }
    strip_memory_metadata(&buf)
}

fn claude_event_corpus(event: &claude::ClaudeHistoryEvent) -> String {
    if matches!(event.kind.as_str(), "compaction" | "branch_summary") {
        let text = event
            .detail
            .as_deref()
            .or(event.text.as_deref())
            .unwrap_or_default();
        return strip_memory_metadata(text);
    }
    let mut buf = String::new();
    if let Some(user) = &event.user_message {
        push_user_message_corpus(&mut buf, user);
    } else if event.role.as_deref() == Some("user")
        || matches!(event.kind.as_str(), "user_message" | "user_message_chunk")
    {
        buf.push_str(&crate::user_message::searchable_text(
            event.text.as_deref().unwrap_or(""),
        ));
    } else {
        if matches!(
            event.kind.as_str(),
            "tool_call" | "tool_result" | "tool_reference"
        ) && event.role != event.text
        {
            push_non_empty(&mut buf, event.role.as_deref());
        }
        push_non_empty(&mut buf, event.text.as_deref());
        push_non_empty(&mut buf, event.detail.as_deref());
        push_non_empty(&mut buf, event.result.as_deref());
    }
    strip_memory_metadata(&buf)
}

fn grok_event_corpus(event: &grok::GrokHistoryEvent) -> String {
    if matches!(event.kind.as_str(), "compaction" | "branch_summary") {
        let text = event
            .detail
            .as_deref()
            .or(event.text.as_deref())
            .unwrap_or_default();
        return strip_memory_metadata(text);
    }
    let mut buf = String::new();
    if let Some(user) = &event.user_message {
        push_user_message_corpus(&mut buf, user);
    } else if matches!(event.kind.as_str(), "user_message" | "user_message_chunk") {
        buf.push_str(&crate::user_message::searchable_text(
            event.text.as_deref().unwrap_or(""),
        ));
    } else {
        push_non_empty(&mut buf, event.text.as_deref());
        if event.kind.starts_with("tool_") && event.method.as_deref() != Some("session/update") {
            push_non_empty(&mut buf, event.method.as_deref());
        }
        push_non_empty(&mut buf, event.detail.as_deref());
    }
    strip_memory_metadata(&buf)
}

fn push_warning(warnings: &mut Vec<String>) {
    if warnings.len() < 8
        && !warnings
            .iter()
            .any(|warning| warning == "部分记录无法读取，已跳过")
    {
        warnings.push("部分记录无法读取，已跳过".to_string());
    }
}

fn find_ascii_case_insensitive_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    if haystack.len() < needle.len() {
        return None;
    }
    let first_lower = needle[0].to_ascii_lowercase();
    let first_upper = needle[0].to_ascii_uppercase();
    let max_start = haystack.len() - needle.len();
    for i in 0..=max_start {
        let b = haystack[i];
        if (b == first_lower || b == first_upper)
            && haystack[i + 1..i + needle.len()].eq_ignore_ascii_case(&needle[1..])
        {
            return Some(i);
        }
    }
    None
}

fn contains_ascii_case_insensitive_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    find_ascii_case_insensitive_bytes(haystack, needle).is_some()
}

/// True when `c` has no Unicode uppercase equivalents that fold into it (e.g. CJK/Hangul/Kana/digits).
fn is_caseless_non_combining_char(c: char) -> bool {
    c.is_ascii_digit()
        || ('\u{3000}'..='\u{9FFF}').contains(&c)
        || ('\u{AC00}'..='\u{D7AF}').contains(&c)
        || ('\u{FF01}'..='\u{FF20}').contains(&c)
}

fn has_ascii_folding_non_ascii(haystack: &str) -> bool {
    haystack
        .chars()
        .any(|c| matches!(c, '\u{0130}' | '\u{017F}' | '\u{212A}'))
}

pub(crate) fn contains_folded_str(haystack: &str, needle_lower: &str) -> bool {
    contains_folded(haystack, needle_lower)
}

fn contains_folded(haystack: &str, needle_lower: &str) -> bool {
    if needle_lower.is_empty() {
        return true;
    }
    if needle_lower.is_ascii() {
        if haystack.is_ascii() || !has_ascii_folding_non_ascii(haystack) {
            return contains_ascii_case_insensitive_bytes(
                haystack.as_bytes(),
                needle_lower.as_bytes(),
            );
        }
    } else if needle_lower.chars().all(is_caseless_non_combining_char) {
        return haystack.contains(needle_lower);
    }
    haystack.to_lowercase().contains(needle_lower)
}

fn might_contain_memory_metadata(bytes: &[u8]) -> bool {
    contains_ascii_case_insensitive_bytes(bytes, b"memory_citation")
        || contains_ascii_case_insensitive_bytes(bytes, b"oai-mem-citation")
        || contains_ascii_case_insensitive_bytes(bytes, b"citation_entries")
        || contains_ascii_case_insensitive_bytes(bytes, b"rollout_ids")
        || contains_ascii_case_insensitive_bytes(bytes, b"<external_codex_apps_")
}

pub(crate) fn strip_memory_metadata(input: &str) -> String {
    if !might_contain_memory_metadata(input.as_bytes()) {
        return input.to_owned();
    }
    let mut result = String::with_capacity(input.len());
    let mut fence: Option<(u8, usize)> = None;
    let mut literal_line = false;
    let mut metadata_depth: Vec<&str> = Vec::new();
    let lower = input.to_ascii_lowercase();
    let mut cursor = 0;
    while cursor < input.len() {
        let at_line_start =
            cursor == 0 || input.as_bytes().get(cursor.saturating_sub(1)) == Some(&b'\n');
        if metadata_depth.is_empty() && at_line_start {
            let line_end = input[cursor..]
                .find('\n')
                .map(|offset| cursor + offset + 1)
                .unwrap_or(input.len());
            let trimmed = input[cursor..line_end].trim_start();
            let line = &input[cursor..line_end];
            literal_line =
                trimmed.starts_with('>') || line.starts_with("    ") || line.starts_with('\t');
            let marker = trimmed.as_bytes().first().copied();
            let marker_len = trimmed
                .bytes()
                .take_while(|byte| Some(*byte) == marker)
                .count();
            let is_fence = !literal_line && matches!(marker, Some(b'`' | b'~')) && marker_len >= 3;
            if fence.is_some() || is_fence {
                result.push_str(&input[cursor..line_end]);
                if let Some((active, length)) = fence {
                    if marker == Some(active)
                        && marker_len >= length
                        && trimmed[marker_len..].trim().is_empty()
                    {
                        fence = None;
                    }
                } else {
                    fence = marker.map(|marker| (marker, marker_len));
                }
                cursor = line_end;
                continue;
            }
            let field = trimmed.to_ascii_lowercase();
            if field.starts_with("memory_citation")
                || field.starts_with("\"memory_citation\"")
                || field.starts_with("'memory_citation'")
            {
                if let Some(newline) = input[cursor..line_end].find('\n') {
                    result.push('\n');
                    cursor += newline + 1;
                } else {
                    cursor = line_end;
                }
                continue;
            }
        }
        if !metadata_depth.is_empty() {
            let token = &lower[cursor..];
            if let Some((true, name, end)) = match_metadata_tag(token) {
                if let Some(index) = metadata_depth.iter().rposition(|item| *item == name) {
                    metadata_depth.truncate(index);
                }
                cursor += end;
                continue;
            }
            if let Some((false, name, end)) = match_metadata_tag(token) {
                metadata_depth.push(name);
                cursor += end;
                continue;
            }
            cursor += input[cursor..]
                .chars()
                .next()
                .map(char::len_utf8)
                .unwrap_or(1);
            continue;
        }
        if input[cursor..].starts_with('`') {
            let run = input[cursor..].chars().take_while(|ch| *ch == '`').count();
            let delimiter = "`".repeat(run);
            let content_start = cursor + delimiter.len();
            let end = input[content_start..]
                .find(&delimiter)
                .map(|offset| content_start + offset + delimiter.len())
                .unwrap_or(input.len());
            result.push_str(&input[cursor..end]);
            cursor = end;
            continue;
        }
        if !literal_line {
            if let Some(length) = page_metadata_length(&input[cursor..]) {
                cursor += length;
                continue;
            }
        }
        if let Some((closing, name, end)) = match_metadata_tag(&lower[cursor..]) {
            if !closing {
                metadata_depth.push(name);
                cursor += end;
                continue;
            }
        }
        result.push_str(
            &input[cursor
                ..cursor
                    + input[cursor..]
                        .chars()
                        .next()
                        .map(char::len_utf8)
                        .unwrap_or(1)],
        );
        cursor += input[cursor..]
            .chars()
            .next()
            .map(char::len_utf8)
            .unwrap_or(1);
    }
    result
}

// Match only the complete native transport envelope. Examples and incomplete
// JSON stay readable; this never changes the source transcript.
fn page_metadata_length(input: &str) -> Option<usize> {
    const OPEN: &str = "<external_codex_apps_open_page>";
    const CLOSE: &str = "</external_codex_apps_open_page>";
    if let Some(body) = input.strip_prefix(OPEN) {
        let end = body.find(CLOSE)?;
        let value: Value = serde_json::from_str(&body[..end]).ok()?;
        let object = value.as_object()?;
        let page_id = object.get("page_id")?;
        return (object.len() == 1 && (page_id.is_null() || page_id.is_string()))
            .then_some(OPEN.len() + end + CLOSE.len());
    }
    selected_page_snapshot_length(input)
}

fn selected_page_snapshot_length(input: &str) -> Option<usize> {
    const TAG_PREFIX: &str = "<external_codex_apps_selected_page_part_";
    let rest = input.strip_prefix(TAG_PREFIX)?;
    let gt = rest.find('>')?;
    let tag_spec = &rest[..gt];
    let (part_str, total_str) = tag_spec.split_once("_of_")?;
    let part: u32 = part_str.parse().ok().filter(|&n| n >= 1)?;
    let total: u32 = total_str.parse().ok().filter(|&n| n >= part)?;

    let open_len = TAG_PREFIX.len() + gt + 1;
    let close_tag = format!("</external_codex_apps_selected_page_part_{part_str}_of_{total_str}>");
    let body_all = &input[open_len..];
    let close_pos = body_all.find(&close_tag)?;
    let inner = &body_all[..close_pos];
    let expected_header = format!("Companion Page snapshot part {part} of {total}:");
    let json_slice = inner.strip_prefix(&expected_header)?.trim();
    let value: Value = serde_json::from_str(json_slice).ok()?;
    let object = value.as_object()?;
    let page_id = object.get("page_id")?;
    (object.contains_key("document_view") && (page_id.is_null() || page_id.is_string()))
        .then_some(open_len + close_pos + close_tag.len())
}

fn match_metadata_tag(input: &str) -> Option<(bool, &'static str, usize)> {
    let mut cursor = if input.starts_with("&lt;") {
        4
    } else if input.starts_with('<') {
        1
    } else {
        return None;
    };
    let closing = input[cursor..].starts_with('/');
    if closing {
        cursor += 1;
    }
    let names: [(&str, &str); 3] = [
        ("oai-mem-citation", "oai-mem-citation"),
        ("citation_entries", "citation_entries"),
        ("rollout_ids", "rollout_ids"),
    ];
    let (_, name) = names
        .iter()
        .find(|(prefix, _)| input[cursor..].starts_with(prefix))?;
    cursor += name.len();
    while cursor < input.len() {
        if input[cursor..].starts_with('>') {
            return Some((closing, name, cursor + 1));
        }
        if input[cursor..].starts_with("&gt;") {
            return Some((closing, name, cursor + 4));
        }
        cursor += input[cursor..].chars().next()?.len_utf8();
    }
    None
}

fn format_snippet_window(text: &str, start: usize, end: usize) -> String {
    text.get(start..end)
        .unwrap_or(text)
        .replace('\n', " ")
        .trim()
        .to_string()
}

fn find_snippet(text: &str, needle: &str) -> Option<String> {
    const PRE_CHARS: usize = 90;
    const POST_CHARS: usize = 160;
    if needle.is_empty() {
        let end = text
            .char_indices()
            .nth(POST_CHARS)
            .map_or(text.len(), |(idx, _)| idx);
        return Some(format_snippet_window(text, 0, end));
    }

    let ascii_fast = needle.is_ascii() && (text.is_ascii() || !has_ascii_folding_non_ascii(text));
    if ascii_fast {
        let match_start = find_ascii_case_insensitive_bytes(text.as_bytes(), needle.as_bytes())?;
        let match_end = match_start + needle.len();
        let start = text[..match_start]
            .char_indices()
            .rev()
            .nth(PRE_CHARS - 1)
            .map_or(0, |(idx, _)| idx);
        let end = text[match_end..]
            .char_indices()
            .nth(POST_CHARS)
            .map_or(text.len(), |(idx, _)| match_end + idx);
        return Some(format_snippet_window(text, start, end));
    }

    if needle.chars().all(is_caseless_non_combining_char) {
        let match_start = text.find(needle)?;
        let match_end = match_start + needle.len();
        let start = text[..match_start]
            .char_indices()
            .rev()
            .nth(PRE_CHARS - 1)
            .map_or(0, |(idx, _)| idx);
        let end = text[match_end..]
            .char_indices()
            .nth(POST_CHARS)
            .map_or(text.len(), |(idx, _)| match_end + idx);
        return Some(format_snippet_window(text, start, end));
    }

    // General Unicode case-folding single-pass scan with fixed stack ring buffer (zero per-char heap allocs).
    let folded_needle: Vec<char> = needle.chars().flat_map(char::to_lowercase).collect();
    if folded_needle.is_empty() {
        return Some(format_snippet_window(text, 0, text.len()));
    }
    let needle_len = folded_needle.len();
    let lower = text.to_lowercase();
    let folded_needle_str: String = folded_needle.iter().collect();
    let byte_hit = lower.find(&folded_needle_str)?;
    let folded_start = lower[..byte_hit].chars().count();
    let folded_end = folded_start + needle_len;
    let target_start = folded_start.saturating_sub(PRE_CHARS);
    let target_end = folded_end.saturating_add(POST_CHARS);

    let mut folded_pos = 0usize;
    let mut start_byte = None;
    let mut end_byte = None;
    for (source_index, ch) in text.char_indices() {
        for _ in ch.to_lowercase() {
            if start_byte.is_none() && folded_pos >= target_start {
                start_byte = Some(source_index);
            }
            if end_byte.is_none() && folded_pos >= target_end {
                end_byte = Some(source_index);
                break;
            }
            folded_pos += 1;
        }
        if end_byte.is_some() {
            break;
        }
    }
    Some(format_snippet_window(
        text,
        start_byte.unwrap_or(0),
        end_byte.unwrap_or(text.len()),
    ))
}

#[allow(clippy::too_many_arguments)]
fn push_metadata(
    output: &mut Vec<SessionSearchResult>,
    key: &str,
    native_id: &str,
    title: &str,
    cwd: Option<String>,
    metadata: &str,
    needle: &str,
    timestamp: Option<String>,
) {
    let snippet = find_snippet(metadata, needle).unwrap_or_else(|| metadata.to_string());
    push_result(
        output,
        key,
        native_id,
        title,
        cwd,
        "metadata",
        "session".to_string(),
        snippet,
        timestamp,
    );
}

#[allow(clippy::too_many_arguments)]
fn push_result(
    output: &mut Vec<SessionSearchResult>,
    key: &str,
    native_id: &str,
    title: &str,
    cwd: Option<String>,
    kind: &str,
    position: String,
    snippet: String,
    timestamp: Option<String>,
) {
    if output.len() >= MAX_RESULTS {
        return;
    }
    let result_id = format!("{key}:{position}:{kind}");
    if output.iter().any(|item| item.result_id == result_id) {
        return;
    }
    output.push(SessionSearchResult {
        result_id,
        session_key: key.to_string(),
        native_id: native_id.to_string(),
        title: title.to_string(),
        cwd,
        match_kind: kind.to_string(),
        position_key: position,
        snippet,
        timestamp,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn isolated_service(root: &std::path::Path) -> SearchUseCases {
        SearchUseCases {
            codex: codex::CodexPaths::new(root.join("codex")),
            grok: grok::GrokPaths {
                home: root.join("grok"),
            },
            claude: claude::ClaudePaths {
                config: root.join("claude"),
                projects: root.join("claude/projects"),
            },
            antigravity: crate::antigravity::sessions::AntigravityPaths::offline(root.join("ag")),
            dsh: crate::dsh::DshPaths {
                home: root.join("dsh"),
            },
            grokbot: Some(crate::grokbot::GrokBotPaths {
                persistence: root.join("grokbot"),
            }),
        }
    }

    #[test]
    fn codex_search_and_attachments_read_history_older_than_visual_compaction() {
        use crate::user_message::SessionAttachmentRequest;
        let root =
            std::env::temp_dir().join(format!("agenthub-search-history-{}", uuid::Uuid::new_v4()));
        let service = isolated_service(&root);
        std::fs::create_dir_all(service.codex.sessions_dir()).unwrap();
        let id = "11111111-1111-4111-8111-111111111111";
        let png = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aN1cAAAAASUVORK5CYII=";
        let mut rows = vec![json!({"type":"session_meta","payload":{"id":id,"source":"cli"}})];
        for n in 0..100 {
            let mut content = vec![
                json!({"type":"input_text","text":if n==0 {"older-needle"} else {"recent fixture"}}),
            ];
            if n == 0 {
                content.push(json!({"type":"input_image","image_url":format!("data:image/png;base64,{png}")}));
            }
            rows.push(json!({"type":"response_item","payload":{"id":format!("user-{n}"),"type":"message","role":"user","content":content}}));
        }
        let text = rows.iter().map(|r| format!("{r}\n")).collect::<String>();
        let conn = rusqlite::Connection::open(service.codex.state_db()).unwrap();
        conn.execute_batch("CREATE TABLE threads(id TEXT PRIMARY KEY,title TEXT,rollout_path TEXT,source TEXT,archived INTEGER,created_at INTEGER,updated_at INTEGER);").unwrap();
        for compressed in [false, true] {
            let path = service.codex.sessions_dir().join(if compressed {
                "history.jsonl.zst"
            } else {
                "history.jsonl"
            });
            std::fs::write(
                &path,
                if compressed {
                    zstd::encode_all(text.as_bytes(), 1).unwrap()
                } else {
                    text.as_bytes().to_vec()
                },
            )
            .unwrap();
            conn.execute(
                "INSERT OR REPLACE INTO threads VALUES(?1,'Fixture',?2,'cli',0,1,1)",
                rusqlite::params![id, path.to_str().unwrap()],
            )
            .unwrap();
            let summary = codex::local_thread_summary(&service.codex, id)
                .unwrap()
                .unwrap();
            let full =
                codex::thread_detail_page_from_summary(summary.clone(), false, None, None, None)
                    .unwrap();
            let old = full
                .blocks
                .iter()
                .filter_map(|b| b.user_message.as_ref())
                .find(|m| !m.attachments.is_empty())
                .unwrap();
            let compact =
                codex::thread_detail_page_from_summary(summary, true, None, None, None).unwrap();
            assert!(!compact
                .blocks
                .iter()
                .any(|b| b.user_message.as_ref().is_some_and(|m| m.id == old.id)));
            for scope in [SearchScope::Thread, SearchScope::Provider] {
                let found = service
                    .search(SessionSearchRequest {
                        provider: SessionProvider::Codex,
                        scope,
                        session_key: Some(id.into()),
                        query: "older-needle".into(),
                        cursor: None,
                        limit: None,
                    })
                    .unwrap();
                assert_eq!(found.results.len(), 1, "compressed={compressed}");
                assert_eq!(
                    found.results[0].position_key,
                    full.blocks
                        .iter()
                        .find(|b| b.user_message.as_ref().is_some_and(|m| m.id == old.id))
                        .unwrap()
                        .id
                );
            }
            let platform = crate::platform::PlatformPaths::desktop_current().unwrap();
            let sessions = crate::services::use_cases::AgentHubUseCases::new(&platform)
                .sessions(service.codex.clone())
                .unwrap();
            let response = sessions
                .attachment_read(SessionAttachmentRequest {
                    root_thread_id: None,
                    provider: SessionProvider::Codex,
                    session_key: id.into(),
                    message_id: old.id.clone(),
                    attachment_id: old.attachments[0].id.clone(),
                })
                .unwrap();
            assert_eq!(response.mime_type, "image/png");
            assert_eq!(response.base64, png);
        }
        drop(conn);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn current_thread_search_reads_an_older_native_identity_outside_recent_list() {
        let root =
            std::env::temp_dir().join(format!("agenthub-old-search-{}", uuid::Uuid::new_v4()));
        let service = isolated_service(&root);
        let old_id = "00000000-0000-4000-8000-000000000000";
        for i in 0..201 {
            let id = format!("00000000-0000-4000-8000-{i:012}");
            let dir = service.grok.sessions().join("fixture").join(&id);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("summary.json"), json!({"info":{"id":id,"cwd":"/workspace"},"generated_title":"Fixture","num_messages":1}).to_string()).unwrap();
            std::fs::write(dir.join("chat_history.jsonl"),format!("{}\n",json!({"type":"user","content":"needle fixture","timestamp":if i==0 {"2020-01-01T00:00:00Z"} else {"2026-01-01T00:00:00Z"}}))).unwrap();
        }
        assert!(!grok::list_grok_sessions(&service.grok, 200, None)
            .unwrap()
            .iter()
            .any(|s| s.id == old_id));
        let result = service
            .search(SessionSearchRequest {
                provider: SessionProvider::Grok,
                scope: SearchScope::Thread,
                session_key: Some(old_id.into()),
                query: "needle".into(),
                cursor: None,
                limit: None,
            })
            .unwrap();
        assert_eq!(result.results.len(), 1);
        assert_eq!(result.results[0].native_id, old_id);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn search_stops_reading_history_at_result_limit_in_native_order() {
        let root = std::env::temp_dir().join("agenthub-search-fixture");
        let service = isolated_service(&root);
        let summary = |id: &str| {
            serde_json::from_value(json!({"id":id,"sessionKey":id,"title":"Fixture","cwd":"","path":"","archived":false,"updatedAt":null,"messageCount":220,"lastMessage":null,"status":"unknown","formatVersion":"fixture","canRename":false,"canArchive":false,"canDelete":false})).unwrap()
        };
        let events: Vec<crate::provider_sessions::ProviderHistoryEvent> = (0..220).map(|i|serde_json::from_value(json!({"id":format!("event-{i}"),"kind":"assistant_message","text":format!("needle {i}")})).unwrap()).collect();
        let events = std::sync::Arc::new(events);
        let reads = std::cell::Cell::new(0);
        let mut output = vec![];
        let mut warnings = vec![];
        service
            .search_provider_events(
                &SessionSearchRequest {
                    provider: SessionProvider::Grokbot,
                    scope: SearchScope::Provider,
                    session_key: None,
                    query: "needle".into(),
                    cursor: None,
                    limit: Some(200),
                },
                "needle",
                vec![summary("first"), summary("unread")],
                |id| {
                    assert_eq!(id, "first");
                    reads.set(reads.get() + 1);
                    Ok(events.clone())
                },
                false,
                &mut output,
                &mut warnings,
            )
            .unwrap();
        assert_eq!(reads.get(), 1);
        assert_eq!(output.len(), 200);
        assert_eq!(output[0].position_key, "event-0");
        assert_eq!(output[199].position_key, "event-199");
        assert!(warnings.is_empty());

        let literal_ev: crate::provider_sessions::ProviderHistoryEvent =
            serde_json::from_value(json!({
                "id": "ag-u1",
                "kind": "user_message",
                "role": "user",
                "text": "<environment_context>\n字面量检索词\n</environment_context>",
                "userMessage": {
                    "id": "ag-u1",
                    "text": "<environment_context>\n字面量检索词\n</environment_context>",
                    "attachments": []
                }
            }))
            .unwrap();
        assert!(provider_event_corpus(&literal_ev, true).contains("字面量检索词"));
        assert!(!provider_event_corpus(&literal_ev, false).contains("字面量检索词"));
    }

    #[test]
    fn grok_native_history_search_uses_visible_messages_and_tool_results() {
        let root =
            std::env::temp_dir().join(format!("agenthub-native-search-{}", uuid::Uuid::new_v4()));
        let id = uuid::Uuid::new_v4().to_string();
        let grok = grok::GrokPaths {
            home: root.join("grok"),
        };
        let directory = grok.sessions().join("workspace").join(&id);
        std::fs::create_dir_all(&directory).unwrap();
        let rows = [
            json!({"type":"system","content":"hidden needle"}),
            json!({"type":"user","content":"visible needle"}),
            json!({"type":"assistant","tool_calls":[{"id":"tool","name":"Read","arguments":"{}"}]}),
            json!({"type":"tool_result","tool_call_id":"tool","content":"result needle"}),
        ];
        std::fs::write(
            directory.join("chat_history.jsonl"),
            rows.iter()
                .map(|row| format!("{row}\n"))
                .collect::<String>(),
        )
        .unwrap();
        let service = SearchUseCases {
            grokbot: Some(crate::grokbot::GrokBotPaths {
                persistence: root.join("grokbot"),
            }),
            codex: codex::CodexPaths::new(root.join("codex")),
            claude: claude::ClaudePaths {
                config: root.join("claude"),
                projects: root.join("claude/projects"),
            },
            grok,
            antigravity: crate::antigravity::sessions::AntigravityPaths::offline(
                root.join("antigravity"),
            ),
            dsh: crate::dsh::DshPaths {
                home: root.join("dsh"),
            },
        };
        let results = service
            .search(SessionSearchRequest {
                provider: SessionProvider::Grok,
                scope: SearchScope::Thread,
                session_key: Some(id),
                query: "needle".into(),
                cursor: None,
                limit: None,
            })
            .unwrap();
        assert_eq!(results.results.len(), 2);
        assert!(results
            .results
            .iter()
            .all(|item| !item.snippet.contains("hidden")));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn removes_memory_blocks_but_keeps_code_examples() {
        let text = "before\n<oai-mem-citation>secret</oai-mem-citation>\nafter\n```\n<oai-mem-citation>example</oai-mem-citation>\n```";
        let output = strip_memory_metadata(text);
        assert!(output.contains("before"));
        assert!(output.contains("after"));
        assert!(output.contains("example"));
        assert!(!output.contains("secret"));
    }

    #[test]
    fn removes_adjacent_and_multiline_metadata_without_dropping_visible_text() {
        let text = "before <citation_entries>secret</citation_entries><rollout_ids>id</rollout_ids> after\n<oai-mem-citation>\ninternal\n</oai-mem-citation>\n`<rollout_ids>literal</rollout_ids>`\nmemory_citation: {\"private\":true}\nafter";
        let output = strip_memory_metadata(text);
        assert!(output.contains("before  after"));
        assert!(output.contains("`<rollout_ids>literal</rollout_ids>`"));
        assert!(output.contains("after"));
        assert!(!output.contains("secret"));
        assert!(!output.contains("internal"));
        assert!(!output.contains("private"));
    }

    #[test]
    fn excludes_native_page_envelopes_from_search_without_losing_adjacent_text() {
        let empty =
            "<external_codex_apps_open_page>{\"page_id\":null}</external_codex_apps_open_page>";
        let named = "<external_codex_apps_open_page>\n{\"page_id\":\"fixture-page\"}\n</external_codex_apps_open_page>";
        let output = strip_memory_metadata(&format!("Before {empty}{named} after."));
        assert_eq!(output, "Before  after.");
        assert_eq!(strip_memory_metadata(empty), "");
    }

    #[test]
    fn page_envelope_examples_and_unconfirmed_content_remain_searchable() {
        let envelope =
            "<external_codex_apps_open_page>{\"page_id\":null}</external_codex_apps_open_page>";
        for source in [
            format!("`{envelope}`"),
            format!("```xml\n{envelope}\n```"),
            format!("````xml\n```\n{envelope}\n````"),
            format!("~~~xml\n{envelope}\n~~~"),
            format!("> {envelope}"),
            format!("    {envelope}"),
            "<external_codex_apps_open_page>invalid</external_codex_apps_open_page>".into(),
            "<external_codex_apps_open_page>{\"page_id\":null}".into(),
            "<external_codex_apps_open_page>{\"request\":\"keep\"}</external_codex_apps_open_page>"
                .into(),
        ] {
            assert_eq!(strip_memory_metadata(&source), source);
        }
    }

    #[test]
    fn folded_native_compaction_remains_searchable_without_indexing_runtime_context() {
        let compact: claude::ClaudeHistoryEvent = serde_json::from_value(json!({
            "id": "compact-1",
            "kind": "compaction",
            "text": "压缩摘要",
            "detail": "Summary fixture text",
            "userMessage": {
                "id": "compact",
                "text": "",
                "attachments": [],
                "contextBlocks": [{"kind": "runtime", "text": "Summary fixture text"}]
            }
        }))
        .unwrap();
        assert_eq!(claude_event_corpus(&compact), "Summary fixture text");
        let mut automatic = compact;
        automatic.kind = "system".into();
        assert!(claude_event_corpus(&automatic).trim().is_empty());
    }

    #[test]
    fn user_message_search_accepts_both_wire_keys_and_indexes_only_searchable_content() {
        let body = "SEARCH_BODY_TOKEN";
        let question = "SEARCH_QUESTION_TOKEN";
        let answer = "SEARCH_ANSWER_TOKEN";
        let text = format!(
            "{body}\n<send_user_message_question_reply>\n[{{\"questionItemId\":\"fixture-question-id\",\"question\":\"{question}\",\"answer\":\"{answer}\"}}]\n</send_user_message_question_reply>"
        );
        let user_message = json!({
            "id": "fixture-user-message",
            "text": text,
            "contextBlocks": [
                {"kind":"instructions", "text":"AGENTS_INSTRUCTIONS_TOKEN"},
                {"kind":"environment", "text":"AUTO_ENVIRONMENT_TOKEN"},
                {"kind":"runtime", "text":"AUTO_PERMISSIONS_TOKEN"},
                {"kind":"runtime", "text":"AUTO_RUNTIME_TOKEN"},
                {"kind":"files", "text":"AUTO_FILE_CONTEXT_TOKEN"}
            ],
            "attachments": [{
                "id":"fixture-attachment",
                "name":"fixture-attachment-name.md",
                "kind":"file",
                "path":"/fixture/excluded-attachment-path",
                "reason":null
            }]
        });

        let codex_block: codex::MessageBlock = serde_json::from_value(json!({
            "id": "b-user-1",
            "role": "user",
            "kind": "user_message",
            "text": text,
            "answers": [],
            "questions": [],
            "user_message": user_message,
            "payload": {"duplicatePayload": "OUTER_ONLY_PAYLOAD_TOKEN"}
        }))
        .unwrap();
        let claude_event: claude::ClaudeHistoryEvent = serde_json::from_value(json!({
            "id": "c-user-1",
            "kind": "user_message",
            "role": "user",
            "text": text,
            "detail": "OUTER_ONLY_PAYLOAD_TOKEN",
            "userMessage": user_message
        }))
        .unwrap();
        let grok_event: grok::GrokHistoryEvent = serde_json::from_value(json!({
            "id": "g-user-1",
            "kind": "user_message",
            "text": text,
            "detail": "OUTER_ONLY_PAYLOAD_TOKEN",
            "userMessage": user_message
        }))
        .unwrap();

        for (label, corpus) in [
            ("codex", codex_block_corpus(&codex_block)),
            ("claude", claude_event_corpus(&claude_event)),
            ("grok", grok_event_corpus(&grok_event)),
        ] {
            for token in [body, question, answer] {
                assert_eq!(
                    corpus.matches(token).count(),
                    1,
                    "{token} should occur once for {label}"
                );
            }
            assert!(corpus.contains("AGENTS_INSTRUCTIONS_TOKEN"));
            assert!(corpus.contains("fixture-attachment-name.md"));
            for excluded in [
                "fixture-question-id",
                "AUTO_ENVIRONMENT_TOKEN",
                "AUTO_PERMISSIONS_TOKEN",
                "AUTO_RUNTIME_TOKEN",
                "AUTO_FILE_CONTEXT_TOKEN",
                "excluded-attachment-path",
                "OUTER_ONLY_PAYLOAD_TOKEN",
            ] {
                assert!(
                    !corpus.contains(excluded),
                    "{excluded} must not enter the searchable corpus for {label}"
                );
            }
        }
    }

    #[test]
    fn context_only_environment_user_message_has_an_empty_search_corpus() {
        let environment =
            "<environment_context>ONLY_ENVIRONMENT_CONTEXT_TOKEN</environment_context>";
        let user_msg = json!({
            "id": "fixture-context-only",
            "text": environment,
            "contextBlocks": [{"kind": "environment", "text": environment}],
            "attachments": []
        });
        let codex_block: codex::MessageBlock = serde_json::from_value(json!({
            "id": "b-env",
            "role": "user",
            "kind": "user_message",
            "text": environment,
            "answers": [],
            "questions": [],
            "user_message": user_msg
        }))
        .unwrap();
        let claude_event: claude::ClaudeHistoryEvent = serde_json::from_value(json!({
            "id": "c-env",
            "kind": "user_message",
            "role": "user",
            "text": environment,
            "userMessage": user_msg
        }))
        .unwrap();

        assert!(codex_block_corpus(&codex_block).trim().is_empty());
        assert!(claude_event_corpus(&claude_event).trim().is_empty());
    }

    #[test]
    fn legacy_user_text_fallback_still_excludes_memory_and_page_metadata() {
        let legacy_text = concat!(
            "LEGACY_VISIBLE_BODY_TOKEN\n",
            "<oai-mem-citation>LEGACY_MEMORY_SECRET_TOKEN</oai-mem-citation>\n",
            "<external_codex_apps_open_page>{\"page_id\":\"fixture-page-secret-token\"}</external_codex_apps_open_page>"
        );
        let codex_block: codex::MessageBlock = serde_json::from_value(json!({
            "id": "b-legacy",
            "role": "user",
            "kind": "message",
            "text": legacy_text,
            "answers": [],
            "questions": []
        }))
        .unwrap();
        let claude_event: claude::ClaudeHistoryEvent = serde_json::from_value(json!({
            "id": "c-legacy",
            "kind": "user_message",
            "role": "user",
            "text": legacy_text
        }))
        .unwrap();

        for corpus in [
            codex_block_corpus(&codex_block),
            claude_event_corpus(&claude_event),
        ] {
            assert!(corpus.contains("LEGACY_VISIBLE_BODY_TOKEN"));
            assert!(!corpus.contains("LEGACY_MEMORY_SECRET_TOKEN"));
            assert!(!corpus.contains("fixture-page-secret-token"));
        }
    }

    #[test]
    fn selected_page_snapshot_wrapper_is_stripped_outside_literals_and_never_creates_timeline_marker(
    ) {
        let snapshot = concat!(
            "<external_codex_apps_selected_page_part_1_of_1>Companion Page snapshot part 1 of 1:\n",
            "{\"page_id\":null,\"document_view\":null}\n",
            "</external_codex_apps_selected_page_part_1_of_1>"
        );
        // 1) Snapshot-only message produces empty visible markdown, empty search corpus, and zero timeline markers
        assert_eq!(super::strip_memory_metadata(snapshot).trim(), "");
        let only_event: claude::ClaudeHistoryEvent = serde_json::from_value(json!({
            "id": "msg-snap-only",
            "kind": "user_message",
            "role": "user",
            "text": snapshot,
            "userMessage": {
                "id": "msg-snap-only",
                "text": snapshot,
                "contextBlocks": [],
                "attachments": []
            }
        }))
        .unwrap();
        assert!(claude_event_corpus(&only_event).trim().is_empty());
        let tl = crate::history_navigation::timeline(std::iter::once((
            "msg-snap-only",
            None,
            Some(snapshot),
        )));
        assert!(tl.is_empty());

        // 2) Mixed message strips snapshot JSON while keeping real user request
        let mixed = format!("{snapshot}\n盘点命令型工具");
        assert_eq!(
            super::strip_memory_metadata(&mixed).trim(),
            "盘点命令型工具"
        );

        // 3) Fenced code and mismatched part numbers are preserved verbatim
        let fenced = format!("```xml\n{snapshot}\n```");
        assert_eq!(super::strip_memory_metadata(&fenced), fenced);
        let mismatched = "<external_codex_apps_selected_page_part_1_of_2>Companion Page snapshot part 1 of 2:\n{\"page_id\":null}\n</external_codex_apps_selected_page_part_2_of_2>";
        assert_eq!(super::strip_memory_metadata(mismatched), mismatched);
    }

    #[test]
    fn finds_unicode_snippets_on_character_boundaries() {
        let text = "前置🙂 中文搜索命中后置";
        let snippet = super::find_snippet(text, "中文搜索").expect("unicode match");
        assert!(snippet.contains("中文搜索"));
        assert!(std::str::from_utf8(snippet.as_bytes()).is_ok());
    }

    #[test]
    fn corpus_builders_index_visible_content_and_exclude_internal_identifiers_across_all_providers()
    {
        let block = codex::MessageBlock {
            subagent: Some(codex::subagents::SubagentActivity {
                event_kind: Some(codex::subagents::SubagentEventKind::Interacted),
                event_id: Some("INTERNAL_SUB_EVENT_ID".into()),
                agent_id: Some("INTERNAL_SUB_AGENT_ID".into()),
                name: "worker-alpha".into(),
                role: Some("reviewer".into()),
                status: codex::subagents::SubagentStatus::Interrupted,
                available: false,
                unavailable_reason: Some("子智能体尚未就绪".into()),
                delegation: Some("Audit search corpus".into()),
            }),
            user_message: None,
            phase: Some("INTERNAL_PHASE_TOKEN".into()),
            id: "INTERNAL_BLOCK_ID".into(),
            role: "INTERNAL_ROLE_TOKEN".into(),
            kind: "INTERNAL_KIND_TOKEN".into(),
            display_kind: Some("INTERNAL_DISPLAY_KIND".into()),
            status: Some("INTERNAL_STATUS_TOKEN".into()),
            text: Some("visible reply <oai-mem-citation>secret</oai-mem-citation>".into()),
            summary: Some("ran check".into()),
            input: Some("cargo test".into()),
            truncated: Some(false),
            resolved: Some(true),
            answers: vec![codex::UserInputAnswer {
                question_id: "INTERNAL_ANSWER_QUESTION_ID".into(),
                answers: vec!["Confirm".into()],
                note: Some("ship it".into()),
            }],
            plan_status: Some("INTERNAL_PLAN_STATUS".into()),
            group_id: Some("INTERNAL_GROUP_ID".into()),
            tool_name: Some("exec_command".into()),
            call_id: Some("INTERNAL_CALL_ID".into()),
            turn_id: Some("INTERNAL_TURN_ID".into()),
            item_id: Some("INTERNAL_ITEM_ID".into()),
            created_at: Some("2099-12-31T23:59:59Z".into()),
            questions: vec![codex::UserInputQuestion {
                id: "INTERNAL_QUESTION_ID".into(),
                header: Some("Mode".into()),
                question: "Proceed?".into(),
                options: vec![codex::UserInputOption {
                    label: "Confirm".into(),
                    description: Some("Run now".into()),
                }],
            }],
            payload: Some(json!({
                "turn_id": "INTERNAL_TURN_ID",
                "call_id": "INTERNAL_CALL_ID",
                "memory_citation": "hidden-mem"
            })),
        };
        let codex_typed = codex_block_corpus(&block);
        for visible in [
            "worker-alpha",
            "reviewer",
            "子智能体尚未就绪",
            "Audit search corpus",
            "exec_command",
            "visible reply",
            "ran check",
            "cargo test",
            "Confirm",
            "ship it",
            "Mode",
            "Proceed?",
            "Run now",
        ] {
            assert!(
                codex_typed.contains(visible),
                "expected visible token {visible} in codex corpus: {codex_typed}"
            );
        }
        for hidden in [
            "secret",
            "hidden-mem",
            "INTERNAL_SUB_EVENT_ID",
            "INTERNAL_SUB_AGENT_ID",
            "interacted",
            "interrupted",
            "INTERNAL_PHASE_TOKEN",
            "INTERNAL_BLOCK_ID",
            "INTERNAL_ROLE_TOKEN",
            "INTERNAL_KIND_TOKEN",
            "INTERNAL_DISPLAY_KIND",
            "INTERNAL_STATUS_TOKEN",
            "INTERNAL_ANSWER_QUESTION_ID",
            "INTERNAL_PLAN_STATUS",
            "INTERNAL_GROUP_ID",
            "INTERNAL_CALL_ID",
            "INTERNAL_TURN_ID",
            "INTERNAL_ITEM_ID",
            "2099-12-31T23:59:59Z",
            "INTERNAL_QUESTION_ID",
        ] {
            assert!(
                !codex_typed.contains(hidden),
                "internal token {hidden} must not enter codex corpus: {codex_typed}"
            );
        }

        let claude_ev = claude::ClaudeHistoryEvent {
            id: "INTERNAL_CLAUDE_ID".into(),
            timestamp: Some("2099-12-31T23:59:59Z".into()),
            kind: "INTERNAL_CLAUDE_KIND".into(),
            role: Some("INTERNAL_CLAUDE_ROLE".into()),
            text: Some("cargo clippy".into()),
            call_id: Some("INTERNAL_CLAUDE_CALL_ID".into()),
            turn_id: Some("INTERNAL_CLAUDE_TURN_ID".into()),
            status: Some("INTERNAL_CLAUDE_STATUS".into()),
            detail: Some("linting workspace".into()),
            result: Some("0 warnings".into()),
            user_message: None,
            assistant_phase: Some(claude::ClaudeAssistantPhase::Intermediate),
        };
        let claude_typed = claude_event_corpus(&claude_ev);
        for visible in ["cargo clippy", "linting workspace", "0 warnings"] {
            assert!(claude_typed.contains(visible));
        }
        for hidden in [
            "INTERNAL_CLAUDE_ID",
            "2099-12-31T23:59:59Z",
            "INTERNAL_CLAUDE_KIND",
            "INTERNAL_CLAUDE_ROLE",
            "INTERNAL_CLAUDE_CALL_ID",
            "INTERNAL_CLAUDE_TURN_ID",
            "INTERNAL_CLAUDE_STATUS",
            "intermediate",
        ] {
            assert!(
                !claude_typed.contains(hidden),
                "internal token {hidden} must not enter claude corpus: {claude_typed}"
            );
        }

        let claude_tool = claude::ClaudeHistoryEvent {
            id: "INTERNAL_CLAUDE_TOOL_ID".into(),
            timestamp: Some("2099-12-31T23:59:59Z".into()),
            kind: "tool_call".into(),
            role: Some("Bash".into()),
            text: Some("echo ok".into()),
            call_id: Some("INTERNAL_CLAUDE_TOOL_CALL_ID".into()),
            turn_id: Some("INTERNAL_CLAUDE_TOOL_TURN_ID".into()),
            status: Some("in_progress".into()),
            detail: None,
            result: None,
            user_message: None,
            assistant_phase: None,
        };
        let claude_tool_typed = claude_event_corpus(&claude_tool);
        assert!(claude_tool_typed.contains("Bash"));
        assert!(claude_tool_typed.contains("echo ok"));
        assert!(!claude_tool_typed.contains("in_progress"));
        assert!(!claude_tool_typed.contains("INTERNAL_CLAUDE_TOOL_CALL_ID"));

        let grok_tool_ev = grok::GrokHistoryEvent {
            id: "INTERNAL_GROK_ID".into(),
            user_message: None,
            timestamp: Some("2099-12-31T23:59:59Z".into()),
            kind: "tool_call".into(),
            text: Some("tool output".into()),
            method: Some("Read".into()),
            call_id: Some("INTERNAL_GROK_CALL_ID".into()),
            status: Some("INTERNAL_GROK_STATUS".into()),
            detail: Some("src/lib.rs".into()),
        };
        let grok_typed = grok_event_corpus(&grok_tool_ev);
        for visible in ["tool output", "Read", "src/lib.rs"] {
            assert!(grok_typed.contains(visible));
        }
        for hidden in [
            "INTERNAL_GROK_ID",
            "2099-12-31T23:59:59Z",
            "tool_call",
            "INTERNAL_GROK_CALL_ID",
            "INTERNAL_GROK_STATUS",
        ] {
            assert!(
                !grok_typed.contains(hidden),
                "internal token {hidden} must not enter grok corpus: {grok_typed}"
            );
        }

        let grok_msg_ev = grok::GrokHistoryEvent {
            id: "INTERNAL_GROK_MSG_ID".into(),
            user_message: None,
            timestamp: Some("2099-12-31T23:59:59Z".into()),
            kind: "agent_message_chunk".into(),
            text: Some("visible grok reply".into()),
            method: Some("session/update".into()),
            call_id: None,
            status: None,
            detail: None,
        };
        let grok_msg_typed = grok_event_corpus(&grok_msg_ev);
        assert!(grok_msg_typed.contains("visible grok reply"));
        assert!(
            !grok_msg_typed.contains("session/update"),
            "ACP JSON-RPC envelope method must not enter grok corpus: {grok_msg_typed}"
        );

        let provider_ev = crate::provider_sessions::ProviderHistoryEvent {
            id: "INTERNAL_PROVIDER_ID".into(),
            timestamp: Some("2099-12-31T23:59:59Z".into()),
            kind: "subagent".into(),
            role: Some("INTERNAL_PROVIDER_ROLE".into()),
            text: None,
            call_id: Some("INTERNAL_PROVIDER_CALL_ID".into()),
            turn_id: Some("INTERNAL_PROVIDER_TURN_ID".into()),
            status: Some("INTERNAL_PROVIDER_STATUS".into()),
            detail: None,
            result: None,
            user_message: None,
            subagent: Some(codex::subagents::SubagentActivity {
                event_kind: Some(codex::subagents::SubagentEventKind::Interacted),
                event_id: Some("INTERNAL_PROV_SUB_EID".into()),
                agent_id: Some("INTERNAL_PROV_SUB_AID".into()),
                name: "sub-beta".into(),
                role: Some("auditor".into()),
                status: codex::subagents::SubagentStatus::Running,
                available: false,
                unavailable_reason: Some("离线未水合".into()),
                delegation: Some("Verify corpus".into()),
            }),
        };
        let provider_typed = provider_event_corpus(&provider_ev, false);
        for visible in ["sub-beta", "auditor", "离线未水合", "Verify corpus"] {
            assert!(provider_typed.contains(visible));
        }
        for hidden in [
            "INTERNAL_PROVIDER_ID",
            "INTERNAL_PROVIDER_ROLE",
            "INTERNAL_PROVIDER_CALL_ID",
            "INTERNAL_PROVIDER_TURN_ID",
            "INTERNAL_PROVIDER_STATUS",
            "INTERNAL_PROV_SUB_EID",
            "INTERNAL_PROV_SUB_AID",
            "running",
        ] {
            assert!(!provider_typed.contains(hidden));
        }
    }

    #[test]
    fn search_never_false_matches_turn_id_or_call_id_only_across_codex_claude_and_grok() {
        let root = std::env::temp_dir().join(format!(
            "agenthub-search-no-internal-ids-{}",
            uuid::Uuid::new_v4()
        ));
        let service = isolated_service(&root);

        // 1. Codex rollout with turn_id/call_id on tool call, elicitation question, and assistant reply
        std::fs::create_dir_all(service.codex.sessions_dir()).unwrap();
        let codex_id = "22222222-2222-4222-8222-222222222222";
        let codex_rollout = service.codex.sessions_dir().join("rollout.jsonl");
        let codex_rows = [
            json!({"type":"session_meta","payload":{"id":codex_id,"source":"cli"}}),
            json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"TURN_ONLY_NEEDLE_XYZ"}}),
            json!({
                "type":"response_item",
                "turn_id":"TURN_ONLY_NEEDLE_XYZ",
                "payload":{
                    "type":"function_call",
                    "name":"exec_command",
                    "call_id":"CALL_ONLY_NEEDLE_XYZ",
                    "arguments":"{\"cmd\":\"echo VISIBLE_CODEX_STDOUT\"}"
                }
            }),
            json!({
                "type":"response_item",
                "turn_id":"TURN_ONLY_NEEDLE_XYZ",
                "payload":{
                    "type":"function_call_output",
                    "call_id":"CALL_ONLY_NEEDLE_XYZ",
                    "output":"VISIBLE_CODEX_STDOUT"
                }
            }),
            json!({
                "type":"response_item",
                "turn_id":"TURN_ONLY_NEEDLE_XYZ",
                "payload":{
                    "type":"function_call",
                    "name":"request_user_input",
                    "call_id":"CALL_QUESTION_NEEDLE_XYZ",
                    "arguments":json!({
                        "questions":[{
                            "id":"QID_ONLY_NEEDLE_XYZ",
                            "question":"VISIBLE_QUESTION_TEXT",
                            "options":[{"label":"Yes"}]
                        }]
                    }).to_string()
                }
            }),
        ];
        std::fs::write(
            &codex_rollout,
            codex_rows
                .iter()
                .map(|r| format!("{r}\n"))
                .collect::<String>(),
        )
        .unwrap();
        let conn = rusqlite::Connection::open(service.codex.state_db()).unwrap();
        conn.execute_batch("CREATE TABLE threads(id TEXT PRIMARY KEY,title TEXT,rollout_path TEXT,source TEXT,archived INTEGER,created_at INTEGER,updated_at INTEGER);").unwrap();
        conn.execute(
            "INSERT INTO threads VALUES(?1,'Clean Title',?2,'cli',0,1,1)",
            rusqlite::params![codex_id, codex_rollout.to_str().unwrap()],
        )
        .unwrap();
        drop(conn);

        for internal_query in [
            "TURN_ONLY_NEEDLE_XYZ",
            "CALL_ONLY_NEEDLE_XYZ",
            "CALL_QUESTION_NEEDLE_XYZ",
            "QID_ONLY_NEEDLE_XYZ",
        ] {
            let res = service
                .search(SessionSearchRequest {
                    provider: SessionProvider::Codex,
                    scope: SearchScope::Thread,
                    session_key: Some(codex_id.into()),
                    query: internal_query.into(),
                    cursor: None,
                    limit: None,
                })
                .unwrap();
            assert!(
                res.results.is_empty(),
                "Codex search must not match internal field {internal_query}: {:?}",
                res.results
            );
        }
        assert_eq!(
            service
                .search(SessionSearchRequest {
                    provider: SessionProvider::Codex,
                    scope: SearchScope::Thread,
                    session_key: Some(codex_id.into()),
                    query: "VISIBLE_QUESTION_TEXT".into(),
                    cursor: None,
                    limit: None,
                })
                .unwrap()
                .results
                .len(),
            1
        );

        // 2. Claude session with uuid (turn_id) and tool_use id (call_id)
        let claude_proj = service.claude.projects.join("proj");
        std::fs::create_dir_all(&claude_proj).unwrap();
        let claude_id = "33333333-3333-4333-8333-333333333333";
        let claude_rows = [
            json!({
                "type":"user",
                "uuid":"TURN_ONLY_CLAUDE_UUID",
                "sessionId":claude_id,
                "version":"2.1.0",
                "message":{"role":"user","content":"Hello Claude"}
            }),
            json!({
                "type":"assistant",
                "uuid":"ASST_ONLY_CLAUDE_UUID",
                "sessionId":claude_id,
                "version":"2.1.0",
                "message":{
                    "role":"assistant",
                    "stop_reason":"tool_use",
                    "content":[{
                        "type":"tool_use",
                        "id":"CALL_ONLY_CLAUDE_ID",
                        "name":"Bash",
                        "input":{"command":"echo VISIBLE_CLAUDE_CMD"}
                    }]
                }
            }),
        ];
        std::fs::write(
            claude_proj.join(format!("{claude_id}.jsonl")),
            claude_rows
                .iter()
                .map(|r| format!("{r}\n"))
                .collect::<String>(),
        )
        .unwrap();
        let claude_key = claude::list_claude_sessions_without_storage(&service.claude, 10, None)
            .unwrap()
            .remove(0)
            .session_key;
        for internal_query in [
            "TURN_ONLY_CLAUDE_UUID",
            "ASST_ONLY_CLAUDE_UUID",
            "CALL_ONLY_CLAUDE_ID",
        ] {
            let res = service
                .search(SessionSearchRequest {
                    provider: SessionProvider::Claude,
                    scope: SearchScope::Thread,
                    session_key: Some(claude_key.clone()),
                    query: internal_query.into(),
                    cursor: None,
                    limit: None,
                })
                .unwrap();
            assert!(
                res.results.is_empty(),
                "Claude search must not match internal field {internal_query}: {:?}",
                res.results
            );
        }
        assert_eq!(
            service
                .search(SessionSearchRequest {
                    provider: SessionProvider::Claude,
                    scope: SearchScope::Thread,
                    session_key: Some(claude_key),
                    query: "VISIBLE_CLAUDE_CMD".into(),
                    cursor: None,
                    limit: None,
                })
                .unwrap()
                .results
                .len(),
            1
        );

        // 3. Grok sessions: chat_history.jsonl tool_call id + updates.jsonl ACP method="session/update"
        let grok_id = "44444444-4444-4444-8444-444444444444";
        let grok_dir = service.grok.sessions().join("ws").join(grok_id);
        std::fs::create_dir_all(&grok_dir).unwrap();
        let grok_rows = [
            json!({"type":"user","content":"Hello Grok"}),
            json!({"type":"assistant","tool_calls":[{"id":"CALL_ONLY_GROK_ID","name":"Read","arguments":"{\"path\":\"VISIBLE_GROK_PATH\"}"}]}),
        ];
        std::fs::write(
            grok_dir.join("chat_history.jsonl"),
            grok_rows
                .iter()
                .map(|r| format!("{r}\n"))
                .collect::<String>(),
        )
        .unwrap();
        let grok_res = service
            .search(SessionSearchRequest {
                provider: SessionProvider::Grok,
                scope: SearchScope::Thread,
                session_key: Some(grok_id.into()),
                query: "CALL_ONLY_GROK_ID".into(),
                cursor: None,
                limit: None,
            })
            .unwrap();
        assert!(grok_res.results.is_empty());
        assert_eq!(
            service
                .search(SessionSearchRequest {
                    provider: SessionProvider::Grok,
                    scope: SearchScope::Thread,
                    session_key: Some(grok_id.into()),
                    query: "VISIBLE_GROK_PATH".into(),
                    cursor: None,
                    limit: None,
                })
                .unwrap()
                .results
                .len(),
            1
        );

        let grok_acp_id = "55555555-5555-4555-8555-555555555555";
        let grok_acp_dir = service.grok.sessions().join("ws").join(grok_acp_id);
        std::fs::create_dir_all(&grok_acp_dir).unwrap();
        std::fs::write(
            grok_acp_dir.join("summary.json"),
            json!({"info":{"id":grok_acp_id,"title":"Plain Title","cwd":"/workspace"}}).to_string(),
        )
        .unwrap();
        let grok_updates = [
            json!({"timestamp":"2026-01-01T00:00:00Z","method":"session/update","params":{"sessionId":grok_acp_id,"update":{"sessionUpdate":"user_message_chunk","messageId":"m1","content":{"type":"text","text":"Ask Grok"}}}}),
            json!({"timestamp":"2026-01-01T00:00:01Z","method":"session/update","params":{"sessionId":grok_acp_id,"update":{"sessionUpdate":"tool_call","toolCallId":"CALL_ONLY_GROK_ACP_ID","title":"Read","status":"completed","content":[{"type":"content","content":{"type":"text","text":"VISIBLE_ACP_TOOL"}}]}}}),
            json!({"timestamp":"2026-01-01T00:00:02Z","method":"session/update","params":{"sessionId":grok_acp_id,"update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"VISIBLE_ACP_REPLY"}}}}),
        ];
        std::fs::write(
            grok_acp_dir.join("updates.jsonl"),
            grok_updates
                .iter()
                .map(|r| format!("{r}\n"))
                .collect::<String>(),
        )
        .unwrap();
        for internal_query in ["session/update", "CALL_ONLY_GROK_ACP_ID"] {
            let acp_res = service
                .search(SessionSearchRequest {
                    provider: SessionProvider::Grok,
                    scope: SearchScope::Thread,
                    session_key: Some(grok_acp_id.into()),
                    query: internal_query.into(),
                    cursor: None,
                    limit: None,
                })
                .unwrap();
            assert!(
                acp_res.results.is_empty(),
                "Grok ACP search must not match internal token {internal_query}: {:?}",
                acp_res.results
            );
        }
        assert_eq!(
            service
                .search(SessionSearchRequest {
                    provider: SessionProvider::Grok,
                    scope: SearchScope::Thread,
                    session_key: Some(grok_acp_id.into()),
                    query: "VISIBLE_ACP_REPLY".into(),
                    cursor: None,
                    limit: None,
                })
                .unwrap()
                .results
                .len(),
            1
        );

        std::fs::remove_dir_all(root).unwrap();
    }
}
