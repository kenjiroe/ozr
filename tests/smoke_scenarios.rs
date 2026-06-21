use async_trait::async_trait;
use ozr::core::agent_loop::AgentLoop;
use ozr::core::approval::{
    ApprovalDecision, ApprovalGate, ApprovalMode, ApprovalOutcome, CliApprovalGate,
};
use ozr::core::audit::AuditLogger;
use ozr::core::budget::BudgetGuard;
use ozr::core::guardrail::Guardrail;
use ozr::core::llm_adapter::{LlmProvider, MockLlmProvider, ToolCallPlan};
use ozr::core::mcp_client::MockMcpClient;
use ozr::core::memory::MemoryStore;
use ozr::core::policy::{
    ActionKind, Decision, PlannedAction, PolicyEngine, PonytailMode, RiskTier,
};
use ozr::core::replay::{load_runs, RunOutcome};
use ozr::core::sandbox_executor::HostExecutor;
use ozr::core::session_recovery::{load_checkpoint, SessionStatus};
use std::sync::Mutex;
use std::time::Duration;

static SMOKE_ENV_LOCK: Mutex<()> = Mutex::new(());

struct FixedPlanLlm {
    tool: String,
    kind: ActionKind,
    tokens: usize,
}

#[async_trait]
impl LlmProvider for FixedPlanLlm {
    async fn propose_plan(
        &self,
        prompt: &str,
        _catalog: &[ozr::core::mcp_tool_catalog::McpToolDefinition],
    ) -> ToolCallPlan {
        ToolCallPlan {
            tool: self.tool.clone(),
            params: format!("prompt={}", prompt),
            action_kind: self.kind,
            estimated_tokens: self.tokens,
        }
    }

    async fn summarize(&self, prompt: &str, tool_result: &str) -> String {
        format!("summary prompt={} result={}", prompt, tool_result)
    }
}

struct StubApprovalGate {
    decision: ApprovalDecision,
}

#[async_trait]
impl ApprovalGate for StubApprovalGate {
    async fn request(
        &mut self,
        _action: &PlannedAction,
        _tier: RiskTier,
        _params: &str,
    ) -> Result<ApprovalOutcome, String> {
        Ok(ApprovalOutcome {
            decision: self.decision,
            reason: "stub approval gate".to_string(),
            edited_params: String::new(),
        })
    }
}

async fn run_case(
    llm: impl LlmProvider,
    approver: impl ApprovalGate,
    budget: BudgetGuard,
    prompt: &str,
    audit_path: &str,
    checkpoint_path: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let _lock = SMOKE_ENV_LOCK.lock().expect("smoke env lock");
    std::env::set_var("OZR_SESSION_CHECKPOINT_PATH", checkpoint_path);
    let memory = MemoryStore::new("/tmp/ozr-smoke-unused");
    let mut audit = AuditLogger::new(audit_path)?;
    let policy = PolicyEngine::default();
    let mcp = MockMcpClient::default();
    let executor = HostExecutor::default();
    let mut loop_engine = AgentLoop::new(
        policy, budget, llm, mcp, executor, approver, memory, &mut audit,
    );
    let result = loop_engine.run_once(prompt).await;
    std::env::remove_var("OZR_SESSION_CHECKPOINT_PATH");
    result
}

#[tokio::test]
async fn smoke_01_low_risk_read_completes() {
    let dir = tempfile::tempdir().unwrap();
    let audit = dir.path().join("runs.log");
    let checkpoint = dir.path().join("checkpoint.json");
    let result = run_case(
        MockLlmProvider::default(),
        CliApprovalGate::new(ApprovalMode::Prompt),
        BudgetGuard::new(2_000, 5, Duration::from_secs(5)),
        "read docs",
        audit.to_str().unwrap(),
        checkpoint.to_str().unwrap(),
    )
    .await
    .expect("run should complete");
    assert!(result.contains("summary"));
}

#[tokio::test]
async fn smoke_02_guardrail_auto_approves_read() {
    let engine = PolicyEngine::default();
    let guardrail = Guardrail::new(&engine);
    let gate = guardrail.check_plan(&PlannedAction {
        tool: "read_file".to_string(),
        kind: ActionKind::Read,
    });
    assert_eq!(gate.decision, Decision::AutoApprove);
}

#[tokio::test]
async fn smoke_03_medium_risk_requires_approval() {
    let engine = PolicyEngine::default();
    let guardrail = Guardrail::new(&engine);
    let gate = guardrail.check_plan(&PlannedAction {
        tool: "write_file".to_string(),
        kind: ActionKind::Write,
    });
    assert!(gate.requires_approval);
}

