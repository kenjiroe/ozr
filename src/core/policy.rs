#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum RiskTier {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ActionKind {
    Read,
    Write,
    Shell,
    Network,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum PonytailMode {
    Off,
    Lite,
    Full,
    Ultra,
}

impl PonytailMode {
    pub fn from_env(value: &str) -> Self {
        match value.to_lowercase().as_str() {
            "lite" => PonytailMode::Lite,
            "full" => PonytailMode::Full,
            "ultra" => PonytailMode::Ultra,
            _ => PonytailMode::Off,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            PonytailMode::Off => "off",
            PonytailMode::Lite => "lite",
            PonytailMode::Full => "full",
            PonytailMode::Ultra => "ultra",
        }
    }
}

#[derive(Debug, Clone)]
pub struct PlannedAction {
    pub tool: String,
    pub kind: ActionKind,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Decision {
    AutoApprove,
    RequireApproval,
}

#[derive(Debug, Clone)]
pub struct PolicyEngine {
    pub allow_shell_auto: bool,
    pub ponytail_mode: PonytailMode,
    /// When true, Write/Shell/Network must route through a wired sandboxd executor.
    pub require_sandboxd: bool,
}

impl Default for PolicyEngine {
    fn default() -> Self {
        Self {
            allow_shell_auto: false,
            ponytail_mode: PonytailMode::Off,
            require_sandboxd: false,
        }
    }
}

impl PolicyEngine {
    pub(crate) fn evaluate(&self, action: &PlannedAction) -> (RiskTier, Decision) {
        let base_tier = match action.kind {
            ActionKind::Read => RiskTier::Low,
            ActionKind::Write => RiskTier::Medium,
            ActionKind::Network => RiskTier::Medium,
            ActionKind::Shell => RiskTier::High,
        };

        let tier = self.apply_ponytail_risk(base_tier, action.kind);

        let decision = match tier {
            RiskTier::Low => Decision::AutoApprove,
            RiskTier::Medium => Decision::RequireApproval,
            RiskTier::High => {
                if self.allow_shell_auto {
                    Decision::AutoApprove
                } else {
                    Decision::RequireApproval
                }
            }
        };

        (tier, decision)
    }

    fn apply_ponytail_risk(&self, tier: RiskTier, kind: ActionKind) -> RiskTier {
        match self.ponytail_mode {
            PonytailMode::Off => tier,
            PonytailMode::Lite => tier,
            PonytailMode::Full => {
                if kind == ActionKind::Write && tier == RiskTier::Low {
                    RiskTier::Medium
                } else {
                    tier
                }
            }
            PonytailMode::Ultra => match kind {
                ActionKind::Write | ActionKind::Network => RiskTier::High,
                _ => tier,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn low_risk_read_is_auto_approved() {
        let engine = PolicyEngine::default();
        let action = PlannedAction {
            tool: "read_file".to_string(),
            kind: ActionKind::Read,
        };
        let (_, decision) = engine.evaluate(&action);
        assert_eq!(decision, Decision::AutoApprove);
    }

    #[test]
    fn ultra_mode_escalates_network_to_high() {
        let mut engine = PolicyEngine::default();
        engine.ponytail_mode = PonytailMode::Ultra;
        let action = PlannedAction {
            tool: "fetch_url".to_string(),
            kind: ActionKind::Network,
        };

        let (tier, decision) = engine.evaluate(&action);
        assert_eq!(tier, RiskTier::High);
        assert_eq!(decision, Decision::RequireApproval);
    }
}
