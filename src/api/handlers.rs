use crate::api::approval_gate::ApiApprovalGate;
use crate::api::openai_shim::chat_completions;
use crate::api::state::{SessionStatus, SessionStore, SessionView};
use crate::core::approval::{parse_decision, ApprovalOutcome};
use crate::core::config::AppConfig;
use crate::core::runtime::run_agent_once;
use axum::extract::{Path, State};
use axum::http::{Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use tower_http::cors::{Any, CorsLayer};

#[derive(Clone)]
pub struct AppState {
    pub cfg: AppConfig,
    pub sessions: SessionStore,
}

#[derive(Debug, Deserialize)]
pub struct RunRequest {
    pub prompt: String,
}

#[derive(Debug, Serialize)]
pub struct RunResponse {
    pub session_id: String,
    pub status: SessionStatus,
    pub message: String,
}

#[derive(Debug, Deserialize)]
pub struct ApproveRequest {
    pub decision: String,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub edited_params: String,
}

#[derive(Debug, Serialize)]
pub struct ErrorBody {
    pub error: String,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/v1/run", post(start_run))
        .route("/v1/session/{session_id}", get(get_session))
        .route("/v1/session/{session_id}/approve", post(approve_session))
        .route("/v1/chat/completions", post(chat_completions))
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
                .allow_headers(Any),
        )
        .with_state(state)
}

async fn health() -> &'static str {
    "ok"
}

async fn start_run(
    State(state): State<AppState>,
    Json(body): Json<RunRequest>,
) -> Result<Json<RunResponse>, ApiError> {
    if body.prompt.trim().is_empty() {
        return Err(ApiError::bad_request("prompt must not be empty"));
    }

    let session_id = SessionStore::new_session_id();
    state
        .sessions
        .create(&session_id, body.prompt.clone())
        .await;

    let cfg = state.cfg.clone();
    let store = state.sessions.clone();
    let sid = session_id.clone();
    tokio::spawn(async move {
        let gate = ApiApprovalGate::new(sid.clone(), store.clone());
        match run_agent_once(&cfg, &body.prompt, gate).await {
            Ok(result) => store.complete(&sid, result).await,
            Err(err) => store.fail(&sid, err).await,
        }
    });

    Ok(Json(RunResponse {
        session_id,
        status: SessionStatus::Running,
        message: "run started; poll GET /v1/session/{session_id}".to_string(),
    }))
}

async fn get_session(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> Result<Json<SessionView>, ApiError> {
    state
        .sessions
        .get_view(&session_id)
        .await
        .map(Json)
        .ok_or_else(|| ApiError::not_found("session not found"))
}

async fn approve_session(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    Json(body): Json<ApproveRequest>,
) -> Result<Json<SessionView>, ApiError> {
    let decision = parse_decision(&body.decision);
    let reason = if body.reason.trim().is_empty() {
        match decision {
            crate::core::approval::ApprovalDecision::Approve => {
                "approved via api".to_string()
            }
            crate::core::approval::ApprovalDecision::Deny => "denied via api".to_string(),
            crate::core::approval::ApprovalDecision::Skip => "skipped via api".to_string(),
            crate::core::approval::ApprovalDecision::Retry => "retry via api".to_string(),
            crate::core::approval::ApprovalDecision::EditPlan => "edited via api".to_string(),
        }
    } else {
        body.reason.clone()
    };

    state
        .sessions
        .submit_approval(
            &session_id,
            ApprovalOutcome {
                decision,
                reason,
                edited_params: body.edited_params,
            },
        )
        .await
        .map_err(ApiError::bad_request)?;

    state
        .sessions
        .get_view(&session_id)
        .await
        .map(Json)
        .ok_or_else(|| ApiError::not_found("session not found"))
}

#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }

    fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: message.into(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(ErrorBody {
                error: self.message,
            }),
        )
            .into_response()
    }
}

pub async fn serve(cfg: AppConfig) -> Result<(), String> {
    let bind = cfg.api_bind.clone();
    let state = AppState {
        cfg,
        sessions: SessionStore::new(),
    };
    let app = router(state);
    let listener = tokio::net::TcpListener::bind(&bind)
        .await
        .map_err(|e| format!("failed to bind {}: {}", bind, e))?;
    axum::serve(listener, app)
        .await
        .map_err(|e| format!("api server error: {}", e))
}

pub fn app_for_tests(cfg: AppConfig) -> Router {
    router(AppState {
        cfg,
        sessions: SessionStore::new(),
    })
}
