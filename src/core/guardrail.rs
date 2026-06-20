use crate::core::policy::{Decision, PlannedAction, PolicyEngine, PonytailMode, RiskTier};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlanGateResult {
    pub tier: RiskTier,
    pub decision: Decision,
    pub requires_approval: bool,
}

pub struct Guardrail<'a> {
    engine: &'a PolicyEngine,
}

impl<'a> Guardrail<'a> {
    pub fn new(engine: &'a PolicyEngine) -> Self {
        Self { engine }
    }

    pub fn check_plan(&self, action: &PlannedAction) -> PlanGateResult {
        let (tier, decision) = self.engine.evaluate(action);
        PlanGateResult {
            tier,
            decision,
            requires_approval: decision == Decision::RequireApproval,
        }
    }

    pub fn ponytail_mode(&self) -> PonytailMode {
        self.engine.ponytail_mode
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::policy::{ActionKind, PolicyEngine};

    #[test]
    fn guardrail_is_required_entry_for_plan_mode() {
        let engine = PolicyEngine::default();
        let guardrail = Guardrail::new(&engine);
        let action = PlannedAction {
            tool: "read_file".to_string(),
            kind: ActionKind::Read,
        };
        let gate = guardrail.check_plan(&action);
        assert_eq!(gate.tier, RiskTier::Low);
        assert_eq!(gate.decision, Decision::AutoApprove);
        assert!(!gate.requires_approval);
    }

    #[test]
    fn guardrail_flags_shell_for_approval() {
        let engine = PolicyEngine::default();
        let guardrail = Guardrail::new(&engine);
        let action = PlannedAction {
            tool: "run_shell".to_string(),
            kind: ActionKind::Shell,
        };
        let gate = guardrail.check_plan(&action);
        assert_eq!(gate.tier, RiskTier::High);
        assert!(gate.requires_approval);
    }
}
