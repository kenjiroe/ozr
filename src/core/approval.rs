use crate::core::policy::{PlannedAction, RiskTier};
use async_trait::async_trait;
use std::io::{self, Write};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ApprovalMode {
    Prompt,
    AutoApprove,
    AutoDeny,
}

impl ApprovalMode {
    pub fn from_env(value: &str) -> Self {
        match value.to_lowercase().as_str() {
            "auto" | "auto-approve" => ApprovalMode::AutoApprove,
            "deny" | "auto-deny" => ApprovalMode::AutoDeny,
            _ => ApprovalMode::Prompt,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            ApprovalMode::Prompt => "prompt",
            ApprovalMode::AutoApprove => "auto-approve",
            ApprovalMode::AutoDeny => "auto-deny",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ApprovalOutcome {
    pub decision: ApprovalDecision,
    pub reason: String,
    pub edited_params: String,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ApprovalDecision {
    Approve,
    Deny,
    Skip,
    Retry,
    EditPlan,
}

#[async_trait]
pub trait ApprovalGate: Send {
    async fn request(
        &mut self,
        action: &PlannedAction,
        tier: RiskTier,
        params: &str,
    ) -> Result<ApprovalOutcome, String>;
}

#[derive(Debug, Clone)]
pub struct CliApprovalGate {
    mode: ApprovalMode,
}

impl CliApprovalGate {
    pub fn new(mode: ApprovalMode) -> Self {
        Self { mode }
    }
}

#[async_trait]
impl ApprovalGate for CliApprovalGate {
    async fn request(
        &mut self,
        action: &PlannedAction,
        tier: RiskTier,
        _params: &str,
    ) -> Result<ApprovalOutcome, String> {
        match self.mode {
            ApprovalMode::AutoApprove => Ok(ApprovalOutcome {
                decision: ApprovalDecision::Approve,
                reason: "auto-approve mode".to_string(),
                edited_params: String::new(),
            }),
            ApprovalMode::AutoDeny => Ok(ApprovalOutcome {
                decision: ApprovalDecision::Deny,
                reason: "auto-deny mode".to_string(),
                edited_params: String::new(),
            }),
            ApprovalMode::Prompt => {
                let action = action.clone();
                tokio::task::spawn_blocking(move || prompt_approval(&action, tier))
                    .await
                    .map_err(|e| e.to_string())?
            }
        }
    }
}

fn prompt_approval(action: &PlannedAction, tier: RiskTier) -> Result<ApprovalOutcome, String> {
    println!("Approval required");
    println!("- tool: {}", action.tool);
    println!("- risk: {}", tier_label(tier));

    match tier {
        RiskTier::High => {
            println!("Type one of: approve | deny | skip | retry | edit");
            let mut token = String::new();
            io::stdin().read_line(&mut token).map_err(|e| e.to_string())?;
            let action_decision = parse_decision(token.trim());
            if action_decision == ApprovalDecision::Deny {
                return Ok(ApprovalOutcome {
                    decision: ApprovalDecision::Deny,
                    reason: "operator denied".to_string(),
                    edited_params: String::new(),
                });
            }

            if action_decision == ApprovalDecision::Skip {
                return Ok(ApprovalOutcome {
                    decision: ApprovalDecision::Skip,
                    reason: "operator skipped action".to_string(),
                    edited_params: String::new(),
                });
            }

            if action_decision == ApprovalDecision::Retry {
                return Ok(ApprovalOutcome {
                    decision: ApprovalDecision::Retry,
                    reason: "operator requested retry with safer plan".to_string(),
                    edited_params: String::new(),
                });
            }

            if action_decision == ApprovalDecision::EditPlan {
                print!("Edited params: ");
                io::stdout().flush().map_err(|e| e.to_string())?;
                let mut edited = String::new();
                io::stdin().read_line(&mut edited).map_err(|e| e.to_string())?;
                let edited = edited.trim().to_string();
                if edited.is_empty() {
                    return Ok(ApprovalOutcome {
                        decision: ApprovalDecision::Deny,
                        reason: "empty edited params".to_string(),
                        edited_params: String::new(),
                    });
                }

                return Ok(ApprovalOutcome {
                    decision: ApprovalDecision::EditPlan,
                    reason: "operator edited plan params".to_string(),
                    edited_params: edited,
                });
            }

            if action_decision != ApprovalDecision::Approve {
                return Ok(ApprovalOutcome {
                    decision: ApprovalDecision::Deny,
                    reason: "typed confirmation failed".to_string(),
                    edited_params: String::new(),
                });
            }

            print!("Reason: ");
            io::stdout().flush().map_err(|e| e.to_string())?;
            let mut reason = String::new();
            io::stdin().read_line(&mut reason).map_err(|e| e.to_string())?;
            let reason = reason.trim().to_string();
            if reason.is_empty() {
                return Ok(ApprovalOutcome {
                    decision: ApprovalDecision::Deny,
                    reason: "missing reason".to_string(),
                    edited_params: String::new(),
                });
            }

            Ok(ApprovalOutcome {
                decision: ApprovalDecision::Approve,
                reason,
                edited_params: String::new(),
            })
        }
        RiskTier::Medium | RiskTier::Low => {
            println!("Choose: approve(a) | deny(d) | skip(s) | retry(r) | edit(e)");
            let mut input = String::new();
            io::stdin().read_line(&mut input).map_err(|e| e.to_string())?;
            let decision = parse_decision(input.trim());

            if decision == ApprovalDecision::EditPlan {
                print!("Edited params: ");
                io::stdout().flush().map_err(|e| e.to_string())?;
                let mut edited = String::new();
                io::stdin().read_line(&mut edited).map_err(|e| e.to_string())?;
                let edited = edited.trim().to_string();
                if edited.is_empty() {
                    return Ok(ApprovalOutcome {
                        decision: ApprovalDecision::Deny,
                        reason: "empty edited params".to_string(),
                        edited_params: String::new(),
                    });
                }

                return Ok(ApprovalOutcome {
                    decision: ApprovalDecision::EditPlan,
                    reason: "operator edited plan params".to_string(),
                    edited_params: edited,
                });
            }

            Ok(ApprovalOutcome {
                decision,
                reason: decision_reason(decision),
                edited_params: String::new(),
            })
        }
    }
}

pub fn parse_decision(raw: &str) -> ApprovalDecision {
    match raw.trim().to_lowercase().as_str() {
        "approve" | "a" | "y" | "yes" | "APPROVE" => ApprovalDecision::Approve,
        "skip" | "s" => ApprovalDecision::Skip,
        "retry" | "r" => ApprovalDecision::Retry,
        "edit" | "e" => ApprovalDecision::EditPlan,
        _ => ApprovalDecision::Deny,
    }
}

fn decision_reason(decision: ApprovalDecision) -> String {
    match decision {
        ApprovalDecision::Approve => "approved by operator".to_string(),
        ApprovalDecision::Deny => "denied by operator".to_string(),
        ApprovalDecision::Skip => "skipped by operator".to_string(),
        ApprovalDecision::Retry => "retry requested by operator".to_string(),
        ApprovalDecision::EditPlan => "plan edited by operator".to_string(),
    }
}

fn tier_label(tier: RiskTier) -> &'static str {
    match tier {
        RiskTier::Low => "low",
        RiskTier::Medium => "medium",
        RiskTier::High => "high",
    }
}
