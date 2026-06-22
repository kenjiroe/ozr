use crate::core::agent_loop::AgentLoop;
use crate::core::approval::ApprovalGate;
use crate::core::audit::AuditLogger;
use crate::core::config::AppConfig;
use crate::core::llm_adapter::{build_llm_provider, LlmProvider};
use crate::core::mcp_client::{build_mcp_client, McpClient};
use crate::core::memory::MemoryStore;
use crate::core::policy_pack::PolicyPack;
use crate::core::sandbox_executor::RuntimeExecutor;

pub async fn run_agent_once<A: ApprovalGate>(
    cfg: &AppConfig,
    prompt: &str,
    approver: A,
) -> Result<String, String> {
    let memory = MemoryStore::new(".ozr");
    memory.ensure_layout()?;

    let mut audit = AuditLogger::new(".ozr/audit/runs.log")?;
    let (policy, budget) = PolicyPack::prepare_runtime(cfg)?;

    let llm = build_llm_provider(cfg);
    let mcp = build_mcp_client(cfg);

    run_agent_with_deps(
        cfg, prompt, policy, budget, llm, mcp, approver, memory, &mut audit,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn run_agent_with_deps(
    cfg: &AppConfig,
    prompt: &str,
    policy: crate::core::policy::PolicyEngine,
    budget: crate::core::budget::BudgetGuard,
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
