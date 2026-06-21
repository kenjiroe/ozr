use crate::core::approval::ApprovalGate;
use crate::core::audit::AuditLogger;
use crate::core::budget::BudgetGuard;
use crate::core::config::AppConfig;
use crate::core::llm_adapter::{build_llm_provider, LlmProvider};
use crate::core::mcp_client::{build_mcp_client, McpClient};
use crate::core::memory::MemoryStore;
use crate::core::policy::PolicyEngine;
use crate::core::policy_pack::{BudgetPreset, PolicyPack};
use crate::core::sandbox_executor::RuntimeExecutor;
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
    let executor = RuntimeExecutor::from_config(cfg);
    let mut loop_engine =
        AgentLoop::new(policy, budget, llm, mcp, executor, approver, memory, audit);
    loop_engine
        .run_once(prompt)
        .await
        .map_err(|err| err.to_string())
}
