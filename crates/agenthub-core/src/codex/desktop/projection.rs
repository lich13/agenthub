use super::review::{self, Params, BODY_LIMIT, PARAMS_LIMIT};
use super::state::{NativeState, RuntimeState};
use crate::services::codex_desktop::{
    DesktopGoal, DesktopMode, DesktopPendingRequest, DesktopQuestion, DesktopQuestionOption,
    DesktopQueuedMessage, DesktopView,
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::io::{self, Write};

struct HashWriter(Sha256);
impl Write for HashWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn hash_request(request: &impl serde::Serialize) -> String {
    let mut writer = HashWriter(Sha256::new());
    // Values and borrowed fields serialize infallibly to this writer. Preserve
    // the original canonical JSON hash without allocating a second full copy.
    let _ = serde_json::to_writer(&mut writer, request);
    hex::encode(writer.0.finalize())
}

pub fn request_key(request: &Value) -> String {
    // Include the entire request, not just a reusable numeric JSON-RPC id.
    hash_request(request)
}

pub fn state_request_key(state: &NativeState, request: &Value) -> String {
    if let Some(changes) = file_changes(state, request) {
        #[derive(serde::Serialize)]
        struct BoundRequest<'a> {
            changes: &'a Value,
            request: &'a Value,
        }
        return hash_request(&BoundRequest { changes, request });
    }
    request_key(request)
}

fn file_changes<'a>(state: &'a NativeState, request: &Value) -> Option<&'a Value> {
    if request["method"] != "item/fileChange/requestApproval" {
        return None;
    }
    let params = &request["params"];
    state
        .turns()
        .find(|turn| turn.get("turnId").unwrap_or(&turn["id"]) == &params["turnId"])
        .and_then(|turn| turn["items"].as_array())?
        .iter()
        .find(|item| item["type"] == "fileChange" && item["id"] == params["itemId"])
        .map(|item| &item["changes"])
}

pub(super) fn can_approve(state: &NativeState, request: &Value) -> bool {
    project_request(state, request).can_accept
}

pub fn view(
    state: &NativeState,
    session_id: &str,
    queue: Vec<DesktopQueuedMessage>,
) -> DesktopView {
    let value = state.value();
    DesktopView {
        connection_phase: None,
        desktop_build: None,
        goal: project_goal(value),
        session_id: session_id.into(),
        thread_id: state.thread_id().into(),
        revision: state.revision().unwrap_or(0),
        connected: state.revision().is_some(),
        writable: state.check_writable().is_ok(),
        runtime: match state.runtime() {
            RuntimeState::Idle => "idle",
            RuntimeState::Active => "active",
            RuntimeState::Waiting => "waiting",
            RuntimeState::Unavailable => "unavailable",
        }
        .into(),
        active_turn_id: state.active_turn_id().ok(),
        model: value["latestModel"].as_str().map(str::to_owned),
        reasoning_effort: value["latestReasoningEffort"].as_str().map(str::to_owned),
        mode: match value["latestCollaborationMode"]["mode"]
            .as_str()
            .or_else(|| value["mode"].as_str())
        {
            Some("default") => Some(DesktopMode::Default),
            Some("plan") => Some(DesktopMode::Plan),
            _ => None,
        },
        requests: state
            .pending_requests()
            .take(32)
            .map(|request| project_request(state, request))
            .collect(),
        queue,
        error: None,
    }
}

fn project_goal(value: &Value) -> Option<DesktopGoal> {
    // Build 20586 exposes these fields in the existing conversation snapshot
    // (native Jg projection). Its goal panel uses objective and these statuses.
    // No goal/get request or inferred follower mutation is issued here.
    let goal = value
        .get("threadGoal")
        .filter(|goal| !goal.is_null())
        .or_else(|| value.get("completedThreadGoal"))?;
    let objective = goal["objective"]
        .as_str()
        .filter(|text| !text.trim().is_empty())?;
    let status = goal["status"]
        .as_str()
        .filter(|status| {
            matches!(
                *status,
                "active" | "paused" | "blocked" | "usageLimited" | "budgetLimited" | "complete"
            )
        })
        .unwrap_or("unknown");
    Some(DesktopGoal {
        objective: bounded(objective, if objective.len() > 4096 { 4093 } else { 4096 }),
        status: status.into(),
        truncated: objective.len() > 4096,
    })
}

