use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{
    archive,
    codex::CodexPaths,
    platform::PlatformPaths,
    services::commands,
    services::system::{require_capability, Capability},
};

pub use crate::archive::{
    ArchiveDeletePlan, ArchiveDeleteResult, HiddenThreadDeletePlan, HiddenThreadDeleteResult,
};

pub const ARCHIVE_DELETE_CONFIRMATION_MESSAGE: &str = "archive deletion must be confirmed";
pub const HIDDEN_DELETE_CONFIRMATION_MESSAGE: &str = "hidden thread deletion must be confirmed";
pub const UNIFIED_CLEANUP_CONFIRMATION_MESSAGE: &str = "unified thread cleanup must be confirmed";

pub const SCOPE_CODEX_ARCHIVED: &str = "codex_archived";
pub const SCOPE_CODEX_HIDDEN: &str = "codex_hidden";
pub const SCOPE_ANTIGRAVITY_ARCHIVED: &str = "antigravity_archived";
pub const SCOPE_DSH_ARCHIVED: &str = "dsh_archived";
pub const SCOPE_CLAUDE_EMPTY: &str = "claude_empty";
pub const SCOPE_GROK_EMPTY: &str = "grok_empty";

pub const ALL_UNIFIED_SCOPES: &[&str] = &[
    SCOPE_CODEX_ARCHIVED,
    SCOPE_CODEX_HIDDEN,
    SCOPE_ANTIGRAVITY_ARCHIVED,
    SCOPE_DSH_ARCHIVED,
    SCOPE_CLAUDE_EMPTY,
    SCOPE_GROK_EMPTY,
];

#[derive(Debug, Clone)]
pub struct UnifiedCleanupPaths {
    pub codex: CodexPaths,
    pub antigravity: crate::antigravity::sessions::AntigravityPaths,
    pub dsh: crate::dsh::DshPaths,
    pub claude: crate::claude::ClaudePaths,
    pub grok: crate::grok::GrokPaths,
}

