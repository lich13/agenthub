use crate::{
    codex::{self, CodexPaths},
    grok::{self, GrokPaths},
    selected_codex,
};
use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, path::PathBuf};

pub const MAX_BATCH_SIZE: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionProvider {
    Codex,
    #[serde(rename = "claude_code")]
    Claude,
    Grok,
    Antigravity,
    Dsh,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionOperation {
    Archive,
    Restore,
    Delete,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionBatchRequest {
    pub provider: SessionProvider,
    pub operation: SessionOperation,
    pub session_keys: Vec<String>,
    #[serde(default)]
    pub include_descendants: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionBatchItem {
    pub session_key: String,
    pub id: String,
    pub title: String,
    pub paths: Vec<PathBuf>,
    pub bytes: u64,
    pub allowed: bool,
    pub reason: Option<String>,
    pub fingerprint: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub descendants: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionBatchPreview {
    pub provider: SessionProvider,
    pub operation: SessionOperation,
    pub items: Vec<SessionBatchItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionBatchSelection {
    pub session_key: String,
    #[serde(default)]
    pub fingerprint: String,
    #[serde(default)]
    pub descendants: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionBatchExecuteRequest {
    pub provider: SessionProvider,
    pub operation: SessionOperation,
    pub items: Vec<SessionBatchSelection>,
    #[serde(default)]
    pub confirmed: bool,
    #[serde(default)]
    pub include_descendants: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionBatchItemResult {
    pub session_key: String,
    pub status: String,
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deleted_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub residuals: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionBatchResult {
    pub items: Vec<SessionBatchItemResult>,
}

pub struct SessionUseCases {
    pub platform: crate::platform::PlatformPaths,
    pub codex: CodexPaths,
    pub grok: GrokPaths,
    pub claude: crate::claude::ClaudePaths,
    pub antigravity: crate::antigravity::sessions::AntigravityPaths,
    pub dsh: crate::dsh::DshPaths,
}

fn validate(provider: SessionProvider, operation: SessionOperation, keys: &[String]) -> Result<()> {
    ensure!(
        !keys.is_empty() && (operation == SessionOperation::Delete || keys.len() <= MAX_BATCH_SIZE),
        "每批请选择 1 至 100 个线程"
    );
    ensure!(
        matches!(
            provider,
            SessionProvider::Codex | SessionProvider::Antigravity | SessionProvider::Dsh
        ) || operation == SessionOperation::Delete,
        "此 provider 不支持归档或恢复"
    );
    let mut unique = HashSet::new();
    for key in keys {
        ensure!(
            !key.trim().is_empty() && !key.chars().any(char::is_control),
            "无效的线程定位键"
        );
        ensure!(unique.insert(key), "同一批次不能重复选择线程");
    }
    Ok(())
}

impl SessionUseCases {
    pub fn attachment_read(
        &self,
        request: crate::user_message::SessionAttachmentRequest,
    ) -> Result<crate::user_message::SessionAttachmentResponse> {
        for value in [
            &request.session_key,
            &request.message_id,
            &request.attachment_id,
        ] {
            ensure!(
                !value.is_empty() && value.len() <= 512 && !value.chars().any(char::is_control),
                "无效的附件定位键"
            );
        }
        if request.root_thread_id.is_some() {
            ensure!(
                request.provider == SessionProvider::Codex,
                "子智能体附件仅支持 Codex"
            );
            return codex::subagents::read_attachment(&self.codex, &request);
        }
        let message = match request.provider {
            SessionProvider::Claude => {
                return crate::claude::read_attachment(&self.claude, &request);
            }
            SessionProvider::Antigravity => {
                return crate::antigravity::sessions::read_attachment(&self.antigravity, &request);
            }
            SessionProvider::Dsh => {
                return crate::dsh::sessions::read_attachment(&self.dsh, &request);
            }
            SessionProvider::Codex => codex::thread_detail(&self.codex, &request.session_key)?
                .into_iter()
                .flat_map(|detail| detail.blocks)
                .filter_map(|block| block.user_message)
                .find(|message| message.id == request.message_id),
            SessionProvider::Grok => {
                grok::grok_session_detail(&self.grok, &request.session_key, None)?
                    .events
                    .iter()
                    .filter_map(|event| event.user_message.as_ref())
                    .find(|message| message.id == request.message_id)
                    .cloned()
            }
        }
        .ok_or_else(|| anyhow::anyhow!("会话消息已变化或不存在，请刷新会话"))?;
        message.read_attachment(&request.attachment_id)
    }

    pub fn bulk_preview(&self, request: SessionBatchRequest) -> Result<SessionBatchPreview> {
        crate::services::system::require_capability(
            &self.platform,
            crate::services::system::Capability::ThreadCleanup,
        )?;
        validate(request.provider, request.operation, &request.session_keys)?;
        let grok = (request.provider == SessionProvider::Grok)
            .then(|| grok::DeletionContext::new(&self.grok));
        let items = request
            .session_keys
            .iter()
            .map(|key| {
                self.preview_one(
                    request.provider,
                    request.operation,
                    key,
                    request.include_descendants,
                    grok.as_ref(),
                )
                .unwrap_or_else(|error| {
                    self.blocked_item(request.provider, key, format!("{error:#}"), grok.as_ref())
                })
            })
            .collect();
        Ok(SessionBatchPreview {
            provider: request.provider,
            operation: request.operation,
            items,
        })
    }

    fn blocked_item(
        &self,
        provider: SessionProvider,
        key: &str,
        reason: String,
        grok: Option<&grok::DeletionContext<'_>>,
    ) -> SessionBatchItem {
        let mut item = SessionBatchItem {
            session_key: key.to_string(),
            id: key.to_string(),
            title: key.to_string(),
            paths: vec![],
            bytes: 0,
            allowed: false,
            reason: Some(reason),
            fingerprint: None,
            descendants: vec![],
            warnings: vec![],
        };
        let summary = match provider {
            SessionProvider::Claude => crate::claude::claude_session_summary(&self.claude, key)
                .ok()
                .map(|s| (s.id, s.title, vec![s.path])),
            SessionProvider::Codex => codex::local_thread_summary(&self.codex, key)
                .ok()
                .flatten()
                .map(|s| {
                    (
                        s.id,
                        s.title,
                        s.rollout_path.into_iter().collect::<Vec<_>>(),
                    )
                }),
            SessionProvider::Grok => grok
                .map_or_else(
                    || grok::grok_session_summary(&self.grok, key),
                    |context| context.summary(key),
                )
                .ok()
                .map(|s| (s.id, s.title, vec![s.path])),
            SessionProvider::Antigravity => {
                crate::antigravity::sessions::session_summary(&self.antigravity, key)
                    .ok()
                    .map(|s| (s.id, s.title, vec![s.path]))
            }
            SessionProvider::Dsh => crate::dsh::sessions::session_summary(&self.dsh, key)
                .ok()
                .map(|s| (s.id, s.title, vec![s.path])),
        };
        if let Some((id, title, paths)) = summary {
            item.id = id;
            item.title = title;
            item.paths = paths;
            item.bytes = crate::session_delete::size(&item.paths).1;
        }
        item
    }

    fn preview_one(
        &self,
        provider: SessionProvider,
        operation: SessionOperation,
        key: &str,
        include_descendants: bool,
        grok: Option<&grok::DeletionContext<'_>>,
    ) -> Result<SessionBatchItem> {
        let mut descendants = Vec::new();
        let mut warnings = Vec::new();
        let (id, title, paths, bytes, fingerprint) = match provider {
            SessionProvider::Claude => {
                let p = crate::claude::preview_claude_delete(&self.claude, key)?;
                warnings = p.warnings.unwrap_or_default();
                (p.id, p.title, p.paths, p.bytes, p.fingerprint)
            }
            SessionProvider::Codex => {
                if operation != SessionOperation::Delete {
                    ensure!(
                        codex::codex_task_identity(&self.codex, key)?
                            == codex::CodexTaskIdentity::Main,
                        "线程身份无法确认为主任务"
                    );
                }
                if operation == SessionOperation::Delete {
                    let tree =
                        selected_codex::tree::preview(&self.codex, key, include_descendants)?;
                    descendants = tree.ids.into_iter().filter(|id| id != key).collect();
                    warnings = tree.warnings;
                    let p = tree.selected;
                    (p.id, p.title, p.paths, p.bytes, p.fingerprint)
                } else {
                    let p = selected_codex::preview_archive(
                        &self.codex,
                        key,
                        operation == SessionOperation::Archive,
                    )?;
                    (p.id, p.title, p.paths, p.bytes, p.fingerprint)
                }
            }
            SessionProvider::Grok => {
                let p = grok.map_or_else(
                    || grok::preview_grok_delete(&self.grok, key),
                    |context| context.preview(key),
                )?;
                (p.id, p.title, vec![p.path], p.bytes, p.fingerprint)
            }
            SessionProvider::Antigravity => {
                return crate::antigravity::sessions::preview_batch_item(
                    &self.antigravity,
                    key,
                    operation,
                    include_descendants,
                );
            }
            SessionProvider::Dsh => {
                return crate::dsh::sessions::preview_batch_item(
                    &self.dsh,
                    key,
                    operation,
                    include_descendants,
                );
            }
        };
        Ok(SessionBatchItem {
            session_key: key.to_string(),
            id,
            title,
            paths,
            bytes,
            allowed: true,
            reason: None,
            fingerprint: Some(fingerprint),
            descendants,
            warnings,
        })
    }

    pub fn bulk_execute(&self, request: SessionBatchExecuteRequest) -> Result<SessionBatchResult> {
        crate::services::system::require_capability(
            &self.platform,
            crate::services::system::Capability::ThreadCleanup,
        )?;
        let keys = request
            .items
            .iter()
            .map(|item| item.session_key.clone())
            .collect::<Vec<_>>();
        validate(request.provider, request.operation, &keys)?;
        ensure!(
            request.operation != SessionOperation::Delete || request.confirmed,
            "删除需要明确确认"
        );
        let mut results = Vec::new();
        for item in &request.items {
            let mut result = SessionBatchItemResult {
                session_key: item.session_key.clone(),
                status: "succeeded".into(),
                message: None,
                deleted_ids: vec![],
                residuals: vec![],
            };
            let action = (|| -> Result<()> {
                if matches!(
                    request.provider,
                    SessionProvider::Antigravity | SessionProvider::Dsh
                ) {
                    let outcome = match request.provider {
                        SessionProvider::Antigravity => {
                            crate::antigravity::sessions::execute_batch_item(
                                &self.antigravity,
                                &item.session_key,
                                request.operation,
                                &item.descendants,
                                request.include_descendants,
                            )?
                        }
                        SessionProvider::Dsh => crate::dsh::sessions::execute_batch_item(
                            &self.dsh,
                            &item.session_key,
                            request.operation,
                            &item.descendants,
                            request.include_descendants,
                        )?,
                        _ => unreachable!(),
                    };
                    result.status = outcome.status;
                    result.deleted_ids = outcome.deleted_ids;
                    result.residuals = outcome.residuals;
                    result.message = outcome.message;
                    return Ok(());
                }
                if request.operation != SessionOperation::Delete {
                    return selected_codex::execute_archive(
                        &self.codex,
                        &item.session_key,
                        request.operation == SessionOperation::Archive,
                        &item.fingerprint,
                    );
                }
                match request.provider {
                    SessionProvider::Codex => {
                        let verified: std::collections::BTreeSet<String> =
                            if request.include_descendants {
                                selected_codex::tree::descendants(
                                    &self.codex,
                                    std::slice::from_ref(&item.session_key),
                                )?
                                .into_iter()
                                .filter(|id| id != &item.session_key)
                                .collect()
                            } else {
                                std::collections::BTreeSet::new()
                            };
                        for child in &item.descendants {
                            ensure!(
                                request.include_descendants && verified.contains(child),
                                "子线程 {child} 不在当前主线程名下，请重新预览"
                            );
                        }
                        let mut ids = vec![item.session_key.clone()];
                        if request.include_descendants {
                            ids.extend(verified);
                        }
                        let outcome = selected_codex::tree::execute_ids(&self.codex, &ids)?;
                        result.deleted_ids = outcome.deleted_ids;
                        result.residuals = outcome.residuals;
                        result.message = outcome.error;
                    }
                    SessionProvider::Claude => {
                        let outcome = crate::claude::execute_claude_delete(
                            &self.claude,
                            crate::claude::ClaudeDeleteRequest {
                                session_key: item.session_key.clone(),
                                confirmed: true,
                                fingerprint: item.fingerprint.clone(),
                            },
                        )?;
                        result.residuals = outcome.residuals;
                        if outcome.deleted {
                            result.deleted_ids.push(item.session_key.clone());
                        }
                    }
                    SessionProvider::Grok => {
                        let outcome = grok::execute_grok_delete(
                            &self.grok,
                            grok::GrokDeleteRequest {
                                id: item.session_key.clone(),
                                confirmed: true,
                                fingerprint: item.fingerprint.clone(),
                            },
                        )?;
                        result.residuals = outcome.residuals;
                        if outcome.deleted {
                            result.deleted_ids.push(item.session_key.clone());
                        }
                    }
                    SessionProvider::Antigravity | SessionProvider::Dsh => unreachable!(),
                }
                Ok(())
            })();
            if let Err(error) = action {
                result.status = if request.operation == SessionOperation::Delete {
                    "failed"
                } else {
                    "blocked"
                }
                .into();
                result.message = Some(format!("{error:#}"));
            } else if !result.residuals.is_empty() {
                result.status = "failed".into();
                result
                    .message
                    .get_or_insert_with(|| "部分内容未删除，请查看逐项结果".into());
            }
            results.push(result);
        }
        Ok(SessionBatchResult { items: results })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delete_accepts_over_one_hundred_targets_and_optional_fingerprints() {
        let root =
            std::env::temp_dir().join(format!("agenthub-bulk-delete-{}", uuid::Uuid::new_v4()));
        let service = SessionUseCases {
            platform: crate::platform::PlatformPaths::desktop_current(),
            codex: CodexPaths::new(root.join("codex")),
            grok: GrokPaths {
                home: root.join("grok"),
            },
            claude: crate::claude::ClaudePaths {
                config: root.join("claude"),
                projects: root.join("claude/projects"),
            },
            antigravity: crate::antigravity::sessions::AntigravityPaths::offline(
                root.join("antigravity"),
            ),
            dsh: crate::dsh::DshPaths {
                home: root.join("dsh"),
            },
        };
        let keys = (0..101)
            .map(|_| uuid::Uuid::new_v4().to_string())
            .collect::<Vec<_>>();
        let root_session = service.grok.sessions().join("workspace");
        for key in &keys {
            std::fs::create_dir_all(root_session.join(key)).unwrap();
        }
        let preview = service
            .bulk_preview(SessionBatchRequest {
                provider: SessionProvider::Grok,
                operation: SessionOperation::Delete,
                session_keys: keys.clone(),
                include_descendants: false,
            })
            .unwrap();
        assert_eq!(preview.items.len(), 101);
        assert!(preview.items.iter().all(|item| item.allowed));
        let request:SessionBatchExecuteRequest=serde_json::from_value(serde_json::json!({"provider":"grok","operation":"delete","confirmed":true,"items":keys.iter().map(|key|serde_json::json!({"sessionKey":key})).collect::<Vec<_>>()})).unwrap();
        let result = service.bulk_execute(request).unwrap();
        assert!(result.items.iter().all(|item| item.status == "succeeded"));
        assert_eq!(std::fs::read_dir(root_session).unwrap().count(), 0);
        assert!(validate(SessionProvider::Codex, SessionOperation::Archive, &keys).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn bulk_execute_rejects_forged_descendants_across_codex_and_antigravity() {
        let root =
            std::env::temp_dir().join(format!("agenthub-forged-desc-{}", uuid::Uuid::new_v4()));
        let service = SessionUseCases {
            platform: crate::platform::PlatformPaths::desktop_current(),
            codex: CodexPaths::new(root.join("codex")),
            grok: GrokPaths {
                home: root.join("grok"),
            },
            claude: crate::claude::ClaudePaths {
                config: root.join("claude"),
                projects: root.join("claude/projects"),
            },
            antigravity: crate::antigravity::sessions::AntigravityPaths::offline(
                root.join("antigravity"),
            ),
            dsh: crate::dsh::DshPaths {
                home: root.join("dsh"),
            },
        };
        let codex_a = uuid::Uuid::new_v4().to_string();
        let codex_b = uuid::Uuid::new_v4().to_string();
        let codex_dir = service.codex.sessions_dir().join("2026/10/29");
        std::fs::create_dir_all(&codex_dir).unwrap();
        let file_a = codex_dir.join(format!("rollout-2026-10-29T00-00-00-{codex_a}.jsonl"));
        let file_b = codex_dir.join(format!("rollout-2026-10-29T00-00-00-{codex_b}.jsonl"));
        std::fs::write(&file_a, "{}\n").unwrap();
        std::fs::write(&file_b, "{}\n").unwrap();

        let forged_codex: SessionBatchExecuteRequest = serde_json::from_value(serde_json::json!({
            "provider": "codex",
            "operation": "delete",
            "confirmed": true,
            "includeDescendants": true,
            "items": [{ "sessionKey": codex_a, "descendants": [codex_b] }]
        }))
        .unwrap();
        let codex_res = service.bulk_execute(forged_codex).unwrap();
        assert_eq!(codex_res.items[0].status, "failed");
        assert!(file_a.exists());
        assert!(file_b.exists());

        let ag_a = uuid::Uuid::new_v4().to_string();
        let ag_b = uuid::Uuid::new_v4().to_string();
        std::fs::create_dir_all(service.antigravity.brain_dir().join(&ag_a)).unwrap();
        std::fs::create_dir_all(service.antigravity.brain_dir().join(&ag_b)).unwrap();
        let forged_ag: SessionBatchExecuteRequest = serde_json::from_value(serde_json::json!({
            "provider": "antigravity",
            "operation": "delete",
            "confirmed": true,
            "includeDescendants": true,
            "items": [{ "sessionKey": ag_a, "descendants": [ag_b] }]
        }))
        .unwrap();
        let ag_res = service.bulk_execute(forged_ag).unwrap();
        assert_eq!(ag_res.items[0].status, "failed");
        assert!(service.antigravity.brain_dir().join(&ag_a).exists());
        assert!(service.antigravity.brain_dir().join(&ag_b).exists());

        std::fs::remove_dir_all(root).unwrap();
    }
}
