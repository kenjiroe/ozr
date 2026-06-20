use crate::api::state::SessionStore;
use crate::core::approval::{ApprovalGate, ApprovalOutcome};
use crate::core::policy::{PlannedAction, RiskTier};
use async_trait::async_trait;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct ApiApprovalGate {
    session_id: String,
    store: SessionStore,
}

impl ApiApprovalGate {
    pub fn new(session_id: String, store: SessionStore) -> Self {
        Self { session_id, store }
    }
}

#[async_trait]
impl ApprovalGate for ApiApprovalGate {
    async fn request(
        &mut self,
        action: &PlannedAction,
        tier: RiskTier,
        params: &str,
    ) -> Result<ApprovalOutcome, String> {
        let plan_id = new_plan_id();
        let (resolution, notify) = self
            .store
            .register_pending(
                &self.session_id,
                plan_id,
                action.tool.clone(),
                action.kind,
                tier,
                params.to_string(),
            )
            .await;

        loop {
            if let Some(outcome) = resolution.lock().await.clone() {
                self.store.clear_pending(&self.session_id).await;
                return Ok(outcome);
            }
            notify.notified().await;
        }
    }
}

fn new_plan_id() -> String {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    format!("plan-{}", ts)
}
