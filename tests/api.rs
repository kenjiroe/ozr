use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use ozr::api::app_for_tests;
use ozr::api::state::SessionStatus;
use ozr::core::config::AppConfig;
use serde_json::Value;
use tower::ServiceExt;

async fn json_request(
    app: Router,
    method: &str,
    uri: &str,
    body: Option<String>,
) -> (StatusCode, Value) {
    let request = if let Some(body) = body {
        Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json")
            .body(Body::from(body))
            .unwrap()
    } else {
        Request::builder()
            .method(method)
            .uri(uri)
            .body(Body::empty())
            .unwrap()
    };
    let response = app.oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let payload = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, payload)
}

async fn poll_session_until(
    app: Router,
    session_id: &str,
    want_status: &str,
    max_attempts: usize,
) -> Value {
    for _ in 0..max_attempts {
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        let (status, view) = json_request(
            app.clone(),
            "GET",
            &format!("/v1/session/{}", session_id),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        if view["status"] == want_status {
            return view;
        }
        if view["status"] == "failed" {
            panic!("session failed: {}", view);
        }
        if view["status"] == "completed" && want_status != "completed" {
            panic!(
                "session reached completed before expected status {}: {}",
                want_status, view
            );
        }
    }
    panic!(
        "session {} did not reach status {} in time",
        session_id, want_status
    );
}

#[tokio::test]
async fn api_health_returns_ok() {
    let app = app_for_tests(AppConfig::default());
    let response = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn api_run_starts_session_and_completes_with_mock_llm() {
    let app = app_for_tests(AppConfig::default());
    let (status, payload) = json_request(
        app.clone(),
        "POST",
        "/v1/run",
        Some(r#"{"prompt":"read docs"}"#.to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let session_id = payload["session_id"].as_str().expect("session_id");
    assert_eq!(payload["status"], "running");

    let view = poll_session_until(app, session_id, "completed", 50).await;
    assert!(view["result"].as_str().unwrap().contains("summary"));
}

#[tokio::test]
async fn test_e2e_approval_flow_shell() {
    let app = app_for_tests(AppConfig::default());

    let (status, payload) = json_request(
        app.clone(),
        "POST",
        "/v1/run",
        Some(r#"{"prompt":"run mystery shell task"}"#.to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let session_id = payload["session_id"].as_str().expect("session_id");
    assert_eq!(payload["status"], "running");

    let pending = poll_session_until(app.clone(), session_id, "pending_approval", 50).await;
    assert_eq!(pending["status"], "pending_approval");
    assert!(pending["pending"].is_object());
    assert_eq!(pending["pending"]["tool"], "run_shell");
    assert_eq!(pending["pending"]["action_kind"], "Shell");
    assert_eq!(pending["pending"]["risk_tier"], "high");
    assert!(pending["pending"]["plan_id"]
        .as_str()
        .expect("plan_id")
        .starts_with("plan-"));

    // Without approval the session must stay blocked (not auto-complete).
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let (_, still_pending) = json_request(
        app.clone(),
        "GET",
        &format!("/v1/session/{}", session_id),
        None,
    )
    .await;
    assert_eq!(still_pending["status"], "pending_approval");

    let (approve_status, approve_view) = json_request(
        app.clone(),
        "POST",
        &format!("/v1/session/{}/approve", session_id),
        Some(r#"{"decision":"approve","reason":"e2e ok"}"#.to_string()),
    )
    .await;
    assert_eq!(approve_status, StatusCode::OK);
    assert_eq!(approve_view["status"], "pending_approval");

    let completed = poll_session_until(app, session_id, "completed", 50).await;
    assert_eq!(completed["status"], "completed");
    assert!(completed["pending"].is_null());
    assert!(completed["result"]
        .as_str()
        .expect("result")
        .contains("summary"));
}

#[tokio::test]
async fn api_approve_requires_pending_session() {
    let app = app_for_tests(AppConfig::default());
    let (status, _) = json_request(
        app,
        "POST",
        "/v1/session/sess-missing/approve",
        Some(r#"{"decision":"approve"}"#.to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn api_run_rejects_empty_prompt() {
    let app = app_for_tests(AppConfig::default());
    let (status, _) = json_request(
        app,
        "POST",
        "/v1/run",
        Some(r#"{"prompt":"  "}"#.to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn openai_chat_completions_returns_assistant_message() {
    let app = app_for_tests(AppConfig::default());
    let (status, payload) = json_request(
        app,
        "POST",
        "/v1/chat/completions",
        Some(
            r#"{"model":"ozr","messages":[{"role":"user","content":"read docs"}]}"#.to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(payload["object"], "chat.completion");
    assert_eq!(payload["choices"][0]["message"]["role"], "assistant");
    assert!(payload["choices"][0]["message"]["content"]
        .as_str()
        .unwrap()
        .contains("summary"));
}

#[tokio::test]
async fn openai_chat_completions_rejects_streaming() {
    let app = app_for_tests(AppConfig::default());
    let (status, payload) = json_request(
        app,
        "POST",
        "/v1/chat/completions",
        Some(
            r#"{"stream":true,"messages":[{"role":"user","content":"read docs"}]}"#.to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(payload["error"]["type"], "invalid_request_error");
}

#[test]
fn session_status_serializes_snake_case() {
    let status = SessionStatus::PendingApproval;
    let encoded = serde_json::to_string(&status).unwrap();
    assert_eq!(encoded, "\"pending_approval\"");
}