impl UnifiedCleanupPaths {
    pub fn default_for_codex(codex: CodexPaths) -> Self {
        Self {
            codex,
            antigravity: crate::antigravity::sessions::AntigravityPaths::default_for_user(),
            dsh: crate::dsh::DshPaths::default_for_user(),
            claude: crate::claude::ClaudePaths::default_for_user(),
            grok: crate::grok::GrokPaths::default_for_user(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnifiedCleanupDryRunRequest {
    #[serde(default)]
    pub scopes: Vec<String>,
    #[serde(default)]
    pub include_codex_hidden: bool,
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
    pub stopped_early: bool,
    pub scopes: Vec<UnifiedCleanupScopeResult>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CleanupAction {
    #[serde(rename = "archiveDeleteDryRun", alias = "archive-delete-dry-run")]
    ArchiveDeleteDryRun,
    #[serde(rename = "archiveDeleteExecute", alias = "archive-delete-execute")]
    ArchiveDeleteExecute,
    #[serde(rename = "hiddenDeleteDryRun", alias = "hidden-delete-dry-run")]
    HiddenDeleteDryRun,
    #[serde(rename = "hiddenDeleteExecute", alias = "hidden-delete-execute")]
    HiddenDeleteExecute,
}

impl CleanupAction {
    pub fn as_rpc_action(self) -> &'static str {
        match self {
            Self::ArchiveDeleteDryRun => commands::CLEANUP_ARCHIVE_DRY_RUN,
            Self::ArchiveDeleteExecute => commands::CLEANUP_ARCHIVE_EXECUTE,
            Self::HiddenDeleteDryRun => commands::CLEANUP_HIDDEN_DRY_RUN,
            Self::HiddenDeleteExecute => commands::CLEANUP_HIDDEN_EXECUTE,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CleanupTarget {
    Archived,
    Hidden,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupActionPlan {
    pub required_capability: Capability,
    pub action: CleanupAction,
    pub command: String,
    pub target: CleanupTarget,
    pub execute: bool,
    pub requires_confirmation: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupExecuteRequest {
    pub confirmed: bool,
    #[serde(default, alias = "expectedCount", alias = "expected_count")]
    pub expected_count: Option<u64>,
    #[serde(default)]
    pub candidates: Option<Vec<archive::HiddenThreadSelection>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupConfirmationPlan {
    pub confirmed: bool,
    pub expected_count: Option<u64>,
    pub payload: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CleanupOperationKind {
    DryRun,
    Execute,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupOperationPlan {
    pub required_capability: Capability,
    pub action: CleanupAction,
    pub command: String,
    pub target: CleanupTarget,
    pub operation: CleanupOperationKind,
    pub execute: bool,
    pub requires_confirmation: bool,
    pub requires_prior_dry_run: bool,
    pub requires_expected_count: bool,
    pub confirmation_message: Option<String>,
    pub confirmation: CleanupConfirmationPlan,
}

pub fn plan_cleanup_action(
    platform: &PlatformPaths,
    action: CleanupAction,
) -> Result<CleanupActionPlan> {
    require_capability(platform, Capability::ThreadCleanup)?;
    let (target, execute) = match action {
        CleanupAction::ArchiveDeleteDryRun => (CleanupTarget::Archived, false),
        CleanupAction::ArchiveDeleteExecute => (CleanupTarget::Archived, true),
        CleanupAction::HiddenDeleteDryRun => (CleanupTarget::Hidden, false),
        CleanupAction::HiddenDeleteExecute => (CleanupTarget::Hidden, true),
    };
    Ok(CleanupActionPlan {
        required_capability: Capability::ThreadCleanup,
        action,
        command: action.as_rpc_action().to_string(),
        target,
        execute,
        requires_confirmation: execute,
    })
}

pub fn plan_cleanup_operation(
    platform: &PlatformPaths,
    target: CleanupTarget,
    operation: CleanupOperationKind,
) -> Result<CleanupOperationPlan> {
    plan_cleanup_operation_with_confirmation(
        platform,
        target,
        operation,
        CleanupExecuteRequest::default(),
    )
}

pub fn plan_cleanup_execute_operation(
    platform: &PlatformPaths,
    target: CleanupTarget,
    request: CleanupExecuteRequest,
) -> Result<CleanupOperationPlan> {
    plan_cleanup_operation_with_confirmation(
        platform,
        target,
        CleanupOperationKind::Execute,
        request,
    )
}

pub fn plan_cleanup_operation_with_confirmation(
    platform: &PlatformPaths,
    target: CleanupTarget,
    operation: CleanupOperationKind,
    confirmation: CleanupExecuteRequest,
) -> Result<CleanupOperationPlan> {
    if matches!(operation, CleanupOperationKind::Execute) {
        validate_cleanup_execute_confirmation(target, &confirmation)?;
    }

    let action = match (target, operation) {
        (CleanupTarget::Archived, CleanupOperationKind::DryRun) => {
            CleanupAction::ArchiveDeleteDryRun
        }
        (CleanupTarget::Archived, CleanupOperationKind::Execute) => {
            CleanupAction::ArchiveDeleteExecute
        }
        (CleanupTarget::Hidden, CleanupOperationKind::DryRun) => CleanupAction::HiddenDeleteDryRun,
        (CleanupTarget::Hidden, CleanupOperationKind::Execute) => {
            CleanupAction::HiddenDeleteExecute
        }
    };
    let action_plan = plan_cleanup_action(platform, action)?;
    Ok(CleanupOperationPlan {
        required_capability: action_plan.required_capability,
        action: action_plan.action,
        command: action_plan.command,
        target: action_plan.target,
        operation,
        execute: action_plan.execute,
        requires_confirmation: action_plan.requires_confirmation,
        requires_prior_dry_run: action_plan.execute,
        requires_expected_count: false,
        confirmation_message: action_plan
            .execute
            .then(|| cleanup_confirmation_message(target).to_string()),
        confirmation: cleanup_confirmation_plan(confirmation),
    })
}

pub fn cleanup_confirmation_plan(request: CleanupExecuteRequest) -> CleanupConfirmationPlan {
    CleanupConfirmationPlan {
        confirmed: request.confirmed,
        expected_count: request.expected_count,
        payload: json!({
            "confirmed": request.confirmed,
            "expectedCount": request.expected_count,
            "candidates": request.candidates,
        }),
    }
}

pub fn cleanup_confirmation_message(target: CleanupTarget) -> &'static str {
    match target {
        CleanupTarget::Archived => ARCHIVE_DELETE_CONFIRMATION_MESSAGE,
        CleanupTarget::Hidden => HIDDEN_DELETE_CONFIRMATION_MESSAGE,
    }
}

fn validate_cleanup_execute_confirmation(
    target: CleanupTarget,
    request: &CleanupExecuteRequest,
) -> Result<()> {
    if !request.confirmed {
        anyhow::bail!(cleanup_confirmation_message(target));
    }
    if request.candidates.is_none() {
        anyhow::bail!("清理需要明确的线程列表");
    }
    Ok(())
}

pub fn hidden_candidates(
    plan: &CleanupOperationPlan,
) -> Result<Vec<archive::HiddenThreadSelection>> {
    let candidates = plan
        .confirmation
        .payload
        .get("candidates")
        .filter(|value| !value.is_null())
        .ok_or_else(|| anyhow::anyhow!("隐藏清理缺少预览候选"))?;
    Ok(serde_json::from_value(candidates.clone())?)
}

pub fn dry_run_archived_with_capability(
    platform: &PlatformPaths,
    paths: &CodexPaths,
) -> Result<ArchiveDeletePlan> {
    plan_cleanup_action(platform, CleanupAction::ArchiveDeleteDryRun)?;
    archive::plan_delete_archived(paths)
}

pub fn execute_archived_with_capability(
    platform: &PlatformPaths,
    paths: &CodexPaths,
    candidates: &[archive::HiddenThreadSelection],
) -> Result<ArchiveDeleteResult> {
    plan_cleanup_action(platform, CleanupAction::ArchiveDeleteExecute)?;
    archive::execute_delete_archived(paths, candidates)
}

pub fn dry_run_hidden_with_capability(
    platform: &PlatformPaths,
    paths: &CodexPaths,
) -> Result<HiddenThreadDeletePlan> {
    plan_cleanup_action(platform, CleanupAction::HiddenDeleteDryRun)?;
    archive::plan_delete_hidden(paths)
}

pub fn execute_hidden_with_capability(
    platform: &PlatformPaths,
    paths: &CodexPaths,
    candidates: &[archive::HiddenThreadSelection],
) -> Result<HiddenThreadDeleteResult> {
    plan_cleanup_action(platform, CleanupAction::HiddenDeleteExecute)?;
    archive::execute_delete_hidden(paths, candidates)
}

fn scope_meta(scope: &str) -> Result<(&'static str, &'static str)> {
    match scope {
        SCOPE_CODEX_ARCHIVED => Ok(("codex", "Codex 已归档线程")),
        SCOPE_CODEX_HIDDEN => Ok(("codex", "Codex 隐藏线程")),
        SCOPE_ANTIGRAVITY_ARCHIVED => Ok(("antigravity", "Antigravity 已归档线程")),
        SCOPE_DSH_ARCHIVED => Ok(("dsh", "DeepSeek Harness 已归档线程")),
        SCOPE_CLAUDE_EMPTY => Ok(("claude", "Claude Code 空残余会话")),
        SCOPE_GROK_EMPTY => Ok(("grok", "Grok Build 空残余会话")),
        _ => anyhow::bail!("未知的清理范围: {scope}"),
    }
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
    platform: &PlatformPaths,
    paths: &UnifiedCleanupPaths,
    scope: &str,
    scan_hidden: bool,
) -> Result<UnifiedCleanupScopePlan> {
    let (provider, label) = scope_meta(scope)?;
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
    match scope {
        SCOPE_CODEX_ARCHIVED => match archive::plan_delete_archived(&paths.codex) {
            Ok(plan) => {
                let bytes_map = crate::selected_codex::tree::estimate_bytes_batch(
                    &paths.codex,
                    &plan.archived_ids,
                );
                for id in plan.archived_ids {
                    let bytes = bytes_map.get(&id).copied().unwrap_or(0);
                    let fp = archive::codex_thread_fingerprint(&paths.codex, &id)
                        .ok()
                        .flatten();
                    push_candidate(&mut out, id.clone(), id, bytes, fp, None);
                }
            }
            Err(err) => out.issues.push(err.to_string()),
        },
        SCOPE_CODEX_HIDDEN => {
            if !scan_hidden {
                out.scanned = false;
                return Ok(out);
            }
            match archive::plan_delete_hidden_candidates(&paths.codex) {
                Ok(plan) => {
                    let bytes_map = crate::selected_codex::tree::estimate_bytes_batch(
                        &paths.codex,
                        &plan.hidden_ids,
                    );
                    for c in plan.candidates {
                        let bytes = bytes_map.get(&c.id).copied().unwrap_or(0);
                        let blocked = (!c.allowed)
                            .then(|| c.reason.unwrap_or_else(|| "当前线程暂不可清理".into()));
                        push_candidate(&mut out, c.id, c.title, bytes, c.fingerprint, blocked);
                    }
                }
                Err(err) => out.issues.push(err.to_string()),
            }
        }
        SCOPE_ANTIGRAVITY_ARCHIVED => {
            if platform.kind != crate::platform::PlatformKind::Macos {
                out.supported = false;
                out.scanned = false;
                out.unavailable_reason = Some("仅本机 macOS 支持 Antigravity 会话清理".into());
                return Ok(out);
            }
            match crate::antigravity::sessions::list_sessions(
                &paths.antigravity,
                &crate::provider_sessions::ProviderListRequest {
                    archived: Some(true),
                    limit: Some(1000),
                    q: None,
                },
            ) {
                Ok(sessions) => {
                    for summary in sessions.into_iter().filter(|s| s.archived) {
                        match crate::antigravity::sessions::preview_batch_item(
                            &paths.antigravity,
                            &summary.session_key,
                            crate::services::sessions::SessionOperation::Delete,
                            true,
                        ) {
                            Ok(preview) => {
                                let blocked = (!preview.allowed).then(|| {
                                    preview.reason.unwrap_or_else(|| "当前无法安全清理".into())
                                });
                                push_candidate(
                                    &mut out,
                                    summary.session_key,
                                    summary.title,
                                    preview.bytes,
                                    preview.fingerprint,
                                    blocked,
                                );
                            }
                            Err(err) => {
                                let bytes = summary
                                    .storage_size
                                    .as_ref()
                                    .and_then(|s| s.bytes)
                                    .unwrap_or(0);
                                push_candidate(
                                    &mut out,
                                    summary.session_key,
                                    summary.title,
                                    bytes,
                                    None,
                                    Some(err.to_string()),
                                );
                            }
                        }
                    }
                }
                Err(err) => out.issues.push(err.to_string()),
            }
        }
        SCOPE_DSH_ARCHIVED => match crate::dsh::preview_with_bytes(&paths.dsh) {
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
        SCOPE_CLAUDE_EMPTY => match crate::claude::find_empty_inactive_sessions(&paths.claude) {
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
        },
        SCOPE_GROK_EMPTY => match crate::grok::find_empty_inactive_sessions(&paths.grok) {
            Ok(items) => {
                for (summary, fp, bytes) in items {
                    push_candidate(&mut out, summary.id, summary.title, bytes, Some(fp), None);
                }
            }
            Err(err) => out.issues.push(err.to_string()),
        },
        _ => anyhow::bail!("未知的清理范围: {scope}"),
    }
    Ok(out)
}

pub fn dry_run_unified_with_capability(
    platform: &PlatformPaths,
    paths: &UnifiedCleanupPaths,
    request: UnifiedCleanupDryRunRequest,
) -> Result<UnifiedCleanupPlan> {
    require_capability(platform, Capability::ThreadCleanup)?;
    let requested: Vec<&str> = if request.scopes.is_empty() {
        ALL_UNIFIED_SCOPES.to_vec()
    } else {
        for s in &request.scopes {
            scope_meta(s)?;
        }
        ALL_UNIFIED_SCOPES
            .iter()
            .copied()
            .filter(|s| request.scopes.iter().any(|req| req == s))
            .collect()
    };
    let explicit_hidden = request.scopes.iter().any(|s| s == SCOPE_CODEX_HIDDEN);
    let scan_hidden = request.include_codex_hidden || explicit_hidden;
    let mut plan = UnifiedCleanupPlan::default();
    for scope in requested {
        let item = dry_run_single_scope(platform, paths, scope, scan_hidden)?;
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

fn run_scope_targets(
    scope_targets: Vec<UnifiedCleanupTargetSelection>,
    scope_res: &mut UnifiedCleanupScopeResult,
    stopped_early: &mut bool,
    mut step: impl FnMut(&str, &str) -> Result<TargetStepOutcome>,
) -> Result<()> {
    for target in scope_targets {
        let Some(req_fp) = target.fingerprint.as_deref() else {
            scope_res.skipped_count += 1;
            scope_res
                .issues
                .push(format!("{}: 缺少预览指纹，已跳过", target.id));
            continue;
        };
        match step(&target.id, req_fp)? {
            TargetStepOutcome::Deleted(bytes) => {
                scope_res.deleted_count += 1;
                scope_res.deleted_bytes = scope_res.deleted_bytes.saturating_add(bytes);
            }
            TargetStepOutcome::Skipped(msg) => {
                scope_res.skipped_count += 1;
                scope_res.issues.push(format!("{}: {msg}", target.id));
            }
            TargetStepOutcome::Failed(msg, residuals) => {
                scope_res.failed_count += 1;
                if let Some(m) = msg {
                    scope_res.issues.push(format!("{}: {m}", target.id));
                }
                scope_res.residuals.extend(residuals);
                *stopped_early = true;
                break;
            }
        }
    }
    Ok(())
}

fn execute_codex_scope(
    codex: &CodexPaths,
    scope_targets: Vec<UnifiedCleanupTargetSelection>,
    is_hidden: bool,
    scope_res: &mut UnifiedCleanupScopeResult,
    stopped_early: &mut bool,
) -> Result<()> {
    let db_path = codex.state_db();
    let conn =
        rusqlite::Connection::open_with_flags(&db_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|e| anyhow::anyhow!("open {}: {e}", db_path.display()))?;
    let thread_cols = archive::table_columns(&conn, "threads")?;
    let edge_cols = archive::table_columns(&conn, "thread_spawn_edges")?;
    let has_spawn_edges =
        edge_cols.contains("parent_thread_id") && edge_cols.contains("child_thread_id");
    let edges = crate::selected_codex::tree::spawn_edges(codex)?;

    run_scope_targets(scope_targets, scope_res, stopped_early, |id, req_fp| {
        let verified_fp = if is_hidden {
            archive::verify_hidden_target_fingerprint(&conn, &thread_cols, has_spawn_edges, id)?
        } else {
            archive::verify_archived_target_fingerprint(&conn, &thread_cols, has_spawn_edges, id)?
        };
        let is_residual = verified_fp.is_none()
            && crate::selected_codex::tree::is_recorded_delete_residual(codex, id)
            && crate::selected_codex::tree::has_orphan_artifacts(codex, id);
        match verified_fp {
            Some(cur_fp) if cur_fp == req_fp => {}
            Some(_) => {
                return Ok(TargetStepOutcome::Skipped(
                    "线程状态或指纹已变化，已跳过".into(),
                ))
            }
            None if is_residual => {}
            None => {
                return Ok(TargetStepOutcome::Skipped(
                    "目标不在当前允许集合中，已跳过".into(),
                ))
            }
        }
        let tree_ids =
            match crate::selected_codex::tree::descendants_from_edges(&edges, &[id.to_owned()]) {
                Ok(ids) => ids,
                Err(err) => {
                    return Ok(TargetStepOutcome::Failed(Some(err.to_string()), Vec::new()))
                }
            };
        if let Err(err) =
            crate::selected_codex::tree::ensure_ids_not_running(codex, &tree_ids, Some(&conn))
        {
            return Ok(TargetStepOutcome::Failed(Some(err.to_string()), Vec::new()));
        }
        Ok(
            match crate::selected_codex::tree::execute_ids(codex, &[id.to_owned()]) {
                Ok(out) if out.error.is_none() && out.residuals.is_empty() => {
                    TargetStepOutcome::Deleted(out.deleted_bytes)
                }
                Ok(out) => TargetStepOutcome::Failed(out.error, out.residuals),
                Err(err) => TargetStepOutcome::Failed(Some(err.to_string()), Vec::new()),
            },
        )
    })
}

fn execute_antigravity_archived_scope(
    antigravity: &crate::antigravity::sessions::AntigravityPaths,
    scope_targets: Vec<UnifiedCleanupTargetSelection>,
    scope_res: &mut UnifiedCleanupScopeResult,
    stopped_early: &mut bool,
) -> Result<()> {
    let allowed_keys: std::collections::HashSet<String> =
        crate::antigravity::sessions::list_sessions(
            antigravity,
            &crate::provider_sessions::ProviderListRequest {
                archived: Some(true),
                limit: Some(1000),
                q: None,
            },
        )?
        .into_iter()
        .filter(|s| s.archived)
        .map(|s| s.session_key)
        .collect();

    run_scope_targets(scope_targets, scope_res, stopped_early, |id, req_fp| {
        if !allowed_keys.contains(id) {
            return Ok(TargetStepOutcome::Skipped(
                "目标不在当前已归档允许集合中，已跳过".into(),
            ));
        }
        let preview = match crate::antigravity::sessions::preview_batch_item(
            antigravity,
            id,
            crate::services::sessions::SessionOperation::Delete,
            true,
        ) {
            Ok(p) if p.allowed => p,
            Ok(p) => return Ok(TargetStepOutcome::Failed(p.reason, Vec::new())),
            Err(err) => return Ok(TargetStepOutcome::Failed(Some(err.to_string()), Vec::new())),
        };
        if preview.fingerprint.as_deref() != Some(req_fp) {
            return Ok(TargetStepOutcome::Skipped(
                "会话状态或指纹已变化，已跳过".into(),
            ));
        }
        if !crate::antigravity::sessions::session_summary(antigravity, id)?.archived {
            return Ok(TargetStepOutcome::Skipped("会话已恢复为非归档状态".into()));
        }
        Ok(
            match crate::antigravity::sessions::execute_batch_item(
                antigravity,
                id,
                crate::services::sessions::SessionOperation::Delete,
                &preview.descendants,
                true,
            ) {
                Ok(item) if item.status == "succeeded" && item.residuals.is_empty() => {
                    TargetStepOutcome::Deleted(preview.bytes)
                }
                Ok(item) => TargetStepOutcome::Failed(item.message, item.residuals),
                Err(err) => TargetStepOutcome::Failed(Some(err.to_string()), Vec::new()),
            },
        )
    })
}

fn execute_dsh_archived_scope(
    dsh: &crate::dsh::DshPaths,
    scope_targets: Vec<UnifiedCleanupTargetSelection>,
    scope_res: &mut UnifiedCleanupScopeResult,
    stopped_early: &mut bool,
) -> Result<()> {
    let recomputed = crate::dsh::preview(dsh)?;
    scope_res.issues.extend(recomputed.issues);
    let allowed_map: std::collections::BTreeMap<&str, &crate::dsh::DshArchiveCandidate> =
        recomputed
            .candidates
            .iter()
            .map(|c| (c.id.as_str(), c))
            .collect();
    let mut verified_targets = Vec::new();
    run_scope_targets(scope_targets, scope_res, stopped_early, |id, req_fp| {
        let Some(c) = allowed_map.get(id) else {
            return Ok(TargetStepOutcome::Skipped(
                "目标不在当前已归档允许集合中，已跳过".into(),
            ));
        };
        if let Some(reason) = &c.reason {
            return Ok(TargetStepOutcome::Failed(Some(reason.clone()), Vec::new()));
        }
        if c.archive_id != req_fp {
            return Ok(TargetStepOutcome::Skipped("归档指纹已变化，已跳过".into()));
        }
        verified_targets.push(crate::dsh::DshArchiveSelection {
            id: id.to_owned(),
            archive_id: req_fp.to_owned(),
        });
        Ok(TargetStepOutcome::Deleted(0))
    })?;
    scope_res.deleted_count = 0;
    if !*stopped_early && !verified_targets.is_empty() {
        match crate::dsh::execute(
            dsh,
            crate::dsh::DshArchiveDeleteRequest {
                confirmed: true,
                targets: verified_targets,
            },
        ) {
            Ok(res) => {
                scope_res.deleted_bytes = res.deleted_bytes;
                for item in res.items {
                    match item.status.as_str() {
                        "deleted" => scope_res.deleted_count += 1,
                        "skipped" => {
                            scope_res.skipped_count += 1;
                            if let Some(r) = item.reason {
                                scope_res.issues.push(format!("{}: {r}", item.id));
                            }
                        }
                        _ => {
                            scope_res.failed_count += 1;
                            if let Some(r) = item.reason {
                                scope_res.issues.push(format!("{}: {r}", item.id));
                            }
                            scope_res.residuals.extend(item.residuals);
                        }
                    }
                }
                if scope_res.failed_count > 0 || !scope_res.residuals.is_empty() {
                    *stopped_early = true;
                }
            }
            Err(err) => {
                scope_res.failed_count += 1;
                scope_res.issues.push(err.to_string());
                *stopped_early = true;
            }
        }
    }
    Ok(())
}

fn execute_empty_inactive_scope(
    scope_targets: Vec<UnifiedCleanupTargetSelection>,
    allowed_map: &std::collections::BTreeMap<String, String>,
    check_running_block: impl Fn(&str) -> Option<String>,
    delete_item: impl Fn(&str, &str) -> Result<(bool, u64, Vec<String>)>,
    scope_res: &mut UnifiedCleanupScopeResult,
    stopped_early: &mut bool,
) -> Result<()> {
    run_scope_targets(scope_targets, scope_res, stopped_early, |id, req_fp| {
        let Some(cur_fp) = allowed_map.get(id) else {
            if let Some(block_reason) = check_running_block(id) {
                return Ok(TargetStepOutcome::Failed(Some(block_reason), Vec::new()));
            }
            return Ok(TargetStepOutcome::Skipped(
                "会话不再属于空残余允许集合，已跳过".into(),
            ));
        };
        if cur_fp != req_fp {
            return Ok(TargetStepOutcome::Skipped("会话指纹已变化，已跳过".into()));
        }
        Ok(match delete_item(id, req_fp) {
            Ok((true, bytes, residuals)) if residuals.is_empty() => {
                TargetStepOutcome::Deleted(bytes)
            }
            Ok((_deleted, _bytes, residuals)) => TargetStepOutcome::Failed(None, residuals),
            Err(err) => TargetStepOutcome::Failed(Some(err.to_string()), Vec::new()),
        })
    })
}

pub fn execute_unified_with_capability(
    platform: &PlatformPaths,
    paths: &UnifiedCleanupPaths,
    request: UnifiedCleanupExecuteRequest,
) -> Result<UnifiedCleanupResult> {
    require_capability(platform, Capability::ThreadCleanup)?;
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
        .copied()
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
    for scope in ordered_scopes {
        let (provider, label) = scope_meta(scope)?;
        let mut scope_res = UnifiedCleanupScopeResult {
            scope: scope.to_owned(),
            provider: provider.to_owned(),
            label: label.to_owned(),
            ..Default::default()
        };
        let scope_targets = targets_by_scope.remove(scope).unwrap_or_default();
        if total.stopped_early {
            scope_res
                .issues
                .push("前序清理步骤出现失败或残留，已停止后续清理".into());
        } else if !scope_targets.is_empty() {
            match scope {
                SCOPE_CODEX_ARCHIVED | SCOPE_CODEX_HIDDEN => execute_codex_scope(
                    &paths.codex,
                    scope_targets,
                    scope == SCOPE_CODEX_HIDDEN,
                    &mut scope_res,
                    &mut total.stopped_early,
                )?,
                SCOPE_ANTIGRAVITY_ARCHIVED
                    if platform.kind != crate::platform::PlatformKind::Macos =>
                {
                    scope_res
                        .issues
                        .push("仅本机 macOS 支持 Antigravity 会话清理".into());
                }
                SCOPE_ANTIGRAVITY_ARCHIVED => execute_antigravity_archived_scope(
                    &paths.antigravity,
                    scope_targets,
                    &mut scope_res,
                    &mut total.stopped_early,
                )?,
                SCOPE_DSH_ARCHIVED => execute_dsh_archived_scope(
                    &paths.dsh,
                    scope_targets,
                    &mut scope_res,
                    &mut total.stopped_early,
                )?,
                SCOPE_CLAUDE_EMPTY => {
                    let allowed_map = crate::claude::find_empty_inactive_sessions(&paths.claude)?
                        .into_iter()
                        .map(|(s, fp, _)| (s.session_key, fp))
                        .collect();
                    execute_empty_inactive_scope(
                        scope_targets,
                        &allowed_map,
                        |id| {
                            crate::claude::claude_session_summary(&paths.claude, id)
                                .ok()
                                .and_then(|s| {
                                    (s.status == "running"
                                        || s.delete_block_reason.as_deref().is_some_and(|r| {
                                            r.contains("正在运行") || r.contains("占用")
                                        }))
                                    .then(|| {
                                        s.delete_block_reason.unwrap_or_else(|| {
                                            "Claude 会话正在运行，无法删除".into()
                                        })
                                    })
                                })
                        },
                        |id, fp| {
                            let r = crate::claude::execute_claude_delete(
                                &paths.claude,
                                crate::claude::ClaudeDeleteRequest {
                                    session_key: id.to_owned(),
                                    confirmed: true,
                                    fingerprint: fp.to_owned(),
                                },
                            )?;
                            Ok((r.deleted, r.bytes, r.residuals))
                        },
                        &mut scope_res,
                        &mut total.stopped_early,
                    )?;
                }
                SCOPE_GROK_EMPTY => {
                    let allowed_map = crate::grok::find_empty_inactive_sessions(&paths.grok)?
                        .into_iter()
                        .map(|(s, fp, _)| (s.id, fp))
                        .collect();
                    execute_empty_inactive_scope(
                        scope_targets,
                        &allowed_map,
                        |id| {
                            crate::grok::grok_session_summary(&paths.grok, id)
                                .ok()
                                .and_then(|s| {
                                    s.deletion_eligibility.filter(|e| !e.allowed).map(|e| {
                                        e.reason
                                            .unwrap_or_else(|| "Grok 会话正在运行，无法删除".into())
                                    })
                                })
                        },
                        |id, fp| {
                            let r = crate::grok::execute_grok_delete(
                                &paths.grok,
                                crate::grok::GrokDeleteRequest {
                                    id: id.to_owned(),
                                    confirmed: true,
                                    fingerprint: fp.to_owned(),
                                },
                            )?;
                            Ok((r.deleted, r.bytes, r.residuals))
                        },
                        &mut scope_res,
                        &mut total.stopped_early,
                    )?;
                }
                _ => unreachable!(),
            }
        }
        scope_res.residuals.sort();
        scope_res.residuals.dedup();
        total.deleted_count += scope_res.deleted_count;
        total.skipped_count += scope_res.skipped_count;
        total.failed_count += scope_res.failed_count;
        total.deleted_bytes = total.deleted_bytes.saturating_add(scope_res.deleted_bytes);
        total.scopes.push(scope_res);
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use crate::{
        archive::HiddenThreadSelection,
        platform::{PlatformKind, PlatformPaths},
        services::{
            cleanup::{
                cleanup_confirmation_message, plan_cleanup_execute_operation,
                plan_cleanup_operation, CleanupAction, CleanupExecuteRequest, CleanupOperationKind,
                CleanupTarget, HIDDEN_DELETE_CONFIRMATION_MESSAGE,
            },
            commands,
            system::Capability,
        },
    };

    #[test]
    fn cleanup_operation_plan_keeps_dry_run_and_confirmed_execute_boundaries_shared() {
        let linux = PlatformPaths::for_kind(PlatformKind::Linux);
        let macos = PlatformPaths::for_kind(PlatformKind::Macos);
        let windows = PlatformPaths::for_kind(PlatformKind::Windows);

        let dry_run = plan_cleanup_operation(
            &linux,
            CleanupTarget::Archived,
            CleanupOperationKind::DryRun,
        )
        .unwrap();
        assert_eq!(dry_run.required_capability, Capability::ThreadCleanup);
        assert_eq!(dry_run.target, CleanupTarget::Archived);
        assert_eq!(dry_run.action, CleanupAction::ArchiveDeleteDryRun);
        assert_eq!(dry_run.command, commands::CLEANUP_ARCHIVE_DRY_RUN);
        assert!(!dry_run.execute);
        assert!(!dry_run.requires_confirmation);
        assert!(!dry_run.requires_prior_dry_run);
        assert!(!dry_run.requires_expected_count);
        assert_eq!(dry_run.confirmation_message, None);

        let unconfirmed_execute =
            plan_cleanup_operation(&macos, CleanupTarget::Hidden, CleanupOperationKind::Execute)
                .expect_err("execute cleanup operation must require explicit confirmation");
        assert!(
            unconfirmed_execute
                .to_string()
                .contains(HIDDEN_DELETE_CONFIRMATION_MESSAGE),
            "{unconfirmed_execute}"
        );

        let execute = plan_cleanup_execute_operation(
            &macos,
            CleanupTarget::Hidden,
            CleanupExecuteRequest {
                confirmed: true,
                expected_count: Some(2),
                candidates: Some(vec![
                    HiddenThreadSelection {
                        id: "candidate-one".into(),
                        fingerprint: Some("fingerprint-one".into()),
                    },
                    HiddenThreadSelection {
                        id: "candidate-two".into(),
                        fingerprint: Some("fingerprint-two".into()),
                    },
                ]),
            },
        )
        .unwrap();
        assert_eq!(execute.target, CleanupTarget::Hidden);
        assert_eq!(execute.action, CleanupAction::HiddenDeleteExecute);
        assert_eq!(execute.command, commands::CLEANUP_HIDDEN_EXECUTE);
        assert!(execute.execute);
        assert!(execute.requires_confirmation);
        assert!(execute.requires_prior_dry_run);
        assert!(!execute.requires_expected_count);
        assert_eq!(
            execute.confirmation_message.as_deref(),
            Some(HIDDEN_DELETE_CONFIRMATION_MESSAGE)
        );
        assert!(execute.confirmation.confirmed);
        assert_eq!(execute.confirmation.expected_count, Some(2));
        assert_eq!(
            execute.confirmation.payload["candidates"]
                .as_array()
                .unwrap()
                .len(),
            2
        );

        let archived_execute = plan_cleanup_execute_operation(
            &macos,
            CleanupTarget::Archived,
            CleanupExecuteRequest {
                confirmed: true,
                expected_count: Some(2),
                candidates: Some(Vec::new()),
            },
        )
        .unwrap();
        assert!(!archived_execute.requires_expected_count);
        plan_cleanup_execute_operation(
            &macos,
            CleanupTarget::Hidden,
            CleanupExecuteRequest {
                confirmed: true,
                expected_count: None,
                candidates: Some(Vec::new()),
            },
        )
        .unwrap();
        assert_eq!(
            cleanup_confirmation_message(CleanupTarget::Hidden),
            HIDDEN_DELETE_CONFIRMATION_MESSAGE
        );

        let missing_candidates = plan_cleanup_execute_operation(
            &macos,
            CleanupTarget::Hidden,
            CleanupExecuteRequest {
                confirmed: true,
                expected_count: Some(0),
                candidates: None,
            },
        )
        .expect_err("hidden cleanup must be bound to a preview candidate list");
        assert!(missing_candidates
            .to_string()
            .contains("清理需要明确的线程列表"));

        plan_cleanup_execute_operation(
            &macos,
            CleanupTarget::Hidden,
            CleanupExecuteRequest {
                confirmed: true,
                expected_count: Some(1),
                candidates: Some(Vec::new()),
            },
        )
        .unwrap();

        assert!(plan_cleanup_operation(
            &windows,
            CleanupTarget::Archived,
            CleanupOperationKind::DryRun
        )
        .is_err());
    }

    #[test]
    fn unified_cleanup_recomputes_targets_preserves_non_empty_and_stops_on_first_failure() {
        use std::fs;
        let tmp =
            std::env::temp_dir().join(format!("agenthub-unified-cleanup-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&tmp).unwrap();

        let codex = crate::codex::CodexPaths::new(tmp.join("codex"));
        fs::create_dir_all(&codex.home).unwrap();
        let antigravity = crate::antigravity::sessions::AntigravityPaths {
            home: tmp.join("antigravity"),
            rpc_mode: crate::antigravity::sessions::RpcMode::OfflineOnly,
        };
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
        let linux = PlatformPaths::for_kind(PlatformKind::Linux);
        let macos = PlatformPaths::for_kind(PlatformKind::Macos);

        // Linux marks Antigravity scope unsupported; macOS marks it supported.
        let linux_plan = super::dry_run_unified_with_capability(
            &linux,
            &paths,
            super::UnifiedCleanupDryRunRequest {
                scopes: Vec::new(),
                include_codex_hidden: true,
            },
        )
        .unwrap();
        let ag_linux = linux_plan
            .scopes
            .iter()
            .find(|s| s.scope == super::SCOPE_ANTIGRAVITY_ARCHIVED)
            .unwrap();
        assert!(!ag_linux.supported);

        let mac_plan = super::dry_run_unified_with_capability(
            &macos,
            &paths,
            super::UnifiedCleanupDryRunRequest {
                scopes: Vec::new(),
                include_codex_hidden: true,
            },
        )
        .unwrap();
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

        // Stop-on-first-failure: locking Claude's project dir stops before Grok empty is deleted!
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
            assert!(stopped.stopped_early);
            assert_eq!(stopped.failed_count, 1);
            assert!(
                grok_empty_dir.exists(),
                "subsequent scope must not delete after earlier scope failure"
            );
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
        assert!(!ok.stopped_early);
        assert_eq!(ok.deleted_count, 2);
        assert!(!claude_empty_file.exists());
        assert!(!grok_empty_dir.exists());
        assert!(
            late_grok_dir.exists(),
            "session created after preview must not be deleted"
        );
        assert!(claude_active_file.exists());
        assert!(grok_active_dir.exists());

        // Targeted O(K) Codex hidden + archived execution test (running target stops, late archived untouched).
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
        let codex_plan = super::dry_run_unified_with_capability(
            &macos,
            &paths,
            super::UnifiedCleanupDryRunRequest {
                scopes: vec![super::SCOPE_CODEX_HIDDEN.into()],
                include_codex_hidden: true,
            },
        )
        .unwrap();
        let hidden_targets: Vec<super::UnifiedCleanupTargetSelection> = codex_plan.scopes[0]
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
        assert!(running_stop.stopped_early);
        assert_eq!(running_stop.failed_count, 1);
        assert_eq!(running_stop.deleted_count, 0);
        assert!(paths
            .codex
            .sessions_dir()
            .join(format!("rollout-{h2}.jsonl"))
            .exists());

        // Mark h1 completed (bumping updated_at -> fingerprint changed -> skipped; h2 deleted; h_late untouched!)
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
        assert!(!after_stop.stopped_early);
        assert_eq!(after_stop.skipped_count, 1);
        assert_eq!(after_stop.deleted_count, 1);
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
