use crate::core::approval::ApprovalOutcome;
use crate::core::policy::{ActionKind, RiskTier};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::{Mutex, Notify, RwLock};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Running,
    PendingApproval,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
pub struct PendingApprovalView {
    pub plan_id: String,
    pub tool: String,
    pub action_kind: String,
    pub risk_tier: String,
    pub params: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionView {
    pub session_id: String,
    pub status: SessionStatus,
    pub prompt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending: Option<PendingApprovalView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

struct PendingApprovalState {
    plan_id: String,
    tool: String,
    action_kind: ActionKind,
    risk_tier: RiskTier,
    params: String,
    resolution: Arc<Mutex<Option<ApprovalOutcome>>>,
    notify: Arc<Notify>,
}

struct AgentSession {
    prompt: String,
    status: SessionStatus,
    pending: Option<PendingApprovalState>,
    result: Option<String>,
    error: Option<String>,
}

#[derive(Clone, Default)]
pub struct SessionStore {
    inner: Arc<RwLock<HashMap<String, AgentSession>>>,
}

impl SessionStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn new_session_id() -> String {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .unwrap_or(0);
        format!("sess-{}", ts)
    }

    pub async fn create(&self, session_id: &str, prompt: String) {
        let mut sessions = self.inner.write().await;
        sessions.insert(
            session_id.to_string(),
            AgentSession {
                prompt,
                status: SessionStatus::Running,
                pending: None,
                result: None,
                error: None,
            },
        );
    }

    pub async fn get_view(&self, session_id: &str) -> Option<SessionView> {
        let sessions = self.inner.read().await;
        sessions.get(session_id).map(|session| self.to_view(session_id, session))
    }

    pub async fn register_pending(
        &self,
        session_id: &str,
        plan_id: String,
        tool: String,
        action_kind: ActionKind,
        risk_tier: RiskTier,
        params: String,
    ) -> (Arc<Mutex<Option<ApprovalOutcome>>>, Arc<Notify>) {
        let resolution = Arc::new(Mutex::new(None));
        let notify = Arc::new(Notify::new());
        let mut sessions = self.inner.write().await;
        if let Some(session) = sessions.get_mut(session_id) {
            session.status = SessionStatus::PendingApproval;
            session.pending = Some(PendingApprovalState {
                plan_id,
                tool,
                action_kind,
                risk_tier,
                params,
                resolution: resolution.clone(),
                notify: notify.clone(),
            });
        }
        (resolution, notify)
    }

    pub async fn submit_approval(
        &self,
        session_id: &str,
        outcome: ApprovalOutcome,
    ) -> Result<(), String> {
        let sessions = self.inner.read().await;
        let session = sessions
            .get(session_id)
            .ok_or_else(|| "session not found".to_string())?;
        if session.status != SessionStatus::PendingApproval {
            return Err(format!("session is not pending approval: {:?}", session.status));
        }
        let pending = session
            .pending
            .as_ref()
            .ok_or_else(|| "missing pending approval state".to_string())?;
        {
            let mut slot = pending.resolution.lock().await;
            if slot.is_some() {
                return Err("approval already submitted".to_string());
            }
            *slot = Some(outcome);
        }
        pending.notify.notify_one();
        Ok(())
    }

    pub async fn clear_pending(&self, session_id: &str) {
        let mut sessions = self.inner.write().await;
        if let Some(session) = sessions.get_mut(session_id) {
            session.pending = None;
            if session.status == SessionStatus::PendingApproval {
                session.status = SessionStatus::Running;
            }
        }
    }

    pub async fn complete(&self, session_id: &str, result: String) {
        let mut sessions = self.inner.write().await;
        if let Some(session) = sessions.get_mut(session_id) {
            session.status = SessionStatus::Completed;
            session.result = Some(result);
            session.pending = None;
        }
    }

    pub async fn fail(&self, session_id: &str, error: String) {
        let mut sessions = self.inner.write().await;
        if let Some(session) = sessions.get_mut(session_id) {
            session.status = SessionStatus::Failed;
            session.error = Some(error);
            session.pending = None;
        }
    }

    fn to_view(&self, session_id: &str, session: &AgentSession) -> SessionView {
        SessionView {
            session_id: session_id.to_string(),
            status: session.status,
            prompt: session.prompt.clone(),
            pending: session.pending.as_ref().map(|pending| PendingApprovalView {
                plan_id: pending.plan_id.clone(),
                tool: pending.tool.clone(),
                action_kind: format!("{:?}", pending.action_kind),
                risk_tier: tier_label(pending.risk_tier).to_string(),
                params: pending.params.clone(),
            }),
            result: session.result.clone(),
            error: session.error.clone(),
        }
    }
}

fn tier_label(tier: RiskTier) -> &'static str {
    match tier {
        RiskTier::Low => "low",
        RiskTier::Medium => "medium",
        RiskTier::High => "high",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::approval::{ApprovalDecision, ApprovalOutcome};
    use crate::core::policy::ActionKind;

    #[tokio::test]
    async fn submit_approval_unblocks_waiter() {
        let store = SessionStore::new();
        store.create("sess-1", "test prompt".to_string()).await;
        let (resolution, notify) = store
            .register_pending(
                "sess-1",
                "plan-1".to_string(),
                "run_shell".to_string(),
                ActionKind::Shell,
                RiskTier::High,
                "cmd=ls".to_string(),
            )
            .await;

        let store_wait = store.clone();
        let waiter = tokio::spawn(async move {
            loop {
                if let Some(outcome) = resolution.lock().await.clone() {
                    return outcome;
                }
                notify.notified().await;
            }
        });

        store_wait
            .submit_approval(
                "sess-1",
                ApprovalOutcome {
                    decision: ApprovalDecision::Approve,
                    reason: "ok".to_string(),
                    edited_params: String::new(),
                },
            )
            .await
            .expect("submit approval");

        let outcome = waiter.await.expect("waiter");
        assert_eq!(outcome.decision, ApprovalDecision::Approve);
    }
}