#[tokio::test]
async fn smoke_04_auto_deny_blocks_medium_risk() {
    let dir = tempfile::tempdir().unwrap();
    let audit = dir.path().join("runs.log");
    let checkpoint = dir.path().join("checkpoint.json");
    let result = run_case(
        FixedPlanLlm {
            tool: "write_file".to_string(),
            kind: ActionKind::Write,
            tokens: 50,
        },
        CliApprovalGate::new(ApprovalMode::AutoDeny),
        BudgetGuard::new(2_000, 5, Duration::from_secs(5)),
        "write config",
        audit.to_str().unwrap(),
        checkpoint.to_str().unwrap(),
    )
    .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn smoke_05_token_budget_hard_stop() {
    let dir = tempfile::tempdir().unwrap();
    let audit = dir.path().join("runs.log");
    let checkpoint = dir.path().join("checkpoint.json");
    let result = run_case(
        FixedPlanLlm {
            tool: "read_file".to_string(),
            kind: ActionKind::Read,
            tokens: 5_000,
        },
        CliApprovalGate::new(ApprovalMode::AutoApprove),
        BudgetGuard::new(100, 5, Duration::from_secs(5)),
        "expensive plan",
        audit.to_str().unwrap(),
        checkpoint.to_str().unwrap(),
    )
    .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn smoke_06_auto_approve_shell_executes() {
    let dir = tempfile::tempdir().unwrap();
    let audit = dir.path().join("runs.log");
    let checkpoint = dir.path().join("checkpoint.json");
    let result = run_case(
        FixedPlanLlm {
            tool: "read_file".to_string(),
            kind: ActionKind::Shell,
            tokens: 80,
        },
        CliApprovalGate::new(ApprovalMode::AutoApprove),
        BudgetGuard::new(2_000, 5, Duration::from_secs(5)),
        "run shell",
        audit.to_str().unwrap(),
        checkpoint.to_str().unwrap(),
    )
    .await
    .expect("shell run should complete when approved");
    assert!(result.contains("summary"));
}

#[tokio::test]
async fn smoke_07_skip_returns_without_error() {
    let dir = tempfile::tempdir().unwrap();
    let audit = dir.path().join("runs.log");
    let checkpoint = dir.path().join("checkpoint.json");
    let result = run_case(
        FixedPlanLlm {
            tool: "write_file".to_string(),
            kind: ActionKind::Write,
            tokens: 50,
        },
        StubApprovalGate {
            decision: ApprovalDecision::Skip,
        },
        BudgetGuard::new(2_000, 5, Duration::from_secs(5)),
        "skip write",
        audit.to_str().unwrap(),
        checkpoint.to_str().unwrap(),
    )
    .await
    .expect("skip should return ok");
    assert!(result.contains("skipped"));
}

#[tokio::test]
async fn smoke_08_ultra_ponytail_escalates_network() {
    let mut engine = PolicyEngine::default();
    engine.ponytail_mode = PonytailMode::Ultra;
    let guardrail = Guardrail::new(&engine);
    let gate = guardrail.check_plan(&PlannedAction {
        tool: "fetch_url".to_string(),
        kind: ActionKind::Network,
    });
    assert_eq!(gate.tier, RiskTier::High);
    assert!(gate.requires_approval);
}

#[tokio::test]
async fn smoke_09_session_checkpoint_completes() {
    let dir = tempfile::tempdir().unwrap();
    let audit = dir.path().join("runs.log");
    let checkpoint = dir.path().join("checkpoint.json");
    std::env::set_var("OZR_SESSION_CHECKPOINT_PATH", checkpoint.to_str().unwrap());
    run_case(
        MockLlmProvider::default(),
        CliApprovalGate::new(ApprovalMode::AutoApprove),
        BudgetGuard::new(2_000, 5, Duration::from_secs(5)),
        "checkpoint test",
        audit.to_str().unwrap(),
        checkpoint.to_str().unwrap(),
    )
    .await
    .expect("run should complete");
    std::env::set_var("OZR_SESSION_CHECKPOINT_PATH", checkpoint.to_str().unwrap());
    let saved = load_checkpoint()
        .expect("checkpoint load")
        .expect("checkpoint exists");
    assert_eq!(saved.status, SessionStatus::Completed);
    std::env::remove_var("OZR_SESSION_CHECKPOINT_PATH");
}

#[tokio::test]
async fn smoke_10_replay_detects_completed_run() {
    let dir = tempfile::tempdir().unwrap();
    let audit = dir.path().join("runs.log");
    let checkpoint = dir.path().join("checkpoint.json");
    run_case(
        MockLlmProvider::default(),
        CliApprovalGate::new(ApprovalMode::AutoApprove),
        BudgetGuard::new(2_000, 5, Duration::from_secs(5)),
        "replay me",
        audit.to_str().unwrap(),
        checkpoint.to_str().unwrap(),
    )
    .await
    .expect("run should complete");

    let runs = load_runs(audit.to_str().unwrap()).expect("runs load");
    let latest = runs.last().expect("run record");
    assert_eq!(latest.outcome, RunOutcome::Completed);
}
