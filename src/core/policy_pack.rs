use crate::core::approval::ApprovalMode;
use crate::core::budget::BudgetGuard;
use crate::core::config::AppConfig;
use crate::core::policy::{PolicyEngine, PonytailMode};
use crate::core::sandbox_executor::SandboxdSettings;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyPack {
    Strict,
    Balanced,
    Fast,
    Production,
}

impl PolicyPack {
    pub fn from_env(value: &str) -> Self {
        match value.to_lowercase().as_str() {
            "strict" => PolicyPack::Strict,
            "fast" => PolicyPack::Fast,
            "production" => PolicyPack::Production,
            _ => PolicyPack::Balanced,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            PolicyPack::Strict => "strict",
            PolicyPack::Balanced => "balanced",
            PolicyPack::Fast => "fast",
            PolicyPack::Production => "production",
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
            PolicyPack::Production => {
                policy.ponytail_mode = PonytailMode::Full;
                policy.allow_shell_auto = false;
                policy.require_sandboxd = true;
                budget.max_run_seconds = budget.max_run_seconds.max(900);
                budget.max_iterations = budget.max_iterations.max(5);
            }
        }
    }

    pub fn prepare_runtime(cfg: &AppConfig) -> Result<(PolicyEngine, BudgetGuard), String> {
        let mut policy = PolicyEngine {
            ponytail_mode: cfg.ponytail_mode,
            ..Default::default()
        };

        let pack = Self::from_env(&cfg.policy_pack);
        let mut budget_preset = BudgetPreset {
            max_tokens: cfg.budget_max_tokens,
            max_iterations: cfg.budget_max_iterations,
            max_run_seconds: cfg.budget_max_run_seconds,
        };
        pack.apply(&mut policy, &cfg.approval_mode, &mut budget_preset);
        validate_sandboxd_requirements(cfg, &policy)?;

        let budget = BudgetGuard::new(
            budget_preset.max_tokens,
            budget_preset.max_iterations,
            Duration::from_secs(budget_preset.max_run_seconds),
        );
        Ok((policy, budget))
    }
}

pub fn validate_sandboxd_requirements(cfg: &AppConfig, policy: &PolicyEngine) -> Result<(), String> {
    if !policy.require_sandboxd {
        return Ok(());
    }
    if !cfg.feature_sandboxd_executor {
        return Err(
            "policy pack requires OZR_FEATURE_SANDBOXD_EXECUTOR=true (Shell/Write/Network must run in sandboxd)"
                .to_string(),
        );
    }
    let settings = SandboxdSettings::from_config(cfg);
    if !settings.is_ready() {
        return Err(
            "policy pack requires sandboxd to be wired: set OZR_SANDBOXD_SANDBOX_ID (run ./scripts/wire-sandboxd.sh)"
                .to_string(),
        );
    }
    Ok(())
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
        assert!(!policy.require_sandboxd);
    }

    #[test]
    fn fast_pack_never_auto_approves_shell() {
        let pack = PolicyPack::Fast;
        let mut policy = PolicyEngine {
            allow_shell_auto: false,
            ..Default::default()
        };
        let mut budget = BudgetPreset {
            max_tokens: 2_000,
            max_iterations: 5,
            max_run_seconds: 15,
        };
        pack.apply(&mut policy, &ApprovalMode::Prompt, &mut budget);
        assert!(!policy.allow_shell_auto);
    }

    #[test]
    fn production_requires_sandboxd_executor() {
        let pack = PolicyPack::Production;
        let mut policy = PolicyEngine::default();
        let mut budget = BudgetPreset {
            max_tokens: 2_000,
            max_iterations: 5,
            max_run_seconds: 15,
        };
        pack.apply(&mut policy, &ApprovalMode::Prompt, &mut budget);
        assert!(policy.require_sandboxd);
        assert_eq!(budget.max_run_seconds, 900);

        let mut cfg = AppConfig {
            policy_pack: "production".to_string(),
            ..Default::default()
        };
        assert!(PolicyPack::prepare_runtime(&cfg).is_err());

        cfg.feature_sandboxd_executor = true;
        cfg.sandboxd_sandbox_id = "sb-test".to_string();
        assert!(PolicyPack::prepare_runtime(&cfg).is_ok());
    }
}
