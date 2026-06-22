use axum::{
    extract::Path,
    routing::{get, post},
    Json, Router,
};
use ozr::core::config::AppConfig;
use ozr::core::mcp_client::MockMcpClient;
use ozr::core::policy::{ActionKind, PlannedAction};
use ozr::core::sandbox_executor::{SandboxExecutor, SandboxdApiExecutor, SandboxdSettings};
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::oneshot;

struct MockSandboxdServer {
    base_url: String,
    shutdown: Option<oneshot::Sender<()>>,
    _task: tokio::task::JoinHandle<()>,
}

impl MockSandboxdServer {
    async fn start() -> Self {
        let poll_hits = Arc::new(AtomicUsize::new(0));
        let app = Router::new()
            .route("/v1/sandboxes/{sandbox_id}", get(get_sandbox))
            .route("/v1/sandboxes/{sandbox_id}/tasks", post(submit_task))
            .route(
                "/v1/sandboxes/{sandbox_id}/tasks/{task_id}",
                get(get_task_status),
            )
            .route(
                "/v1/sandboxes/{sandbox_id}/tasks/{task_id}/events",
                get(get_task_events),
            )
            .with_state(poll_hits);

        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind mock");
        let port = listener.local_addr().expect("addr").port();
        let base_url = format!("http://127.0.0.1:{}", port);
        let (shutdown_tx, shutdown_rx) = oneshot::channel();

        let task = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async move {
                    let _ = shutdown_rx.await;
                })
                .await
                .expect("mock sandboxd serve");
        });

        Self {
            base_url,
            shutdown: Some(shutdown_tx),
            _task: task,
        }
    }

    fn base_url(&self) -> &str {
        &self.base_url
    }
}

impl Drop for MockSandboxdServer {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
    }
}

async fn get_sandbox(Path(sandbox_id): Path<String>) -> Json<serde_json::Value> {
    Json(json!({ "id": sandbox_id, "status": "ready" }))
}

async fn submit_task() -> Json<serde_json::Value> {
    Json(json!({ "id": "task-mock-1" }))
}

async fn get_task_status(
    Path((_sandbox_id, task_id)): Path<(String, String)>,
    axum::extract::State(poll_hits): axum::extract::State<Arc<AtomicUsize>>,
) -> Json<serde_json::Value> {
    let hits = poll_hits.fetch_add(1, Ordering::SeqCst);
    if hits == 0 {
        Json(json!({ "id": task_id, "status": "running" }))
    } else {
        Json(json!({
            "id": task_id,
            "status": "succeeded",
            "result": "isolated execution ok"
        }))
    }
}

async fn get_task_events() -> String {
    "event: log\ndata: {\"line\":\"mock event\"}\n\n".to_string()
}

fn mock_settings(base_url: &str) -> SandboxdSettings {
    SandboxdSettings {
        api_base: base_url.to_string(),
        api_token: String::new(),
        sandbox_id: "ozr-test-sandbox".to_string(),
        agent: "opencode".to_string(),
        poll_attempts: 5,
        poll_interval_ms: 10,
        poll_backoff_multiplier: 1,
        poll_max_interval_ms: 10,
        capture_events: false,
        events_max_time_s: 1,
        require_auth: false,
        https_only: false,
    }
}

#[tokio::test]
async fn sandboxd_api_executor_submits_polls_and_returns_task_id() {
    let server = MockSandboxdServer::start().await;
    let executor = SandboxdApiExecutor::new(mock_settings(server.base_url()));
    let action = PlannedAction {
        tool: "run_shell".to_string(),
        kind: ActionKind::Shell,
    };
    let mcp = MockMcpClient;
    let result = executor
        .execute(&action, "prompt=integration test", &mcp)
        .await
        .expect("execute");

    assert!(result.contains("sandboxd_task=task-mock-1"));
    assert!(result.contains("status=succeeded"));
    assert!(result.contains("isolated execution ok"));
}

#[tokio::test]
async fn sandboxd_api_executor_routes_read_via_mcp() {
    let server = MockSandboxdServer::start().await;
    let executor = SandboxdApiExecutor::new(mock_settings(server.base_url()));
    let action = PlannedAction {
        tool: "read_file".to_string(),
        kind: ActionKind::Read,
    };
    let mcp = MockMcpClient;
    let result = executor
        .execute(&action, "path=README.md", &mcp)
        .await
        .expect("execute");

    assert!(result.contains("mock read executed"));
    assert!(!result.contains("sandboxd_task="));
}

