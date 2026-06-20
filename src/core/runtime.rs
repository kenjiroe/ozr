use crate::core::approval::ApprovalGate;
use crate::core::audit::AuditLogger;
use crate::core::budget::BudgetGuard;
use crate::core::config::AppConfig;
use crate::core::llm_adapter::{build_llm_provider, LlmProvider};
use crate::core::mcp_client::{build_mcp_client, McpClient};
use crate::core::memory::MemoryStore;
use crate::core::policy::PolicyEngine;
use crate::core::policy_pack::{BudgetPreset, PolicyPack};
use crate::core::sandbox_executor::{
    HostExecutor, SandboxdApiExecutor, SandboxdExecutor, SandboxdSettings,
};
use crate::core::agent_loop::AgentLoop;
use std::time::Duration;

pub async fn run_agent_once<A: ApprovalGate>(
    cfg: &AppConfig,
    prompt: &str,
    approver: A,
) -> Result<String, String> {
    let memory = MemoryStore::new(".ozr");
    memory.ensure_layout()?;

    let mut audit = AuditLogger::new(".ozr/audit/runs.log")?;
    let mut policy = PolicyEngine::default();
    policy.ponytail_mode = cfg.ponytail_mode;

    let policy_pack = PolicyPack::from_env(&cfg.policy_pack);
    let mut budget_preset = BudgetPreset {
        max_tokens: cfg.budget_max_tokens,
        max_iterations: cfg.budget_max_iterations,
        max_run_seconds: cfg.budget_max_run_seconds,
    };
    policy_pack.apply(&mut policy, &cfg.approval_mode, &mut budget_preset);

    let budget = BudgetGuard::new(
        budget_preset.max_tokens,
        budget_preset.max_iterations,
        Duration::from_secs(budget_preset.max_run_seconds),
    );
    let llm = build_llm_provider(cfg);
    let mcp = build_mcp_client(cfg);

    run_agent_with_deps(cfg, prompt, policy, budget, llm, mcp, approver, memory, &mut audit).await
}

async fn run_agent_with_deps(
    cfg: &AppConfig,
    prompt: &str,
    policy: PolicyEngine,
    budget: BudgetGuard,
    llm: Box<dyn LlmProvider>,
    mcp: Box<dyn McpClient>,
    approver: impl ApprovalGate,
    memory: MemoryStore,
    audit: &mut AuditLogger,
) -> Result<String, String> {
    if cfg.feature_sandboxd_executor {
        let settings = sandboxd_settings_from_config(cfg);
        if settings.is_ready() {
            let executor = SandboxdApiExecutor::new(settings);
            let mut loop_engine =
                AgentLoop::new(policy, budget, llm, mcp, executor, approver, memory, audit);
            return loop_engine
                .run_once(prompt)
                .await
                .map_err(|err| err.to_string());
        }
        let executor = SandboxdExecutor::default();
        let mut loop_engine =
            AgentLoop::new(policy, budget, llm, mcp, executor, approver, memory, audit);
        return loop_engine
            .run_once(prompt)
            .await
            .map_err(|err| err.to_string());
    }

    let executor = HostExecutor::default();
    let mut loop_engine =
        AgentLoop::new(policy, budget, llm, mcp, executor, approver, memory, audit);
    loop_engine
        .run_once(prompt)
        .await
        .map_err(|err| err.to_string())
}

fn sandboxd_settings_from_config(cfg: &AppConfig) -> SandboxdSettings {
    SandboxdSettings {
        api_base: cfg.sandboxd_api_base.clone(),
        api_token: cfg.sandboxd_api_token.clone(),
        sandbox_id: cfg.sandboxd_sandbox_id.clone(),
        agent: cfg.sandboxd_agent.clone(),
        poll_attempts: cfg.sandboxd_poll_attempts,
        poll_interval_ms: cfg.sandboxd_poll_interval_ms,
        poll_backoff_multiplier: cfg.sandboxd_poll_backoff_multiplier,
        poll_max_interval_ms: cfg.sandboxd_poll_max_interval_ms,
        capture_events: cfg.sandboxd_capture_events,
        events_max_time_s: cfg.sandboxd_events_max_time_s,
        require_auth: cfg.sandboxd_require_auth,
        https_only: cfg.sandboxd_https_only,
    }
}
