use crate::api::approval_gate::ApiApprovalGate;
use crate::api::handlers::AppState;
use crate::api::state::{SessionStatus, SessionStore, SessionView};
use crate::core::runtime::run_agent_once;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const DEFAULT_COMPLETION_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Deserialize)]
pub struct ChatCompletionRequest {
    #[serde(default)]
    pub model: String,
    pub messages: Vec<ChatMessage>,
    #[serde(default)]
    pub stream: bool,
}

#[derive(Debug, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: ChatContent,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum ChatContent {
    Text(String),
    Parts(Vec<ChatContentPart>),
}

#[derive(Debug, Deserialize)]
pub struct ChatContentPart {
    #[serde(rename = "type")]
    pub kind: String,
    pub text: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ChatCompletionResponse {
    pub id: String,
    pub object: &'static str,
    pub created: u64,
    pub model: String,
    pub choices: Vec<ChatCompletionChoice>,
}

#[derive(Debug, Serialize)]
pub struct ChatCompletionChoice {
    pub index: u32,
    pub message: ChatCompletionMessage,
    pub finish_reason: &'static str,
}

#[derive(Debug, Serialize)]
pub struct ChatCompletionMessage {
    pub role: &'static str,
    pub content: String,
}

#[derive(Debug, Serialize)]
pub struct OpenAiErrorResponse {
    pub error: OpenAiErrorBody,
}

#[derive(Debug, Serialize)]
pub struct OpenAiErrorBody {
    pub message: String,
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub code: Option<String>,
}

pub async fn chat_completions(
    State(state): State<AppState>,
    Json(body): Json<ChatCompletionRequest>,
) -> Result<Json<ChatCompletionResponse>, OpenAiShimError> {
    if body.stream {
        return Err(OpenAiShimError::bad_request(
            "streaming is not supported yet; set stream=false",
        ));
    }

    let prompt = extract_user_prompt(&body.messages).ok_or_else(|| {
        OpenAiShimError::bad_request("messages must include at least one user prompt")
    })?;

    if prompt.trim().is_empty() {
        return Err(OpenAiShimError::bad_request("user prompt must not be empty"));
    }

    let session_id = SessionStore::new_session_id();
    state
        .sessions
        .create(&session_id, prompt.clone())
        .await;

    let cfg = state.cfg.clone();
    let store = state.sessions.clone();
    let sid = session_id.clone();
    tokio::spawn(async move {
        let gate = ApiApprovalGate::new(sid.clone(), store.clone());
        match run_agent_once(&cfg, &prompt, gate).await {
            Ok(result) => store.complete(&sid, result).await,
            Err(err) => store.fail(&sid, err).await,
        }
    });

    let view = wait_for_session_terminal(&state.sessions, &session_id, DEFAULT_COMPLETION_TIMEOUT)
        .await
        .map_err(|message| match message.contains("timed out") {
            true => OpenAiShimError::gateway_timeout(message),
            false => OpenAiShimError::internal(message),
        })?;

    let content = view
        .result
        .ok_or_else(|| OpenAiShimError::internal("completed session missing result"))?;

    Ok(Json(build_completion_response(
        &body.model,
        content,
        &session_id,
    )))
}

fn extract_user_prompt(messages: &[ChatMessage]) -> Option<String> {
    messages
        .iter()
        .rev()
        .find(|message| message.role.eq_ignore_ascii_case("user"))
        .map(|message| match &message.content {
            ChatContent::Text(text) => text.clone(),
            ChatContent::Parts(parts) => parts
                .iter()
                .filter(|part| part.kind == "text")
                .filter_map(|part| part.text.clone())
                .collect::<Vec<_>>()
                .join("\n"),
        })
        .filter(|text| !text.trim().is_empty())
}

async fn wait_for_session_terminal(
    store: &SessionStore,
    session_id: &str,
    timeout: Duration,
) -> Result<SessionView, String> {
    let deadline = Instant::now() + timeout;
    loop {
        let view = store
            .get_view(session_id)
            .await
            .ok_or_else(|| "session not found".to_string())?;

        match view.status {
            SessionStatus::Completed => return Ok(view),
            SessionStatus::Failed => {
                return Err(view.error.unwrap_or_else(|| "run failed".to_string()));
            }
            SessionStatus::Running | SessionStatus::PendingApproval => {
                if Instant::now() >= deadline {
                    return Err(format!(
                        "timed out waiting for session {session_id} (status={:?}). Approve via POST /v1/session/{session_id}/approve",
                        view.status
                    ));
                }
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        }
    }
}

fn build_completion_response(
    requested_model: &str,
    content: String,
    session_id: &str,
) -> ChatCompletionResponse {
    let model = if requested_model.trim().is_empty() {
        "ozr".to_string()
    } else {
        requested_model.to_string()
    };
    let created = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);

    ChatCompletionResponse {
        id: format!("chatcmpl-{}-{}", session_id, created),
        object: "chat.completion",
        created,
        model,
        choices: vec![ChatCompletionChoice {
            index: 0,
            message: ChatCompletionMessage {
                role: "assistant",
                content,
            },
            finish_reason: "stop",
        }],
    }
}

#[derive(Debug)]
pub struct OpenAiShimError {
    status: StatusCode,
    message: String,
    kind: &'static str,
}

impl OpenAiShimError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
            kind: "invalid_request_error",
        }
    }

    fn internal(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: message.into(),
            kind: "server_error",
        }
    }

    fn gateway_timeout(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::GATEWAY_TIMEOUT,
            message: message.into(),
            kind: "timeout_error",
        }
    }
}

impl IntoResponse for OpenAiShimError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(OpenAiErrorResponse {
                error: OpenAiErrorBody {
                    message: self.message,
                    kind: self.kind,
                    code: None,
                },
            }),
        )
            .into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_user_prompt_uses_last_user_message() {
        let messages = vec![
            ChatMessage {
                role: "system".to_string(),
                content: ChatContent::Text("rules".to_string()),
            },
            ChatMessage {
                role: "user".to_string(),
                content: ChatContent::Text("first".to_string()),
            },
            ChatMessage {
                role: "assistant".to_string(),
                content: ChatContent::Text("ok".to_string()),
            },
            ChatMessage {
                role: "user".to_string(),
                content: ChatContent::Text("second".to_string()),
            },
        ];
        assert_eq!(extract_user_prompt(&messages).as_deref(), Some("second"));
    }
}