#[tokio::test]
async fn runtime_executor_selects_api_backend_when_sandbox_id_set() {
    let server = MockSandboxdServer::start().await;
    let cfg = AppConfig {
        feature_sandboxd_executor: true,
        sandboxd_api_base: server.base_url().to_string(),
        sandboxd_sandbox_id: "ozr-test-sandbox".to_string(),
        sandboxd_poll_interval_ms: 10,
        sandboxd_poll_max_interval_ms: 10,
        ..Default::default()
    };

    let executor = ozr::core::sandbox_executor::RuntimeExecutor::from_config(&cfg);
    let action = PlannedAction {
        tool: "run_shell".to_string(),
        kind: ActionKind::Shell,
    };
    let result = executor
        .execute(&action, "prompt=via runtime", &MockMcpClient)
        .await
        .expect("execute");

    assert!(result.contains("sandboxd_task=task-mock-1"));
}

#[tokio::test]
async fn sandboxd_stub_still_used_without_sandbox_id() {
    let cfg = AppConfig {
        feature_sandboxd_executor: true,
        ..Default::default()
    };
    let executor = ozr::core::sandbox_executor::RuntimeExecutor::from_config(&cfg);
    let action = PlannedAction {
        tool: "run_shell".to_string(),
        kind: ActionKind::Shell,
    };
    let result = executor
        .execute(&action, "prompt=stub", &MockMcpClient)
        .await
        .expect("execute");

    assert!(result.contains("sandboxd-stub executed"));
}

#[test]
fn integration_fixture_reaches_mock_sandboxd() {
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    rt.block_on(async {
        let server = MockSandboxdServer::start().await;
        let cfg = AppConfig {
            feature_sandboxd_executor: true,
            sandboxd_api_base: server.base_url().to_string(),
            sandboxd_sandbox_id: "ozr-test-sandbox".to_string(),
            ..Default::default()
        };

        let report = ozr::core::integration_fixtures::run_sandboxd_fixture(&cfg).expect("fixture");
        assert_eq!(report.name, "sandboxd");
        assert!(report.detail.contains("ozr-test-sandbox"));
    });
}

async fn json_request(
    app: axum::Router,
    method: &str,
    uri: &str,
    body: Option<String>,
) -> (axum::http::StatusCode, serde_json::Value) {
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

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
        serde_json::Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
    };
    (status, payload)
}

async fn poll_session_until(
    app: axum::Router,
    session_id: &str,
    want_status: &str,
    max_attempts: usize,
) -> serde_json::Value {
    let mut last = serde_json::Value::Null;
    for _ in 0..max_attempts {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let (_, payload) = json_request(
            app.clone(),
            "GET",
            &format!("/v1/session/{}", session_id),
            None,
        )
        .await;
        last = payload.clone();
        if payload["status"] == want_status {
            return payload;
        }
        if payload["status"] == "failed" {
            panic!(
                "session {} failed: {}",
                session_id,
                payload["error"].as_str().unwrap_or("unknown")
            );
        }
    }
    panic!("session {session_id} did not reach {want_status}; last={last}");
}

#[tokio::test]
async fn api_e2e_routes_shell_to_sandboxd_mock_after_approval() {
    let server = MockSandboxdServer::start().await;
    let cfg = AppConfig {
        feature_sandboxd_executor: true,
        sandboxd_api_base: server.base_url().to_string(),
        sandboxd_sandbox_id: "ozr-test-sandbox".to_string(),
        sandboxd_poll_interval_ms: 10,
        sandboxd_poll_max_interval_ms: 10,
        ..Default::default()
    };

    let app = ozr::api::app_for_tests(cfg);
    let (status, payload) = json_request(
        app.clone(),
        "POST",
        "/v1/run",
        Some(r#"{"prompt":"run mystery shell task"}"#.to_string()),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    let session_id = payload["session_id"].as_str().expect("session_id");

    poll_session_until(app.clone(), session_id, "pending_approval", 50).await;

    let (approve_status, _) = json_request(
        app.clone(),
        "POST",
        &format!("/v1/session/{}/approve", session_id),
        Some(r#"{"decision":"approve","reason":"sandboxd mock ok"}"#.to_string()),
    )
    .await;
    assert_eq!(approve_status, axum::http::StatusCode::OK);

    let completed = poll_session_until(app, session_id, "completed", 100).await;
    let result = completed["result"].as_str().expect("result");
    assert!(result.contains("sandboxd_task=task-mock-1"));
    assert!(result.contains("isolated execution ok"));
}