fn project_request(state: &NativeState, request: &Value) -> DesktopPendingRequest {
    let params = &request["params"];
    let merged = Params {
        original: params,
        changes: file_changes(state, request),
        omit: &[],
    };
    let method = request["method"].as_str().unwrap_or("");
    let approval = matches!(
        method,
        "item/commandExecution/requestApproval"
            | "item/fileChange/requestApproval"
            | "item/permissions/requestApproval"
    );
    let (details, details_cut) = if approval {
        review::json(&merged, BODY_LIMIT, true)
    } else {
        (String::new(), false)
    };
    let (kind, title, body, mut can_accept) = match method {
        "item/commandExecution/requestApproval" => (
            "command",
            "允许运行命令",
            details,
            params["command"].is_string(),
        ),
        "item/fileChange/requestApproval" => (
            "file",
            "允许修改文件",
            details,
            merged.get("changes").as_array().is_some_and(|changes| {
                !changes.is_empty()
                    && changes.iter().all(|change| {
                        change["path"].is_string()
                            && change["diff"].is_string()
                            && change.get("kind").is_some()
                    })
            }),
        ),
        "item/permissions/requestApproval" => (
            "permissions",
            "允许本回合使用权限",
            details,
            params["permissions"].is_object(),
        ),
        "item/tool/requestUserInput" => ("question", "需要你的回答", String::new(), true),
        "mcpServer/elicitation/request" => (
            "elicitation",
            "应用请求",
            bounded(params["message"].as_str().unwrap_or(""), 16 * 1024),
            params["mode"] == "form"
                && params["message"]
                    .as_str()
                    .is_some_and(|message| message.len() <= 16 * 1024)
                && supported_form(&params["requestedSchema"]),
        ),
        _ => ("unsupported", "请在 Codex 中处理", String::new(), false),
    };
    // A truncated permission request is not a reviewable approval. Keep the
    // original pending request untouched and let the desktop display it fully.
    let review = approval.then(|| review::project(&merged, kind));
    if details_cut
        || review::json(&merged, PARAMS_LIMIT, false).1
        || review.as_ref().is_some_and(|review| review.truncated)
    {
        can_accept = false;
    }
    if kind == "question" {
        can_accept &= params["questions"].as_array().is_some_and(|questions| {
            let mut ids = std::collections::HashSet::new();
            !questions.is_empty()
                && questions.len() <= 32
                && questions.iter().all(|question| {
                    question["id"]
                        .as_str()
                        .is_some_and(|id| !id.is_empty() && id.len() <= 128 && ids.insert(id))
                        && question["header"]
                            .as_str()
                            .is_none_or(|text| text.len() <= 256)
                        && question["question"]
                            .as_str()
                            .is_some_and(|text| text.len() <= 16 * 1024)
                        && question["options"].as_array().is_none_or(|options| {
                            options.len() <= 64
                                && options.iter().all(|option| {
                                    option["label"]
                                        .as_str()
                                        .is_some_and(|text| text.len() <= 4096)
                                        && option["description"]
                                            .as_str()
                                            .is_none_or(|text| text.len() <= 4096)
                                })
                        })
                })
        });
    }
    let questions = if kind == "question" {
        params["questions"]
            .as_array()
            .into_iter()
            .flatten()
            .take(32)
            .map(|question| DesktopQuestion {
                id: bounded(question["id"].as_str().unwrap_or(""), 128),
                header: bounded(question["header"].as_str().unwrap_or(""), 256),
                question: bounded(question["question"].as_str().unwrap_or(""), 16 * 1024),
                secret: question["isSecret"] == true,
                options: question["options"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .take(64)
                    .map(|option| DesktopQuestionOption {
                        label: bounded(option["label"].as_str().unwrap_or(""), 4096),
                        description: bounded(option["description"].as_str().unwrap_or(""), 4096),
                    })
                    .collect(),
            })
            .collect()
    } else {
        Vec::new()
    };
    DesktopPendingRequest {
        key: state_request_key(state, request),
        kind: kind.into(),
        title: title.into(),
        body: bounded(&body, 16 * 1024),
        review,
        questions,
        form_schema: (kind == "elicitation" && can_accept)
            .then(|| params["requestedSchema"].clone()),
        can_accept,
        supported_decisions: if can_accept && matches!(kind, "command" | "file") {
            ["accept", "acceptForSession", "decline"]
                .map(str::to_owned)
                .to_vec()
        } else {
            Vec::new()
        },
    }
}

pub(super) fn supported_form(schema: &Value) -> bool {
    schema["type"] == "object"
        && schema.as_object().is_some_and(|schema| {
            schema.keys().all(|key| {
                matches!(
                    key.as_str(),
                    "type"
                        | "properties"
                        | "required"
                        | "title"
                        | "description"
                        | "additionalProperties"
                )
            })
        })
        && (schema["additionalProperties"].is_null() || schema["additionalProperties"].is_boolean())
        && schema["properties"].as_object().is_some_and(|properties| {
            properties.len() <= 32
                && properties.iter().all(|(name, field)| {
                    !name.is_empty()
                        && name.len() <= 256
                        && !name.chars().any(char::is_control)
                        && ["title", "description"].iter().all(|key| {
                            field.get(key).is_none_or(|value| {
                                value.as_str().is_some_and(|text| text.len() <= 16 * 1024)
                            })
                        })
                        && ["minLength", "maxLength"]
                            .iter()
                            .all(|key| field.get(key).is_none_or(|value| value.as_u64().is_some()))
                        && ["minimum", "maximum"]
                            .iter()
                            .all(|key| field.get(key).is_none_or(|value| value.as_f64().is_some()))
                        && field.get("enum").is_none_or(|value| {
                            value.as_array().is_some_and(|options| {
                                !options.is_empty()
                                    && options.len() <= 64
                                    && options.iter().all(|value| match field["type"].as_str() {
                                        Some("string") => value.is_string(),
                                        Some("boolean") => value.is_boolean(),
                                        Some("number") => value.is_number(),
                                        Some("integer") => value
                                            .as_f64()
                                            .is_some_and(|number| number.fract() == 0.0),
                                        _ => false,
                                    })
                            })
                        })
                        && field.get("enumNames").is_none_or(|value| {
                            value.as_array().is_some_and(|names| {
                                field["enum"]
                                    .as_array()
                                    .is_some_and(|options| options.len() == names.len())
                                    && names.iter().all(Value::is_string)
                            })
                        })
                        && matches!(
                            field["type"].as_str(),
                            Some("string" | "number" | "integer" | "boolean")
                        )
                        && field.as_object().is_some_and(|field| {
                            field.keys().all(|key| {
                                matches!(
                                    key.as_str(),
                                    "type"
                                        | "title"
                                        | "description"
                                        | "default"
                                        | "enum"
                                        | "enumNames"
                                        | "minLength"
                                        | "maxLength"
                                        | "minimum"
                                        | "maximum"
                                )
                            })
                        })
                })
                && (schema["required"].is_null()
                    || schema["required"].as_array().is_some_and(|required| {
                        required
                            .iter()
                            .all(|key| key.as_str().is_some_and(|key| properties.contains_key(key)))
                    }))
        })
}

pub fn bounded(value: &str, limit: usize) -> String {
    if value.len() <= limit {
        return value.into();
    }
    let mut end = limit;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &value[..end])
}
