use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentProviderId {
    Codex,
    ClaudeCode,
    GrokBuild,
    Antigravity,
    DeepseekHarness,
    Cursor,
    Gemini,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentProviderInfo {
    pub id: AgentProviderId,
    pub label: String,
    pub status: String,
    pub description: String,
    pub capabilities: Vec<String>,
    pub safety: String,
}

#[derive(Debug, Clone)]
pub struct ProviderRegistry {
    providers: Vec<AgentProviderInfo>,
}

impl Default for ProviderRegistry {
    fn default() -> Self {
        Self {
            providers: vec![
                AgentProviderInfo {
                    id: AgentProviderId::ClaudeCode, label: "Claude Code".into(),
                    status: if crate::claude::ClaudePaths::default_for_user().available() { "ready" } else { "unavailable" }.into(),
                    description: "Native Claude transcript history and guarded session management".into(),
                    capabilities: vec!["readonly".into(),"sessions".into(),"tools".into(),"attachments".into(),"rename".into(),"delete_local_session".into()],
                    safety: "Only inactive verified transcripts can be renamed or deleted".into(),
                },
                AgentProviderInfo {
                    id: AgentProviderId::Codex,
                    label: "Codex".to_string(),
                    status: "ready".to_string(),
                    description: "Full AgentHub control surface backed by official Codex state DB, session index, rollout files, logs_2.sqlite, and controlled jobs.".to_string(),
                    capabilities: vec![
                        "ready".to_string(),
                        "threads".to_string(),
                        "chat".to_string(),
                        "plan_questions".to_string(),
                        "uploads".to_string(),
                        "updates".to_string(),
                        "doctor".to_string(),
                    ],
                    safety: "uses existing Codex local state and controlled jobs without mutating official schema".to_string(),
                },
                AgentProviderInfo {
                    id: AgentProviderId::GrokBuild,
                    label: "Grok Build".to_string(),
                    status: "ready".to_string(),
                    description:
                        "Read-only Grok Build session history with native rename and guarded local session deletion.".to_string(),
                    capabilities: vec![
                        "readonly".to_string(),
                        "sessions".to_string(),
                        "messages".to_string(),
                        "tools".to_string(),
                        "rename".to_string(),
                        "delete_local_session".to_string(),
                    ],
                    safety: "native rename only; deletion is local-session scoped and never touches workspaces or cloud tasks".to_string(),
                },
                AgentProviderInfo {
                    id: AgentProviderId::Antigravity,
                    label: "Antigravity".to_string(),
                    status: if crate::antigravity::sessions::AntigravityPaths::default_for_user().available() {
                        "ready"
                    } else {
                        "unavailable"
                    }
                    .to_string(),
                    description: "Antigravity App native conversation trajectories with live Connect-RPC and offline SQLite/prototext management.".to_string(),
                    capabilities: vec![
                        "readonly".to_string(),
                        "sessions".to_string(),
                        "tools".to_string(),
                        "attachments".to_string(),
                        "rename".to_string(),
                        "archive".to_string(),
                        "delete_local_session".to_string(),
                    ],
                    safety: "uses official UpdateConversationAnnotations / DeleteCascadeTrajectory when running and atomic SQLite/pbtxt updates when offline".to_string(),
                },
                AgentProviderInfo {
                    id: AgentProviderId::DeepseekHarness,
                    label: "DeepSeek Harness".to_string(),
                    status: if crate::dsh::DshPaths::default_for_user().home.is_dir() {
                        "ready"
                    } else {
                        "unavailable"
                    }
                    .to_string(),
                    description: "DeepSeek Harness v4 multi-frame Zstandard session logs and workspace v2 index management.".to_string(),
                    capabilities: vec![
                        "readonly".to_string(),
                        "sessions".to_string(),
                        "tools".to_string(),
                        "attachments".to_string(),
                        "rename".to_string(),
                        "archive".to_string(),
                        "delete_local_session".to_string(),
                    ],
                    safety: "80-byte UTF-8 checksummed session/title frames under exclusive session.lock and stopped-process workspace.json writes".to_string(),
                },
                AgentProviderInfo {
                    id: AgentProviderId::Cursor,
                    label: "Cursor CLI".to_string(),
                    status: "planned".to_string(),
                    description: "Provider slot reserved for Cursor CLI once command and session contracts are defined.".to_string(),
                    capabilities: Vec::new(),
                    safety: "no command execution is exposed".to_string(),
                },
                AgentProviderInfo {
                    id: AgentProviderId::Gemini,
                    label: "Gemini CLI".to_string(),
                    status: "planned".to_string(),
                    description: "Provider slot reserved for Gemini CLI once command and session contracts are defined.".to_string(),
                    capabilities: Vec::new(),
                    safety: "no command execution is exposed".to_string(),
                },
            ],
        }
    }
}

impl ProviderRegistry {
    pub fn list(&self) -> Vec<AgentProviderInfo> {
        self.providers.clone()
    }
}
