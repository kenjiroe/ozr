use crate::api::approval_gate::ApiApprovalGate;
use crate::api::handlers::AppState;
use crate::api::state::{SessionStatus, SessionStore, SessionView};
use crate::core::runtime::run_agent_once;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::sse::{Event, Sse};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio_stream::wrappers::UnboundedReceiverStream;

const DEFAULT_COMPLETION_TIMEOUT: Duration = Duration::from_secs(120);
const STREAM_CHUNK_CHARS: usize = 24;

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
struct ChatCompletionChunk {
    id: String,
    object: &'static str,
    created: u64,
    model: String,
    choices: Vec<ChatCompletionChunkChoice>,
}

#[derive(Debug, Serialize)]
struct ChatCompletionChunkChoice {
    index: u32,
    delta: ChatCompletionDelta,
    #[serde(skip_serializing_if = "Option::is_none")]
    finish_reason: Option<&'static str>,
}

#[derive(Debug, Serialize, Default)]
struct ChatCompletionDelta {
    #[serde(skip_serializing_if = "Option::is_none")]
    role: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
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
) -> Result<Response, OpenAiShimError> {
    let prompt = extract_user_prompt(&body.messages).ok_or_else(|| {
        OpenAiShimError::bad_request("messages must include at least one user prompt")
    })?;

    if prompt.trim().is_empty() {
        return Err(OpenAiShimError::bad_request(
            "user prompt must not be empty",
        ));
    }

    if body.stream {
        return Ok(stream_chat_completion(state, body, prompt).await.into_response());
    }

    Ok(Json(
        complete_chat_completion(state, body, prompt)
            .await
            .map_err(|message| match message.contains("timed out") {
                true => OpenAiShimError::gateway_timeout(message),
                false => OpenAiShimError::internal(message),
            })?,
    )
    .into_response())
}

async fn complete_chat_completion(
    state: AppState,
    body: ChatCompletionRequest,
    prompt: String,
) -> Result<ChatCompletionResponse, String> {
    let session_id = start_agent_session(&state, prompt).await;
    let view = wait_for_session_terminal(&state.sessions, &session_id, DEFAULT_COMPLETION_TIMEOUT)
        .await?;

    let content = view
        .result
        .ok_or_else(|| "completed session missing result".to_string())?;

    Ok(build_completion_response(
        &body.model,
        content,
        &session_id,
    ))
}

async fn stream_chat_completion(
    state: AppState,
    body: ChatCompletionRequest,
    prompt: String,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    let model = resolve_model(&body.model);
    let session_id = start_agent_session(&state, prompt).await;
    let created = unix_timestamp();
    let completion_id = format!("chatcmpl-{}-{}", session_id, created);

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let store = state.sessions.clone();
    let sid = session_id.clone();
    tokio::spawn(async move {
        emit_stream_events(
            tx,
            store,
            sid,
            completion_id,
            model,
            created,
            DEFAULT_COMPLETION_TIMEOUT,
        )
        .await;
    });

    Sse::new(UnboundedReceiverStream::new(rx))
}

async fn start_agent_session(state: &AppState, prompt: String) -> String {
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

    session_id
}

async fn emit_stream_events(
    tx: tokio::sync::mpsc::UnboundedSender<Result<Event, Infallible>>,
    store: SessionStore,
    session_id: String,
    completion_id: String,
    model: String,
    created: u64,
    timeout: Duration,
) {
    let send = |event: Event| {
        let _ = tx.send(Ok(event));
    };

    match wait_for_session_terminal(&store, &session_id, timeout).await {
        Ok(view) => {
            let content = view.result.unwrap_or_default();
            for event in build_stream_events(&completion_id, &model, created, &content) {
                send(event);
            }
        }
        Err(message) => {
            send(stream_chunk(
                &completion_id,
                &model,
                created,
                ChatCompletionDelta {
                    content: Some(message),
                    ..ChatCompletionDelta::default()
                },
                None,
            ));
            send(stream_chunk(
                &completion_id,
                &model,
                created,
                ChatCompletionDelta::default(),
                Some("stop"),
            ));
        }
    }

    send(Event::default().data("[DONE]"));
}

fn build_stream_events(
    completion_id: &str,
    model: &str,
    created: u64,
    content: &str,
) -> Vec<Event> {
    let mut events = vec![stream_chunk(
        completion_id,
        model,
        created,
        ChatCompletionDelta {
            role: Some("assistant"),
            ..ChatCompletionDelta::default()
        },
        None,
    )];

    for piece in chunk_content(content, STREAM_CHUNK_CHARS) {
        events.push(stream_chunk(
            completion_id,
            model,
            created,
            ChatCompletionDelta {
                content: Some(piece),
                ..ChatCompletionDelta::default()
            },
            None,
        ));
    }

    events.push(stream_chunk(
        completion_id,
        model,
        created,
        ChatCompletionDelta::default(),
        Some("stop"),
    ));
    events
}

fn stream_chunk(
    completion_id: &str,
    model: &str,
    created: u64,
    delta: ChatCompletionDelta,
    finish_reason: Option<&'static str>,
) -> Event {
    let chunk = ChatCompletionChunk {
        id: completion_id.to_string(),
        object: "chat.completion.chunk",
        created,
        model: model.to_string(),
        choices: vec![ChatCompletionChunkChoice {
            index: 0,
            delta,
            finish_reason,
        }],
    };
    Event::default().json_data(chunk).unwrap_or_else(|err| {
        Event::default().data(format!("{{\"error\":\"{}\"}}", err))
    })
}

fn chunk_content(text: &str, max_chars: usize) -> Vec<String> {
    if text.is_empty() {
        return vec![String::new()];
    }

    text.chars()
        .collect::<Vec<_>>()
        .chunks(max_chars.max(1))
        .map(|chunk| chunk.iter().collect())
        .collect()
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
    let model = resolve_model(requested_model);
    let created = unix_timestamp();

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

fn resolve_model(requested_model: &str) -> String {
    if requested_model.trim().is_empty() {
        "ozr".to_string()
    } else {
        requested_model.to_string()
    }
}

fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
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

    #[test]
    fn chunk_content_splits_by_unicode_chars() {
        let chunks = chunk_content("abcdefgh", 3);
        assert_eq!(chunks, vec!["abc", "def", "gh"]);
    }

    #[test]
    fn build_stream_events_include_role_content_and_stop() {
        let text = "a".repeat(STREAM_CHUNK_CHARS + 1);
        let events = build_stream_events("id-1", "ozr", 123, &text);
        assert_eq!(events.len(), 4);
    }
}
