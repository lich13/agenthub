use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::{archive, codex::CodexPaths, platform::PlatformPaths};

pub const UNIFIED_CLEANUP_CONFIRMATION_MESSAGE: &str = "unified thread cleanup must be confirmed";

pub const SCOPE_CODEX_ARCHIVED: &str = "codex_archived";
pub const SCOPE_CODEX_HIDDEN: &str = "codex_hidden";
pub const SCOPE_ANTIGRAVITY_ARCHIVED: &str = "antigravity_archived";
pub const SCOPE_DSH_ARCHIVED: &str = "dsh_archived";
pub const SCOPE_CLAUDE_EMPTY: &str = "claude_empty";
pub const SCOPE_GROK_EMPTY: &str = "grok_empty";

#[derive(Clone, Copy)]
enum CleanupScope {
    CodexArchived,
    CodexHidden,
    AntigravityArchived,
    DshArchived,
    ClaudeEmpty,
    GrokEmpty,
}

const ALL_UNIFIED_SCOPES: &[CleanupScope] = &[
    CleanupScope::CodexArchived,
    CleanupScope::CodexHidden,
    CleanupScope::AntigravityArchived,
    CleanupScope::DshArchived,
    CleanupScope::ClaudeEmpty,
    CleanupScope::GrokEmpty,
];
impl CleanupScope {
    fn meta(self) -> (&'static str, &'static str, &'static str) {
        match self {
            Self::CodexArchived => (SCOPE_CODEX_ARCHIVED, "codex", "Codex 已归档线程"),
            Self::CodexHidden => (SCOPE_CODEX_HIDDEN, "codex", "Codex 隐藏线程"),
            Self::AntigravityArchived => (
                SCOPE_ANTIGRAVITY_ARCHIVED,
                "antigravity",
                "Antigravity 已归档与孤立子会话",
            ),
            Self::DshArchived => (SCOPE_DSH_ARCHIVED, "dsh", "DeepSeek Harness 已归档线程"),
            Self::ClaudeEmpty => (SCOPE_CLAUDE_EMPTY, "claude", "Claude Code 空残余会话"),
            Self::GrokEmpty => (SCOPE_GROK_EMPTY, "grok", "Grok Build 空残余会话"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct UnifiedCleanupPaths {
    pub codex: CodexPaths,
    pub antigravity: crate::antigravity::sessions::AntigravityPaths,
    pub dsh: crate::dsh::DshPaths,
    pub claude: crate::claude::ClaudePaths,
    pub grok: crate::grok::GrokPaths,
}

impl UnifiedCleanupPaths {
    pub fn default_for_codex(codex: CodexPaths) -> Result<Self> {
        Ok(Self {
            codex,
            antigravity: crate::antigravity::sessions::AntigravityPaths::default_for_user()?,
            dsh: crate::dsh::DshPaths::default_for_user()?,
            claude: crate::claude::ClaudePaths::default_for_user()?,
            grok: crate::grok::GrokPaths::default_for_user()?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnifiedCleanupCandidate {
    pub id: String,
    pub title: String,
    pub estimated_bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,
    pub blocked_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnifiedCleanupTargetSelection {
    pub scope: String,
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnifiedCleanupScopePlan {
    pub scope: String,
    pub provider: String,
    pub label: String,
    pub supported: bool,
    pub scanned: bool,
    pub unavailable_reason: Option<String>,
    pub candidate_count: usize,
    pub deletable_count: usize,
    pub estimated_bytes: u64,
    pub candidates: Vec<UnifiedCleanupCandidate>,
    pub issues: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnifiedCleanupPlan {
    pub total_candidates: usize,
    pub total_deletable: usize,
    pub total_estimated_bytes: u64,
    pub scopes: Vec<UnifiedCleanupScopePlan>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnifiedCleanupExecuteRequest {
    pub confirmed: bool,
    #[serde(default)]
    pub scopes: Vec<String>,
    #[serde(default)]
    pub targets: Vec<UnifiedCleanupTargetSelection>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnifiedCleanupScopeResult {
    pub scope: String,
    pub provider: String,
    pub label: String,
    pub deleted_count: usize,
    pub skipped_count: usize,
    pub failed_count: usize,
    pub deleted_bytes: u64,
    pub residuals: Vec<String>,
    pub issues: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnifiedCleanupResult {
    pub deleted_count: usize,
    pub skipped_count: usize,
    pub failed_count: usize,
    pub deleted_bytes: u64,
    pub scopes: Vec<UnifiedCleanupScopeResult>,
}
fn scope_meta(scope: &str) -> Result<(&'static str, &'static str)> {
    ALL_UNIFIED_SCOPES
        .iter()
        .map(|scope| scope.meta())
        .find(|(id, _, _)| *id == scope)
        .map(|(_, provider, label)| (provider, label))
        .ok_or_else(|| anyhow::anyhow!("未知的清理范围: {scope}"))
}

fn push_candidate(
    out: &mut UnifiedCleanupScopePlan,
    id: String,
    title: String,
    bytes: u64,
    fingerprint: Option<String>,
    blocked_reason: Option<String>,
) {
    if blocked_reason.is_none() {
        out.deletable_count += 1;
        out.estimated_bytes = out.estimated_bytes.saturating_add(bytes);
    }
    out.candidates.push(UnifiedCleanupCandidate {
        title: if title.trim().is_empty() {
            id.clone()
        } else {
            title
        },
        id,
        estimated_bytes: bytes,
        fingerprint,
        blocked_reason,
    });
    out.candidate_count = out.candidates.len();
}

fn dry_run_single_scope(
    _platform: &PlatformPaths,
    paths: &UnifiedCleanupPaths,
    kind: CleanupScope,
) -> UnifiedCleanupScopePlan {
    let (scope, provider, label) = kind.meta();
    let mut out = UnifiedCleanupScopePlan {
        scope: scope.to_owned(),
        provider: provider.to_owned(),
        label: label.to_owned(),
        supported: true,
        scanned: true,
        unavailable_reason: None,
        candidate_count: 0,
        deletable_count: 0,
        estimated_bytes: 0,
        candidates: Vec::new(),
        issues: Vec::new(),
    };
    match kind {
        CleanupScope::CodexArchived => match archive::list_archived_threads(&paths.codex) {
            Ok(candidates) => {
                let ids = candidates.iter().map(|c| c.id.clone()).collect::<Vec<_>>();
                let bytes_map =
                    crate::selected_codex::tree::estimate_bytes_batch(&paths.codex, &ids);
                for candidate in candidates {
                    let bytes = bytes_map.get(&candidate.id).copied().unwrap_or(0);
                    push_candidate(
                        &mut out,
                        candidate.id,
                        candidate.title,
                        bytes,
                        candidate.fingerprint,
                        None,
                    );
                }
            }
            Err(err) => out.issues.push(err.to_string()),
        },
        CleanupScope::CodexHidden => {
            out.scanned = false;
        }
        CleanupScope::AntigravityArchived => {
            match crate::antigravity::sessions::preview_unified_cleanup_candidates(
                &paths.antigravity,
            ) {
                Ok(candidates) => {
                    for c in candidates {
                        push_candidate(
                            &mut out,
                            c.id,
                            c.title,
                            c.bytes,
                            c.fingerprint,
                            c.blocked_reason,
                        );
                    }
                }
                Err(err) => out.issues.push(err.to_string()),
            }
        }
        CleanupScope::DshArchived => match crate::dsh::preview_with_bytes(&paths.dsh) {
            Ok((plan, sizes)) => {
                out.issues = plan.issues;
                for c in plan.candidates {
                    let bytes = sizes.get(&c.id).copied().unwrap_or(0);
                    let title = match &c.parent_id {
                        Some(parent) => format!("{} (子会话 ← {parent})", c.id),
                        None => c.id.clone(),
                    };
                    push_candidate(&mut out, c.id, title, bytes, Some(c.archive_id), c.reason);
                }
            }
            Err(err) => out.issues.push(err.to_string()),
        },
        CleanupScope::ClaudeEmpty => {
            match crate::claude::find_empty_inactive_sessions(&paths.claude) {
                Ok(items) => {
                    for (summary, fp, bytes) in items {
                        push_candidate(
                            &mut out,
                            summary.session_key,
                            summary.title,
                            bytes,
                            Some(fp),
                            None,
                        );
                    }
                }
                Err(err) => out.issues.push(err.to_string()),
            }
        }
        CleanupScope::GrokEmpty => match crate::grok::find_empty_inactive_sessions(&paths.grok) {
            Ok(items) => {
                for (summary, fp, bytes) in items {
                    push_candidate(&mut out, summary.id, summary.title, bytes, Some(fp), None);
                }
            }
            Err(err) => out.issues.push(err.to_string()),
        },
    }
    out
}

pub fn dry_run_unified_with_capability(
    platform: &PlatformPaths,
    paths: &UnifiedCleanupPaths,
) -> Result<UnifiedCleanupPlan> {
    let mut plan = UnifiedCleanupPlan::default();
    for scope in ALL_UNIFIED_SCOPES {
        let item = dry_run_single_scope(platform, paths, *scope);
        plan.total_candidates += item.candidate_count;
        plan.total_deletable += item.deletable_count;
        plan.total_estimated_bytes = plan
            .total_estimated_bytes
            .saturating_add(item.estimated_bytes);
        plan.scopes.push(item);
    }
    Ok(plan)
}

fn validate_unified_target_id(scope: &str, id: &str) -> Result<()> {
    let trimmed = id.trim();
    anyhow::ensure!(
        !trimmed.is_empty()
            && trimmed == id
            && !id.contains('\0')
            && !id.contains('\\')
            && !id.starts_with('/')
            && !id
                .split('/')
                .any(|seg| seg.is_empty() || seg == "." || seg == ".."),
        "清理目标 ID 格式越界或非法: {id}"
    );
    match scope {
        SCOPE_CLAUDE_EMPTY => {
            anyhow::ensure!(
                id.starts_with("claude:")
                    && id.len() == 71
                    && id[7..].bytes().all(|c| c.is_ascii_hexdigit()),
                "Claude 清理目标键格式非法: {id}"
            );
        }
        SCOPE_ANTIGRAVITY_ARCHIVED => {
            let raw = id.strip_prefix("ag:").unwrap_or(id);
            crate::session_delete::valid_id(raw)?;
        }
        _ => {
            crate::session_delete::valid_id(id)?;
        }
    }
    Ok(())
}

enum TargetStepOutcome {
    Deleted(u64),
    Skipped(String),
    Failed(Option<String>, Vec<String>),
}

fn record_target_result(
    scope: &mut UnifiedCleanupScopeResult,
    deleted_ids: Option<&mut Vec<String>>,
    id: &str,
    result: Result<TargetStepOutcome>,
) {
    match result.unwrap_or_else(|error| TargetStepOutcome::Failed(Some(error.to_string()), vec![]))
    {
        TargetStepOutcome::Deleted(bytes) => {
            scope.deleted_count += 1;
            scope.deleted_bytes = scope.deleted_bytes.saturating_add(bytes);
            if let Some(out) = deleted_ids {
                out.push(id.to_owned());
            }
        }
        TargetStepOutcome::Skipped(message) => {
            scope.skipped_count += 1;
            scope.issues.push(format!("{id}: {message}"));
        }
        TargetStepOutcome::Failed(message, residuals) => {
            scope.failed_count += 1;
            if let Some(message) = message {
                scope.issues.push(format!("{id}: {message}"));
            }
            scope.residuals.extend(residuals);
        }
    }
}

fn verified_scope_targets(
    targets: Vec<UnifiedCleanupTargetSelection>,
    scope: &mut UnifiedCleanupScopeResult,
    mut check: impl FnMut(&str, &str) -> Result<Option<&'static str>>,
) -> Vec<UnifiedCleanupTargetSelection> {
    targets
        .into_iter()
        .filter_map(|target| {
            let result = match target.fingerprint.as_deref() {
                Some(fingerprint) => check(&target.id, fingerprint),
                None => Ok(Some("缺少预览指纹，已跳过")),
            };
            let outcome = match result {
                Ok(None) => return Some(target),
                Ok(Some(message)) => Ok(TargetStepOutcome::Skipped(message.into())),
                Err(error) => Err(error),
            };
            record_target_result(scope, None, &target.id, outcome);
            None
        })
        .collect()
}

fn execute_codex_scope(
    codex: &CodexPaths,
    targets: Vec<UnifiedCleanupTargetSelection>,
    is_hidden: bool,
    scope: &mut UnifiedCleanupScopeResult,
) -> Result<()> {
    let db_path = codex.state_db();
    let conn = rusqlite::Connection::open_with_flags(
        &db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| anyhow::anyhow!("open {}: {e}", db_path.display()))?;
    let thread_cols = archive::table_columns(&conn, "threads")?;
    let edge_cols = archive::table_columns(&conn, "thread_spawn_edges")?;
    let has_spawn_edges =
        edge_cols.contains("parent_thread_id") && edge_cols.contains("child_thread_id");
    let edges = crate::selected_codex::tree::spawn_edges_with_connection(&conn)?;
    let targets = verified_scope_targets(targets, scope, |id, fingerprint| {
        let verified = if is_hidden {
            archive::verify_hidden_target_fingerprint(&conn, &thread_cols, has_spawn_edges, id)?
        } else {
            archive::verify_archived_target_fingerprint(&conn, &thread_cols, has_spawn_edges, id)?
        };
        match verified {
            Some(current) if current == fingerprint => {}
            Some(_) => return Ok(Some("线程状态或指纹已变化，已跳过")),
            None if crate::selected_codex::tree::is_recorded_delete_residual(codex, id)
                && crate::selected_codex::tree::has_orphan_artifacts(codex, id) => {}
            None => return Ok(Some("目标不在当前允许集合中，已跳过")),
        }
        let tree_ids =
            crate::selected_codex::tree::descendants_from_edges(&edges, &[id.to_owned()])?;
        crate::selected_codex::tree::ensure_ids_not_running(codex, &tree_ids, Some(&conn))?;
        Ok(None)
    });
    let selections = targets
        .iter()
        .map(|target| vec![target.id.clone()])
        .collect::<Vec<_>>();
    for (target, result) in targets
        .iter()
        .zip(crate::selected_codex::tree::execute_batch(
            codex,
            &selections,
        ))
    {
        record_target_result(
            scope,
            None,
            &target.id,
            result.map(|out| {
                if out.error.is_none() && out.residuals.is_empty() {
                    TargetStepOutcome::Deleted(out.deleted_bytes)
                } else {
                    TargetStepOutcome::Failed(out.error, out.residuals)
                }
            }),
        );
    }
    Ok(())
}

fn execute_antigravity_archived_scope(
    antigravity: &crate::antigravity::sessions::AntigravityPaths,
    scope_targets: Vec<UnifiedCleanupTargetSelection>,
    scope_res: &mut UnifiedCleanupScopeResult,
    deleted_ids: &mut Vec<String>,
) -> Result<()> {
    use crate::antigravity::sessions::UnifiedAntigravityStepOutcome;
    let pairs: Vec<(String, Option<String>)> = scope_targets
        .into_iter()
        .map(|t| (t.id, t.fingerprint))
        .collect();
    for (id, outcome) in
        crate::antigravity::sessions::execute_unified_cleanup_targets(antigravity, &pairs)?
    {
        let mapped = match outcome {
            UnifiedAntigravityStepOutcome::Deleted(bytes) => TargetStepOutcome::Deleted(bytes),
            UnifiedAntigravityStepOutcome::Skipped(msg) => TargetStepOutcome::Skipped(msg),
            UnifiedAntigravityStepOutcome::Failed(msg, residuals) => {
                TargetStepOutcome::Failed(msg, residuals)
            }
        };
        record_target_result(scope_res, Some(deleted_ids), &id, Ok(mapped));
    }
    Ok(())
}

fn execute_dsh_archived_scope(
    paths: &crate::dsh::DshPaths,
    targets: Vec<UnifiedCleanupTargetSelection>,
    scope: &mut UnifiedCleanupScopeResult,
    deleted_ids: &mut Vec<String>,
) -> Result<()> {
    let targets = verified_scope_targets(targets, scope, |_, _| Ok(None));
    if targets.is_empty() {
        return Ok(());
    }
    let result = crate::dsh::execute(
        paths,
        crate::dsh::DshArchiveDeleteRequest {
            confirmed: true,
            targets: targets
                .into_iter()
                .map(|target| crate::dsh::DshArchiveSelection {
                    id: target.id,
                    archive_id: target.fingerprint.unwrap(),
                })
                .collect(),
        },
    )?;
    scope.deleted_bytes = result.deleted_bytes;
    for item in result.items {
        record_target_result(
            scope,
            Some(deleted_ids),
            &item.id,
            Ok(match item.status.as_str() {
                "deleted" => TargetStepOutcome::Deleted(0),
                "skipped" => TargetStepOutcome::Skipped(item.reason.unwrap_or_default()),
                _ => TargetStepOutcome::Failed(item.reason, item.residuals),
            }),
        );
    }
    Ok(())
}

fn empty_scope_targets(
    targets: Vec<UnifiedCleanupTargetSelection>,
    allowed: &std::collections::BTreeMap<String, String>,
    check_running: impl Fn(&str) -> Option<String>,
    scope: &mut UnifiedCleanupScopeResult,
) -> Vec<UnifiedCleanupTargetSelection> {
    verified_scope_targets(targets, scope, |id, fingerprint| match allowed.get(id) {
        Some(current) if current == fingerprint => Ok(None),
        Some(_) => Ok(Some("会话指纹已变化，已跳过")),
        None => {
            if let Some(reason) = check_running(id) {
                anyhow::bail!(reason);
            }
            Ok(Some("会话不再属于空残余允许集合，已跳过"))
        }
    })
}

fn execute_claude_empty_scope(
    paths: &crate::claude::ClaudePaths,
    targets: Vec<UnifiedCleanupTargetSelection>,
    scope: &mut UnifiedCleanupScopeResult,
    deleted_ids: &mut Vec<String>,
) -> Result<()> {
    let allowed = crate::claude::find_empty_inactive_sessions(paths)?
        .into_iter()
        .map(|(s, fp, _)| (s.session_key, fp))
        .collect();
    let targets = empty_scope_targets(
        targets,
        &allowed,
        |id| {
            crate::claude::claude_session_summary(paths, id)
                .ok()
                .and_then(|s| {
                    (s.status == "running"
                        || s.delete_block_reason
                            .as_deref()
                            .is_some_and(|r| r.contains("正在运行") || r.contains("占用")))
                    .then(|| {
                        s.delete_block_reason
                            .unwrap_or_else(|| "Claude 会话正在运行，无法删除".into())
                    })
                })
        },
        scope,
    );
    let requests = targets
        .iter()
        .map(|target| crate::claude::ClaudeDeleteRequest {
            session_key: target.id.clone(),
            confirmed: true,
            fingerprint: target.fingerprint.clone().unwrap(),
        })
        .collect();
    for (target, result) in targets
        .iter()
        .zip(crate::claude::execute_claude_delete_batch(paths, requests))
    {
        record_target_result(
            scope,
            Some(deleted_ids),
            &target.id,
            result.map(|r| {
                if r.deleted && r.residuals.is_empty() {
                    TargetStepOutcome::Deleted(r.bytes)
                } else {
                    TargetStepOutcome::Failed(None, r.residuals)
                }
            }),
        );
    }
    Ok(())
}

fn execute_grok_empty_scope(
    paths: &crate::grok::GrokPaths,
    targets: Vec<UnifiedCleanupTargetSelection>,
    scope: &mut UnifiedCleanupScopeResult,
    deleted_ids: &mut Vec<String>,
) -> Result<()> {
    let allowed = crate::grok::find_empty_inactive_sessions(paths)?
        .into_iter()
        .map(|(s, fp, _)| (s.id, fp))
        .collect();
    let targets = empty_scope_targets(
        targets,
        &allowed,
        |id| crate::grok::empty_cleanup_block_reason(paths, id),
        scope,
    );
    let requests = targets
        .iter()
        .map(|target| crate::grok::GrokDeleteRequest {
            id: target.id.clone(),
            confirmed: true,
            fingerprint: target.fingerprint.clone().unwrap(),
        })
        .collect();
    for (target, result) in targets
        .iter()
        .zip(crate::grok::execute_grok_empty_cleanup_batch(
            paths, requests,
        ))
    {
        record_target_result(
            scope,
            Some(deleted_ids),
            &target.id,
            result.map(|r| {
                if r.deleted && r.residuals.is_empty() {
                    TargetStepOutcome::Deleted(r.bytes)
                } else {
                    TargetStepOutcome::Failed(None, r.residuals)
                }
            }),
        );
    }
    Ok(())
}

pub type DeletedProbeStreams = Vec<(crate::native_probe::NativeProvider, Vec<String>)>;

#[cfg(test)]
fn execute_unified_with_capability(
    platform: &PlatformPaths,
    paths: &UnifiedCleanupPaths,
    request: UnifiedCleanupExecuteRequest,
) -> Result<UnifiedCleanupResult> {
    execute_unified_with_deleted_streams(platform, paths, request).map(|(res, _)| res)
}

pub fn execute_unified_with_deleted_streams(
    _platform: &PlatformPaths,
    paths: &UnifiedCleanupPaths,
    request: UnifiedCleanupExecuteRequest,
) -> Result<(UnifiedCleanupResult, DeletedProbeStreams)> {
    anyhow::ensure!(request.confirmed, UNIFIED_CLEANUP_CONFIRMATION_MESSAGE);
    anyhow::ensure!(
        !request.targets.is_empty(),
        "请先扫描预览并确认待清理会话列表"
    );
    for s in &request.scopes {
        scope_meta(s)?;
    }

    let mut targets_by_scope: std::collections::BTreeMap<
        String,
        Vec<UnifiedCleanupTargetSelection>,
    > = std::collections::BTreeMap::new();
    let mut seen_pairs: std::collections::BTreeMap<(String, String), Option<String>> =
        std::collections::BTreeMap::new();
    for target in &request.targets {
        scope_meta(&target.scope)?;
        validate_unified_target_id(&target.scope, &target.id)?;
        let norm_fp = target
            .fingerprint
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned);
        let key = (target.scope.clone(), target.id.clone());
        if let Some(prev_fp) = seen_pairs.get(&key) {
            anyhow::ensure!(
                prev_fp == &norm_fp,
                "清理目标 {} 指纹冲突，请重新预览",
                target.id
            );
            continue;
        }
        seen_pairs.insert(key, norm_fp.clone());
        if request.scopes.is_empty() || request.scopes.iter().any(|s| s == &target.scope) {
            targets_by_scope
                .entry(target.scope.clone())
                .or_default()
                .push(UnifiedCleanupTargetSelection {
                    scope: target.scope.clone(),
                    id: target.id.clone(),
                    fingerprint: norm_fp,
                });
        }
    }

    let ordered_scopes: Vec<&str> = ALL_UNIFIED_SCOPES
        .iter()
        .map(|scope| scope.meta().0)
        .filter(|&known| {
            if request.scopes.is_empty() {
                targets_by_scope.contains_key(known)
            } else {
                request.scopes.iter().any(|s| s == known)
            }
        })
        .collect();
    anyhow::ensure!(!ordered_scopes.is_empty(), "请至少选择一个待清理平台范围");

    let mut total = UnifiedCleanupResult::default();
    let mut deleted_streams = Vec::new();
    for scope in ordered_scopes {
        let (provider, label) = scope_meta(scope)?;
        let mut scope_res = UnifiedCleanupScopeResult {
            scope: scope.to_owned(),
            provider: provider.to_owned(),
            label: label.to_owned(),
            ..Default::default()
        };
        let mut scope_deleted_ids = Vec::new();
        let scope_targets = targets_by_scope.remove(scope).unwrap_or_default();
        if !scope_targets.is_empty() {
            let target_count = scope_targets.len();
            let result = match scope {
                SCOPE_CODEX_ARCHIVED | SCOPE_CODEX_HIDDEN => execute_codex_scope(
                    &paths.codex,
                    scope_targets,
                    scope == SCOPE_CODEX_HIDDEN,
                    &mut scope_res,
                ),
                SCOPE_ANTIGRAVITY_ARCHIVED => execute_antigravity_archived_scope(
                    &paths.antigravity,
                    scope_targets,
                    &mut scope_res,
                    &mut scope_deleted_ids,
                ),
                SCOPE_DSH_ARCHIVED => execute_dsh_archived_scope(
                    &paths.dsh,
                    scope_targets,
                    &mut scope_res,
                    &mut scope_deleted_ids,
                ),
                SCOPE_CLAUDE_EMPTY => execute_claude_empty_scope(
                    &paths.claude,
                    scope_targets,
                    &mut scope_res,
                    &mut scope_deleted_ids,
                ),
                SCOPE_GROK_EMPTY => execute_grok_empty_scope(
                    &paths.grok,
                    scope_targets,
                    &mut scope_res,
                    &mut scope_deleted_ids,
                ),
                _ => unreachable!("validated cleanup scope"),
            };
            if let Err(error) = result {
                scope_res.failed_count += target_count
                    - scope_res.deleted_count
                    - scope_res.skipped_count
                    - scope_res.failed_count;
                scope_res.issues.push(error.to_string());
            }
        }
        if !scope_deleted_ids.is_empty() {
            match scope {
                SCOPE_ANTIGRAVITY_ARCHIVED => {
                    let mut normalized = Vec::with_capacity(scope_deleted_ids.len() * 2);
                    for id in scope_deleted_ids {
                        if let Some(raw) = id.strip_prefix("ag:") {
                            normalized.push(raw.to_owned());
                        }
                        normalized.push(id);
                    }
                    deleted_streams
                        .push((crate::native_probe::NativeProvider::Antigravity, normalized));
                }
                SCOPE_DSH_ARCHIVED => {
                    deleted_streams
                        .push((crate::native_probe::NativeProvider::Dsh, scope_deleted_ids));
                }
                SCOPE_CLAUDE_EMPTY => {
                    deleted_streams.push((
                        crate::native_probe::NativeProvider::Claude,
                        scope_deleted_ids,
                    ));
                }
                SCOPE_GROK_EMPTY => {
                    deleted_streams
                        .push((crate::native_probe::NativeProvider::Grok, scope_deleted_ids));
                }
                _ => {}
            }
        }
        total.deleted_count += scope_res.deleted_count;
        total.skipped_count += scope_res.skipped_count;
        total.failed_count += scope_res.failed_count;
        total.deleted_bytes = total.deleted_bytes.saturating_add(scope_res.deleted_bytes);
        total.scopes.push(scope_res);
    }
    Ok((total, deleted_streams))
}

#[cfg(test)]
mod tests {
    use crate::platform::PlatformPaths;

    #[test]
    fn unified_cleanup_recomputes_targets_preserves_non_empty_and_continues_after_failure() {
        use std::fs;
        let tmp =
            std::env::temp_dir().join(format!("agenthub-unified-cleanup-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&tmp).unwrap();

        let codex = crate::codex::CodexPaths::new(tmp.join("codex"));
        fs::create_dir_all(&codex.home).unwrap();
        let antigravity =
            crate::antigravity::sessions::AntigravityPaths::offline(tmp.join("antigravity"));
        fs::create_dir_all(antigravity.conversations_dir()).unwrap();
        fs::create_dir_all(antigravity.annotations_dir()).unwrap();
        let dsh = crate::dsh::DshPaths {
            home: tmp.join("dsh"),
        };
        fs::create_dir_all(&dsh.home).unwrap();
        let claude = crate::claude::ClaudePaths {
            config: tmp.join("claude"),
            projects: tmp.join("claude/projects"),
        };
        fs::create_dir_all(claude.projects.join("proj")).unwrap();
        let grok = crate::grok::GrokPaths {
            home: tmp.join("grok"),
        };
        fs::create_dir_all(grok.sessions().join("ws")).unwrap();

        // Seed 1 empty Claude session + 1 non-empty Claude session
        let claude_empty_id = "11111111-1111-4111-8111-111111111111";
        let claude_active_id = "22222222-2222-4222-8222-222222222222";
        let claude_empty_file = claude
            .projects
            .join("proj")
            .join(format!("{claude_empty_id}.jsonl"));
        let claude_active_file = claude
            .projects
            .join("proj")
            .join(format!("{claude_active_id}.jsonl"));
        fs::write(&claude_empty_file, "").unwrap();
        fs::write(
            &claude_active_file,
            concat!(
                "{\"type\":\"user\",\"uuid\":\"u1\",\"sessionId\":\"22222222-2222-4222-8222-222222222222\",\"timestamp\":\"2026-10-29T00:00:00Z\",\"message\":{\"role\":\"user\",\"content\":\"hello\"}}\n",
                "{\"type\":\"assistant\",\"uuid\":\"a1\",\"parentUuid\":\"u1\",\"sessionId\":\"22222222-2222-4222-8222-222222222222\",\"timestamp\":\"2026-10-29T00:00:01Z\",\"message\":{\"role\":\"assistant\",\"stop_reason\":\"end_turn\",\"content\":[{\"type\":\"text\",\"text\":\"world\"}]}}\n"
            ),
        )
        .unwrap();

        // Seed 1 empty Grok session + 1 non-empty Grok session
        let grok_empty_id = "33333333-3333-4333-8333-333333333333";
        let grok_active_id = "44444444-4444-4444-8444-444444444444";
        let grok_empty_dir = grok.sessions().join("ws").join(grok_empty_id);
        let grok_active_dir = grok.sessions().join("ws").join(grok_active_id);
        fs::create_dir_all(&grok_empty_dir).unwrap();
        fs::create_dir_all(&grok_active_dir).unwrap();
        fs::write(
            grok_empty_dir.join("summary.json"),
            serde_json::json!({"info":{"id":grok_empty_id,"cwd":"/work"},"generated_title":"Empty Grok","num_messages":0}).to_string(),
        )
        .unwrap();
        fs::write(grok_empty_dir.join("updates.jsonl"), "").unwrap();
        fs::write(
            grok_active_dir.join("summary.json"),
            serde_json::json!({"info":{"id":grok_active_id,"cwd":"/work"},"generated_title":"Active Grok","num_messages":2}).to_string(),
        )
        .unwrap();
        fs::write(
            grok_active_dir.join("updates.jsonl"),
            "{\"timestamp\":\"2026-10-29T00:00:00Z\",\"sessionUpdate\":\"user_message_chunk\",\"messageId\":\"m1\",\"content\":{\"text\":\"hi\"}}\n",
        )
        .unwrap();

        let paths = super::UnifiedCleanupPaths {
            codex,
            antigravity,
            dsh,
            claude,
            grok,
        };
        let linux = PlatformPaths::linux_server();
        let macos = PlatformPaths::macos("/fixture/home");

        // Both Linux (`agy` CLI) and macOS support Antigravity archived cleanup.
        let linux_plan = super::dry_run_unified_with_capability(&linux, &paths).unwrap();
        let ag_linux = linux_plan
            .scopes
            .iter()
            .find(|s| s.scope == super::SCOPE_ANTIGRAVITY_ARCHIVED)
            .unwrap();
        assert!(ag_linux.supported);
        assert!(ag_linux.scanned);

        let mac_plan = super::dry_run_unified_with_capability(&macos, &paths).unwrap();
        assert_eq!(mac_plan.total_deletable, 2);
        let preview_targets: Vec<super::UnifiedCleanupTargetSelection> = mac_plan
            .scopes
            .iter()
            .flat_map(|s| {
                s.candidates
                    .iter()
                    .filter(|c| c.blocked_reason.is_none())
                    .map(move |c| super::UnifiedCleanupTargetSelection {
                        scope: s.scope.clone(),
                        id: c.id.clone(),
                        fingerprint: c.fingerprint.clone(),
                    })
            })
            .collect();
        assert_eq!(preview_targets.len(), 2);
        assert!(preview_targets
            .iter()
            .all(|t| t.fingerprint.as_deref().is_some_and(|fp| !fp.is_empty())));

        // Unconfirmed or targetless execute must fail closed.
        assert!(super::execute_unified_with_capability(
            &macos,
            &paths,
            super::UnifiedCleanupExecuteRequest {
                confirmed: false,
                scopes: vec![super::SCOPE_CLAUDE_EMPTY.into()],
                targets: preview_targets.clone(),
            },
        )
        .is_err());
        assert!(super::execute_unified_with_capability(
            &macos,
            &paths,
            super::UnifiedCleanupExecuteRequest {
                confirmed: true,
                scopes: vec![super::SCOPE_CLAUDE_EMPTY.into()],
                targets: Vec::new(),
            },
        )
        .is_err());

        // Out-of-bounds / path-traversal target ID must fail closed immediately.
        assert!(super::execute_unified_with_capability(
            &macos,
            &paths,
            super::UnifiedCleanupExecuteRequest {
                confirmed: true,
                scopes: vec![super::SCOPE_GROK_EMPTY.into()],
                targets: vec![super::UnifiedCleanupTargetSelection {
                    scope: super::SCOPE_GROK_EMPTY.into(),
                    id: "../escape".into(),
                    fingerprint: Some("forged".into()),
                }],
            },
        )
        .is_err());

        // Seed a brand-new empty Grok session AFTER preview: intersection must NOT delete it!
        let late_grok_id = "55555555-5555-4555-8555-555555555555";
        let late_grok_dir = paths.grok.sessions().join("ws").join(late_grok_id);
        fs::create_dir_all(&late_grok_dir).unwrap();
        fs::write(
            late_grok_dir.join("summary.json"),
            serde_json::json!({"info":{"id":late_grok_id,"cwd":"/work"},"generated_title":"Late Empty Grok","num_messages":0}).to_string(),
        )
        .unwrap();
        fs::write(late_grok_dir.join("updates.jsonl"), "").unwrap();

        // Fingerprint mismatch must skip without deleting the modified session.
        let mut stale_fp_targets = preview_targets.clone();
        for t in &mut stale_fp_targets {
            if t.scope == super::SCOPE_GROK_EMPTY {
                t.fingerprint = Some("stale-fingerprint".into());
            }
        }
        let skipped_res = super::execute_unified_with_capability(
            &macos,
            &paths,
            super::UnifiedCleanupExecuteRequest {
                confirmed: true,
                scopes: vec![super::SCOPE_GROK_EMPTY.into()],
                targets: stale_fp_targets,
            },
        )
        .unwrap();
        assert_eq!(skipped_res.deleted_count, 0);
        assert_eq!(skipped_res.skipped_count, 1);
        assert!(grok_empty_dir.exists());

        // A failed Claude target does not prevent the independent Grok deletion.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let claude_proj = paths.claude.projects.join("proj");
            fs::set_permissions(&claude_proj, fs::Permissions::from_mode(0o555)).unwrap();
            let stopped = super::execute_unified_with_capability(
                &macos,
                &paths,
                super::UnifiedCleanupExecuteRequest {
                    confirmed: true,
                    scopes: vec![
                        super::SCOPE_CLAUDE_EMPTY.into(),
                        super::SCOPE_GROK_EMPTY.into(),
                    ],
                    targets: preview_targets.clone(),
                },
            )
            .unwrap();
            fs::set_permissions(&claude_proj, fs::Permissions::from_mode(0o755)).unwrap();
            assert_eq!(stopped.failed_count, 1);
            assert!(
                !grok_empty_dir.exists(),
                "independent scope continues after earlier scope failure"
            );
        }

        // Recreate the same fixture and refresh only its fingerprint for a second independent request.
        fs::create_dir_all(&grok_empty_dir).unwrap();
        fs::write(
            grok_empty_dir.join("summary.json"),
            serde_json::json!({"info":{"id":grok_empty_id,"cwd":"/work"},"num_messages":0})
                .to_string(),
        )
        .unwrap();
        fs::write(grok_empty_dir.join("updates.jsonl"), "").unwrap();
        let refreshed = crate::grok::find_empty_inactive_sessions(&paths.grok).unwrap();
        let mut preview_targets = preview_targets;
        for target in &mut preview_targets {
            if target.id == grok_empty_id {
                target.fingerprint = Some(
                    refreshed
                        .iter()
                        .find(|(s, _, _)| s.id == grok_empty_id)
                        .unwrap()
                        .1
                        .clone(),
                );
            }
        }

        // Normal confirmed execution cleans ONLY previewed targets while preserving late & active sessions.
        let ok = super::execute_unified_with_capability(
            &macos,
            &paths,
            super::UnifiedCleanupExecuteRequest {
                confirmed: true,
                scopes: vec![
                    super::SCOPE_CLAUDE_EMPTY.into(),
                    super::SCOPE_GROK_EMPTY.into(),
                ],
                targets: preview_targets,
            },
        )
        .unwrap();
        assert_eq!(ok.deleted_count, 2);
        assert!(!claude_empty_file.exists());
        assert!(!grok_empty_dir.exists());
        assert!(
            late_grok_dir.exists(),
            "session created after preview must not be deleted"
        );
        assert!(claude_active_file.exists());
        assert!(grok_active_dir.exists());

        // A running Codex target fails while another confirmed target is deleted.
        fs::create_dir_all(paths.codex.sessions_dir()).unwrap();
        fs::write(paths.codex.session_index(), b"").unwrap();
        let conn = rusqlite::Connection::open(paths.codex.state_db()).unwrap();
        conn.execute_batch(
            "CREATE TABLE threads(
                id TEXT PRIMARY KEY,
                rollout_path TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                source TEXT NOT NULL,
                thread_source TEXT NOT NULL,
                parent_thread_id TEXT,
                has_user_event INTEGER NOT NULL,
                first_user_message TEXT NOT NULL,
                preview TEXT NOT NULL,
                cwd TEXT NOT NULL,
                title TEXT NOT NULL,
                archived INTEGER NOT NULL DEFAULT 0,
                archived_at INTEGER,
                status TEXT
            );",
        )
        .unwrap();
        let h1 = "66666666-6666-4666-8666-666666666666";
        let h2 = "77777777-7777-4777-8777-777777777777";
        let h_late = "88888888-8888-4888-8888-888888888888";
        for (id, status) in [(h1, "running"), (h2, "completed")] {
            let r = paths
                .codex
                .sessions_dir()
                .join(format!("rollout-{id}.jsonl"));
            fs::write(&r, "{}\n").unwrap();
            conn.execute(
                "INSERT INTO threads VALUES(?1, ?2, 1, 10, 'codex', 'subagent', NULL, 1, 'u', 'u', '/tmp', ?1, 0, NULL, ?3)",
                (id, r.display().to_string(), status),
            )
            .unwrap();
        }
        let codex_plan = crate::archive::hidden_delete_plan_with_work(
            &paths.codex,
            &crate::scan_work::ScanWork::default(),
        )
        .unwrap();
        let hidden_targets: Vec<super::UnifiedCleanupTargetSelection> = codex_plan
            .candidates
            .iter()
            .map(|c| super::UnifiedCleanupTargetSelection {
                scope: super::SCOPE_CODEX_HIDDEN.into(),
                id: c.id.clone(),
                fingerprint: c.fingerprint.clone(),
            })
            .collect();
        assert_eq!(hidden_targets.len(), 2);

        // Add unpreviewed hidden thread h_late after preview
        let r_late = paths
            .codex
            .sessions_dir()
            .join(format!("rollout-{h_late}.jsonl"));
        fs::write(&r_late, "{}\n").unwrap();
        conn.execute(
            "INSERT INTO threads VALUES(?1, ?2, 1, 20, 'codex', 'subagent', NULL, 1, 'u', 'u', '/tmp', ?1, 0, NULL, 'completed')",
            (h_late, r_late.display().to_string()),
        )
        .unwrap();

        // Running target h1 stops execution before h2 is deleted!
        let running_stop = super::execute_unified_with_capability(
            &macos,
            &paths,
            super::UnifiedCleanupExecuteRequest {
                confirmed: true,
                scopes: vec![super::SCOPE_CODEX_HIDDEN.into()],
                targets: hidden_targets.clone(),
            },
        )
        .unwrap();
        assert_eq!(running_stop.failed_count, 1);
        assert_eq!(running_stop.deleted_count, 1);
        assert!(!paths
            .codex
            .sessions_dir()
            .join(format!("rollout-{h2}.jsonl"))
            .exists());

        // Changed and already deleted targets are skipped; late archived threads remain untouched.
        conn.execute(
            "UPDATE threads SET status='completed', updated_at=99 WHERE id=?1",
            [h1],
        )
        .unwrap();
        let after_stop = super::execute_unified_with_capability(
            &macos,
            &paths,
            super::UnifiedCleanupExecuteRequest {
                confirmed: true,
                scopes: vec![super::SCOPE_CODEX_HIDDEN.into()],
                targets: hidden_targets,
            },
        )
        .unwrap();
        assert_eq!(after_stop.skipped_count, 2);
        assert_eq!(after_stop.deleted_count, 0);
        assert!(paths
            .codex
            .sessions_dir()
            .join(format!("rollout-{h1}.jsonl"))
            .exists());
        assert!(!paths
            .codex
            .sessions_dir()
            .join(format!("rollout-{h2}.jsonl"))
            .exists());
        assert!(r_late.exists());

        let _ = fs::remove_dir_all(&tmp);
    }
}

#[test]
fn grok_cleanup_reports_unknown_ownership_as_failure() {
    let root = std::env::temp_dir().join(format!(
        "agenthub-cleanup-ownership-{}",
        uuid::Uuid::new_v4()
    ));
    let paths = crate::grok::GrokPaths { home: root.clone() };
    let id = "11111111-1111-4111-8111-111111111111";
    let session = paths.sessions().join("fixture").join(id);
    std::fs::create_dir_all(&session).unwrap();
    std::fs::write(
        session.join("summary.json"),
        serde_json::json!({"info":{"id":id,"cwd":"/fixture"},"num_messages":0}).to_string(),
    )
    .unwrap();
    std::fs::write(session.join("updates.jsonl"), "").unwrap();
    std::fs::write(paths.home.join("active_sessions.json"), "{}").unwrap();
    let candidates = crate::grok::find_empty_inactive_sessions(&paths).unwrap();
    assert_eq!(candidates.len(), 1);
    std::fs::write(paths.home.join("active_sessions.json"), "invalid").unwrap();
    let target = UnifiedCleanupTargetSelection {
        scope: SCOPE_GROK_EMPTY.into(),
        id: id.into(),
        fingerprint: Some(candidates[0].1.clone()),
    };
    let mut result = UnifiedCleanupScopeResult::default();
    execute_grok_empty_scope(&paths, vec![target], &mut result, &mut Vec::new()).unwrap();
    assert_eq!(
        (
            result.failed_count,
            result.skipped_count,
            result.deleted_count
        ),
        (1, 0, 0)
    );
    assert!(session.exists());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn linux_unified_cleanup_supports_archived_antigravity_cli_sessions() {
    use std::fs;
    let tmp = std::env::temp_dir().join(format!(
        "agenthub-linux-ag-cleanup-{}",
        uuid::Uuid::new_v4()
    ));
    let ag = crate::antigravity::sessions::AntigravityPaths::offline_user_home(&tmp);
    let cli_home = ag.cli_home.as_ref().unwrap().clone();
    fs::create_dir_all(cli_home.join("conversations")).unwrap();
    fs::create_dir_all(cli_home.join("annotations")).unwrap();
    let archived_id = uuid::Uuid::new_v4().to_string();
    let active_id = uuid::Uuid::new_v4().to_string();
    for id in [&archived_id, &active_id] {
        fs::write(
            cli_home.join("conversations").join(format!("{id}.db")),
            b"cli-sqlite-fixture",
        )
        .unwrap();
    }
    let conn = rusqlite::Connection::open(cli_home.join("conversation_summaries.db")).unwrap();
    conn.execute_batch(
        "CREATE TABLE conversation_summaries (
            conversation_id TEXT PRIMARY KEY,
            title TEXT,
            preview TEXT,
            step_count INTEGER,
            last_modified_time TEXT,
            workspace_uris TEXT,
            parent_conversation_id TEXT,
            nesting_depth INTEGER
        );
        CREATE VIRTUAL TABLE conversations_fts USING fts5(
            conversation_id UNINDEXED,
            chunk_index UNINDEXED,
            archived UNINDEXED,
            title
        );",
    )
    .unwrap();
    conn.execute(
        "INSERT INTO conversation_summaries VALUES (?1, 'CLI 已归档', '', 2, '2026-10-29T00:00:00Z', '', '', 0), (?2, 'CLI 活跃', '', 1, '2026-10-29T00:01:00Z', '', '', 0)",
        rusqlite::params![archived_id, active_id],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO conversations_fts VALUES (?1, -1, 1, 'CLI 已归档'), (?2, -1, 0, 'CLI 活跃')",
        rusqlite::params![archived_id, active_id],
    )
    .unwrap();
    drop(conn);

    let paths = UnifiedCleanupPaths {
        codex: crate::codex::CodexPaths::new(tmp.join("codex")),
        antigravity: ag,
        dsh: crate::dsh::DshPaths {
            home: tmp.join("dsh"),
        },
        claude: crate::claude::ClaudePaths {
            config: tmp.join("claude"),
            projects: tmp.join("claude/projects"),
        },
        grok: crate::grok::GrokPaths {
            home: tmp.join("grok"),
        },
    };
    let linux = crate::platform::PlatformPaths::linux_server();
    let plan = dry_run_unified_with_capability(&linux, &paths).unwrap();
    let ag_scope = plan
        .scopes
        .iter()
        .find(|s| s.scope == SCOPE_ANTIGRAVITY_ARCHIVED)
        .unwrap();
    assert!(ag_scope.supported && ag_scope.scanned);
    assert_eq!(ag_scope.candidates.len(), 1);
    assert_eq!(ag_scope.candidates[0].id, archived_id);

    let exec = execute_unified_with_capability(
        &linux,
        &paths,
        UnifiedCleanupExecuteRequest {
            confirmed: true,
            scopes: vec![SCOPE_ANTIGRAVITY_ARCHIVED.into()],
            targets: vec![UnifiedCleanupTargetSelection {
                scope: SCOPE_ANTIGRAVITY_ARCHIVED.into(),
                id: archived_id.clone(),
                fingerprint: ag_scope.candidates[0].fingerprint.clone(),
            }],
        },
    )
    .unwrap();
    assert_eq!(exec.deleted_count, 1);
    assert_eq!(exec.failed_count, 0);
    assert!(!cli_home
        .join("conversations")
        .join(format!("{archived_id}.db"))
        .exists());
    assert!(cli_home
        .join("conversations")
        .join(format!("{active_id}.db"))
        .exists());
    let _ = fs::remove_dir_all(&tmp);
}

#[cfg(all(test, unix))]
#[test]
fn antigravity_unified_cleanup_includes_verified_orphan_subagents_and_skips_locked() {
    use std::fs::{self, OpenOptions};
    use std::os::unix::io::AsRawFd;

    let tmp =
        std::env::temp_dir().join(format!("agenthub-ag-orphan-clean-{}", uuid::Uuid::new_v4()));
    let ag_home = tmp.join("ag");
    for d in ["conversations", "brain", "presence"] {
        fs::create_dir_all(ag_home.join(d)).unwrap();
    }
    let deleted_root = uuid::Uuid::new_v4().to_string();
    let orphan_l1 = uuid::Uuid::new_v4().to_string();
    let orphan_l2 = uuid::Uuid::new_v4().to_string();
    let locked_orphan = uuid::Uuid::new_v4().to_string();
    let live_root = uuid::Uuid::new_v4().to_string();
    let live_child = uuid::Uuid::new_v4().to_string();

    for cid in [
        &orphan_l1,
        &orphan_l2,
        &locked_orphan,
        &live_root,
        &live_child,
    ] {
        fs::write(
            ag_home.join("conversations").join(format!("{cid}.db")),
            b"orphan-bytes",
        )
        .unwrap();
    }
    let lock_file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(
            ag_home
                .join("presence")
                .join(format!("{locked_orphan}.lock")),
        )
        .unwrap();
    assert_eq!(
        unsafe { libc::flock(lock_file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
        0
    );

    let conn = rusqlite::Connection::open(ag_home.join("conversation_summaries.db")).unwrap();
    conn.execute_batch(
        "CREATE TABLE conversation_summaries (
            conversation_id TEXT PRIMARY KEY,
            title TEXT, preview TEXT, step_count INTEGER,
            last_modified_time TEXT, workspace_uris TEXT,
            status TEXT, parent_conversation_id TEXT,
            nesting_depth INTEGER, not_fully_idle NUMERIC
        );
        CREATE TABLE conversation_parents (
            conversation_id TEXT PRIMARY KEY, parent_id TEXT NOT NULL
        );",
    )
    .unwrap();
    conn.execute(
        "INSERT INTO conversation_summaries VALUES \
         (?1, '孤立一层', '', 1, '2026-11-24T00:00:00Z', '', 'CASCADE_RUN_STATUS_IDLE', ?2, 1, 0), \
         (?3, '孤立二层', '', 1, '2026-11-24T00:00:01Z', '', 'CASCADE_RUN_STATUS_IDLE', ?1, 2, 0), \
         (?4, '占用中孤立子代理', '', 1, '2026-11-24T00:00:02Z', '', 'CASCADE_RUN_STATUS_IDLE', ?2, 1, 0), \
         (?5, '正常根会话', '', 1, '2026-11-24T00:00:03Z', '', 'CASCADE_RUN_STATUS_IDLE', '', 0, 0), \
         (?6, '正常子会话', '', 1, '2026-11-24T00:00:04Z', '', 'CASCADE_RUN_STATUS_IDLE', ?5, 1, 0)",
        rusqlite::params![orphan_l1, deleted_root, orphan_l2, locked_orphan, live_root, live_child],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO conversation_parents VALUES (?1, ?2), (?3, ?1), (?4, ?2), (?5, ?6)",
        rusqlite::params![
            orphan_l1,
            deleted_root,
            orphan_l2,
            locked_orphan,
            live_child,
            live_root
        ],
    )
    .unwrap();
    drop(conn);

    let paths = UnifiedCleanupPaths {
        codex: crate::codex::CodexPaths::new(tmp.join("codex")),
        antigravity: crate::antigravity::sessions::AntigravityPaths::offline(ag_home.clone()),
        dsh: crate::dsh::DshPaths {
            home: tmp.join("dsh"),
        },
        claude: crate::claude::ClaudePaths {
            config: tmp.join("claude"),
            projects: tmp.join("claude/projects"),
        },
        grok: crate::grok::GrokPaths {
            home: tmp.join("grok"),
        },
    };
    let desktop = crate::platform::PlatformPaths::macos(tmp.clone());
    let plan = dry_run_unified_with_capability(&desktop, &paths).unwrap();
    let ag_scope = plan
        .scopes
        .iter()
        .find(|s| s.scope == SCOPE_ANTIGRAVITY_ARCHIVED)
        .unwrap();
    assert_eq!(ag_scope.label, "Antigravity 已归档与孤立子会话");
    // Only orphan_l1 (cascading orphan_l2) is eligible; locked_orphan and live_child are excluded!
    assert_eq!(ag_scope.candidates.len(), 1);
    assert_eq!(ag_scope.candidates[0].id, orphan_l1);
    assert!(ag_scope.candidates[0].title.contains("孤立子会话 ←"));
    assert_eq!(
        ag_scope.candidates[0].estimated_bytes,
        (b"orphan-bytes".len() * 2) as u64
    );

    let exec = execute_unified_with_capability(
        &desktop,
        &paths,
        UnifiedCleanupExecuteRequest {
            confirmed: true,
            scopes: vec![SCOPE_ANTIGRAVITY_ARCHIVED.into()],
            targets: vec![UnifiedCleanupTargetSelection {
                scope: SCOPE_ANTIGRAVITY_ARCHIVED.into(),
                id: orphan_l1.clone(),
                fingerprint: ag_scope.candidates[0].fingerprint.clone(),
            }],
        },
    )
    .unwrap();
    assert_eq!(exec.deleted_count, 1);
    assert_eq!(exec.failed_count, 0);
    assert!(!ag_home
        .join("conversations")
        .join(format!("{orphan_l1}.db"))
        .exists());
    assert!(!ag_home
        .join("conversations")
        .join(format!("{orphan_l2}.db"))
        .exists());
    assert!(ag_home
        .join("conversations")
        .join(format!("{locked_orphan}.db"))
        .exists());
    assert!(ag_home
        .join("conversations")
        .join(format!("{live_child}.db"))
        .exists());

    unsafe {
        let _ = libc::flock(lock_file.as_raw_fd(), libc::LOCK_UN);
    }
    let _ = fs::remove_dir_all(&tmp);
}
