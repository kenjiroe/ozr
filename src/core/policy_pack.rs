use crate::core::approval::ApprovalMode;
use crate::core::policy::{PolicyEngine, PonytailMode};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyPack {
    Strict,
    Balanced,
    Fast,
}

impl PolicyPack {
    pub fn from_env(value: &str) -> Self {
        match value.to_lowercase().as_str() {
            "strict" => PolicyPack::Strict,
            "fast" => PolicyPack::Fast,
            _ => PolicyPack::Balanced,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            PolicyPack::Strict => "strict",
            PolicyPack::Balanced => "balanced",
            PolicyPack::Fast => "fast",
        }
    }

    pub fn apply(
        &self,
        policy: &mut PolicyEngine,
        approval_mode: &ApprovalMode,
        budget: &mut BudgetPreset,
    ) {
        let _ = approval_mode;
        match self {
            PolicyPack::Strict => {
                policy.ponytail_mode = PonytailMode::Full;
                policy.allow_shell_auto = false;
                budget.max_tokens = budget.max_tokens.min(1_500);
                budget.max_iterations = budget.max_iterations.min(4);
                budget.max_run_seconds = budget.max_run_seconds.min(12);
            }
            PolicyPack::Balanced => {}
            PolicyPack::Fast => {
                policy.ponytail_mode = PonytailMode::Off;
                budget.max_tokens = budget.max_tokens.max(3_000);
                budget.max_iterations = budget.max_iterations.max(8);
                budget.max_run_seconds = budget.max_run_seconds.max(30);
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct BudgetPreset {
    pub max_tokens: usize,
    pub max_iterations: usize,
    pub max_run_seconds: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::approval::ApprovalMode;
    use crate::core::policy::{PolicyEngine, PonytailMode};

    #[test]
    fn strict_tightens_budget_and_ponytail() {
        let pack = PolicyPack::Strict;
        let mut policy = PolicyEngine::default();
        let mut budget = BudgetPreset {
            max_tokens: 2_000,
            max_iterations: 5,
            max_run_seconds: 15,
        };
        pack.apply(&mut policy, &ApprovalMode::Prompt, &mut budget);
        assert_eq!(policy.ponytail_mode, PonytailMode::Full);
        assert_eq!(budget.max_tokens, 1_500);
    }

    #[test]
    fn fast_pack_never_auto_approves_shell() {
        let pack = PolicyPack::Fast;
        let mut policy = PolicyEngine::default();
        policy.allow_shell_auto = false;
        let mut budget = BudgetPreset {
            max_tokens: 2_000,
            max_iterations: 5,
            max_run_seconds: 15,
        };
        pack.apply(&mut policy, &ApprovalMode::Prompt, &mut budget);
        assert!(!policy.allow_shell_auto);
    }
}
