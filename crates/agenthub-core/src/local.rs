use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LocalPluginInfo {
    pub id: String,
    pub label: String,
    pub status: String,
    pub kind: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unavailable_reason: Option<String>,
    pub invocation_template: String,
}

pub fn local_plugin_catalog() -> Vec<LocalPluginInfo> {
    vec![
        LocalPluginInfo {
            id: "codex".to_string(),
            label: "Codex".to_string(),
            status: "ready".to_string(),
            kind: "builtin".to_string(),
            description: "Codex 本地状态、线程和受控 job 操作".to_string(),
            unavailable_reason: None,
            invocation_template: "@Codex ".to_string(),
        },
        LocalPluginInfo {
            id: "probe".to_string(),
            label: "Probe".to_string(),
            status: "ready".to_string(),
            kind: "builtin".to_string(),
            description: "探针状态、Hook 和多来源 Bark 通知".to_string(),
            unavailable_reason: None,
            invocation_template: "@Probe ".to_string(),
        },
        LocalPluginInfo {
            id: "grok_build".to_string(),
            label: "Grok Build".to_string(),
            status: "ready".to_string(),
            kind: "builtin".to_string(),
            description: "Grok Build 会话、消息和工具活动只读浏览".to_string(),
            unavailable_reason: None,
            invocation_template: "@Grok Build ".to_string(),
        },
        LocalPluginInfo {
            id: "system_ops".to_string(),
            label: "System/Ops".to_string(),
            status: "ready".to_string(),
            kind: "builtin".to_string(),
            description: "固定系统运维动作和发布更新任务".to_string(),
            unavailable_reason: None,
            invocation_template: "@System/Ops ".to_string(),
        },
    ]
}
