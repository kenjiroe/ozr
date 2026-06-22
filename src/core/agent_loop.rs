use crate::core::approval::{ApprovalDecision, ApprovalGate};
use crate::core::audit::AuditLogger;
use crate::core::budget::BudgetGuard;
use crate::core::guardrail::Guardrail;
use crate::core::llm_adapter::LlmProvider;
use crate::core::mcp_client::McpClient;
use crate::core::memory::MemoryStore;
use crate::core::policy::{ActionKind, PlannedAction, PolicyEngine, RiskTier};
use crate::core::sandbox_executor::SandboxExecutor;
use crate::core::session_recovery::{begin_session, complete_session, fail_session, touch_session};
use std::error::Error;
use std::time::{SystemTime, UNIX_EPOCH};

type LoopResult<T> = Result<T, Box<dyn Error>>;

pub struct AgentLoop<'a, L: LlmProvider, M: McpClient, E: SandboxExecutor, A: ApprovalGate> {
    policy: PolicyEngine,
    budget: BudgetGuard,
    llm: L,
    mcp: M,
    executor: E,
    approver: A,
    memory: MemoryStore,
    audit: &'a mut AuditLogger,
}

impl<'a, L: LlmProvider, M: McpClient, E: SandboxExecutor, A: ApprovalGate>
    AgentLoop<'a, L, M, E, A>
{
    pub fn new(
        policy: PolicyEngine,
        budget: BudgetGuard,
        llm: L,
        mcp: M,
        executor: E,
        approver: A,
        memory: MemoryStore,
        audit: &'a mut AuditLogger,
    ) -> Self {
        Self {
            policy,
            budget,
            llm,
            mcp,
            executor,
            approver,
            memory,
            audit,
        }
    }

    pub async fn run_once(&mut self, prompt: &str) -> LoopResult<String> {
        self.memory.ensure_layout()?;

        let run_id = generate_run_id()?;
        self.audit.append(&run_id, "loop_started")?;
        let _ = self
            .memory
            .append_session_event(&run_id, "loop_started", prompt);
        let _ = begin_session(&run_id, prompt);
        let guardrail = Guardrail::new(&self.policy);
        self.audit.append(
            &run_id,
            &format!("ponytail_mode:{}", guardrail.ponytail_mode().as_str()),
        )?;

        if let Err(reason) = self.budget.consume_iteration() {
            return self.fail_run(&run_id, &reason);
        }

        let catalog = self.mcp.list_tool_definitions().await;
        self.audit.append(&run_id, "tools_discovered")?;

        let mut plan = self.llm.propose_plan(prompt, &catalog).await;
        if let Err(reason) = self.budget.consume_tokens(plan.estimated_tokens) {
            return self.fail_run(&run_id, &reason);
        }
        let mut approval_attempts = 0usize;

        loop {
            let action = PlannedAction {
                tool: plan.tool.clone(),
                kind: plan.action_kind,
            };
            self.audit
                .append(&run_id, &format!("plan_tool:{}", action.tool))?;
            let _ = touch_session(&run_id, &format!("plan_tool:{}", action.tool));

            let gate = guardrail.check_plan(&action);
            self.audit.append(
                &run_id,
                &format!("plan_evaluated:{}", tier_label(gate.tier)),
            )?;
            let _ = touch_session(
                &run_id,
                &format!("plan_evaluated:{}", tier_label(gate.tier)),
            );

            if gate.requires_approval {
                let approval = self
                    .approver
                    .request(&action, gate.tier, &plan.params)
                    .await?;
                match approval.decision {
                    ApprovalDecision::Approve => {
                        self.audit.append(
                            &run_id,
                            &format!("approved:{}:{}", action.tool, approval.reason),
                        )?;
                    }
                    ApprovalDecision::Deny => {
                        self.audit.append(
                            &run_id,
                            &format!("denied:{}:{}", action.tool, approval.reason),
                        )?;
                        let _ = fail_session(&run_id, "plan denied by approval gate");
                        return Err("plan denied by approval gate".into());
                    }
                    ApprovalDecision::Skip => {
                        self.audit.append(
                            &run_id,
                            &format!("skipped:{}:{}", action.tool, approval.reason),
                        )?;
                        let _ = complete_session(&run_id);
                        return Ok("action skipped by operator".to_string());
                    }
                    ApprovalDecision::Retry => {
                        approval_attempts += 1;
                        self.audit.append(
                            &run_id,
                            &format!("retry_requested:{}:{}", action.tool, approval.reason),
                        )?;

                        if approval_attempts > 2 {
                            return self.fail_run(&run_id, "approval retry limit exceeded");
                        }

                        let retry_prompt = format!(
                            "{}\n\nOperator requested safer retry attempt {}",
                            prompt, approval_attempts
                        );
                        plan = self.llm.propose_plan(&retry_prompt, &catalog).await;
                        if let Err(reason) = self.budget.consume_tokens(plan.estimated_tokens) {
                            return self.fail_run(&run_id, &reason);
                        }
                        continue;
                    }
                    ApprovalDecision::EditPlan => {
                        if approval.edited_params.trim().is_empty() {
                            return self.fail_run(&run_id, "edited plan params are empty");
                        }
                        plan.params = approval.edited_params.clone();
                        self.audit.append(
                            &run_id,
                            &format!("plan_edited:{}:{}", action.tool, approval.reason),
                        )?;
                    }
                }
            }

            if self.policy.require_sandboxd
                && action.kind != ActionKind::Read
                && self.executor.uses_host_execution()
            {
                return self.fail_run(
                    &run_id,
                    "policy requires sandboxd executor for Shell/Write/Network actions",
                );
            }

            let tool_result = self
                .executor
                .execute(&action, &plan.params, &self.mcp)
                .await?;
            self.audit
                .append(&run_id, &format!("tool_executed:{}", action.tool))?;
            if let Some(task_id) = extract_sandboxd_task_id(&tool_result) {
                self.audit
                    .append(&run_id, &format!("sandboxd_task:{}", task_id))?;
            }

            let summary = self.llm.summarize(prompt, &tool_result).await;
            self.audit.append(&run_id, "loop_completed")?;
            let _ = self
                .memory
                .append_session_event(&run_id, "loop_completed", &action.tool);
            let _ = complete_session(&run_id);
            return Ok(summary);
        }
    }

    fn fail_run(&mut self, run_id: &str, reason: &str) -> LoopResult<String> {
        let _ = self
            .audit
            .append(run_id, &format!("loop_failed:{}", reason));
        let _ = self
            .memory
            .append_session_event(run_id, "loop_failed", reason);
        let _ = fail_session(run_id, reason);
        Err(reason.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::approval::{
        ApprovalDecision, ApprovalGate, ApprovalMode, ApprovalOutcome, CliApprovalGate,
    };
    use crate::core::llm_adapter::{LlmProvider, ToolCallPlan};
    use crate::core::mcp_client::MockMcpClient;
    use crate::core::policy::ActionKind;
    use crate::core::sandbox_executor::HostExecutor;
    use std::time::Duration;

    struct DenyGate;

    #[async_trait::async_trait]
    impl ApprovalGate for DenyGate {
        async fn request(
            &mut self,
            _action: &PlannedAction,
            _tier: RiskTier,
            _params: &str,
        ) -> Result<ApprovalOutcome, String> {
            Ok(ApprovalOutcome {
                decision: ApprovalDecision::Deny,
                reason: "test deny".to_string(),
                edited_params: String::new(),
            })
        }
    }

    use async_trait::async_trait;

    struct WritePlanLlm;

    #[async_trait]
    impl LlmProvider for WritePlanLlm {
        async fn propose_plan(
            &self,
            prompt: &str,
            _catalog: &[crate::core::mcp_tool_catalog::McpToolDefinition],
        ) -> ToolCallPlan {
            ToolCallPlan {
                tool: "write_file".to_string(),
                params: format!("prompt={}", prompt),
                action_kind: ActionKind::Write,
                estimated_tokens: 50,
            }
        }

        async fn summarize(&self, _prompt: &str, _tool_result: &str) -> String {
            "unused".to_string()
        }
    }

    #[tokio::test]
    async fn transition_denied_plan_stops_before_execute() {
        let _guard = crate::test_support::env_test_lock();
        let dir = tempfile::tempdir().expect("tempdir");
        let checkpoint = dir.path().join("checkpoint.json");
        std::env::set_var("OZR_SESSION_CHECKPOINT_PATH", checkpoint.to_str().unwrap());
        let audit_path = dir.path().join("audit.log");
        let mut audit = AuditLogger::new(audit_path.to_str().unwrap()).expect("audit");
        let memory = MemoryStore::new(dir.path());
        let mut engine = AgentLoop::new(
            PolicyEngine::default(),
            BudgetGuard::new(500, 3, Duration::from_secs(5)),
            WritePlanLlm,
            MockMcpClient::default(),
            HostExecutor::default(),
            DenyGate,
            memory,
            &mut audit,
        );
        let result = engine.run_once("deny write").await;
        assert!(result.is_err());
        std::env::remove_var("OZR_SESSION_CHECKPOINT_PATH");
    }

    #[tokio::test]
    async fn transition_medium_risk_hits_approval_gate() {
        let _guard = crate::test_support::env_test_lock();
        let dir = tempfile::tempdir().expect("tempdir");
        let checkpoint = dir.path().join("checkpoint.json");
        std::env::set_var("OZR_SESSION_CHECKPOINT_PATH", checkpoint.to_str().unwrap());
        let audit_path = dir.path().join("audit.log");
        let mut audit = AuditLogger::new(audit_path.to_str().unwrap()).expect("audit");
        let memory = MemoryStore::new(dir.path());
        let mut engine = AgentLoop::new(
            PolicyEngine::default(),
            BudgetGuard::new(500, 3, Duration::from_secs(5)),
            WritePlanLlm,
            MockMcpClient::default(),
            HostExecutor::default(),
            CliApprovalGate::new(ApprovalMode::AutoDeny),
            memory,
            &mut audit,
        );
        let result = engine.run_once("write file").await;
        assert!(result.is_err());
        std::env::remove_var("OZR_SESSION_CHECKPOINT_PATH");
    }
}

fn extract_sandboxd_task_id(result: &str) -> Option<String> {
    result.lines().find_map(|line| {
        line.strip_prefix("sandboxd_task=")
            .map(|task_id| task_id.trim().to_string())
            .filter(|task_id| !task_id.is_empty())
    })
}

fn generate_run_id() -> Result<String, String> {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_millis();
    Ok(format!("run-{}", ts))
}

fn tier_label(tier: RiskTier) -> &'static str {
    match tier {
        RiskTier::Low => "low",
        RiskTier::Medium => "medium",
        RiskTier::High => "high",
    }
}
