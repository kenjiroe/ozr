use crate::core::http_client::{self, shell_quote};
use crate::core::json_util::{
    build_sandboxd_task_payload, parse_sandboxd_sse_capture, parse_sandboxd_task_status,
    parse_sandboxd_task_submit, serialize_sandboxd_event_capture, SandboxdTaskStatus,
};
use crate::core::mcp_client::McpClient;
use crate::core::policy::{ActionKind, PlannedAction};
use crate::core::sandboxd_policy::{policy_summary, validate_sandboxd_transport};
use async_trait::async_trait;
use std::fs;
use std::path::Path;

#[async_trait]
pub trait SandboxExecutor: Send + Sync {
    async fn execute(
        &self,
        action: &PlannedAction,
        params: &str,
        mcp: &dyn McpClient,
    ) -> Result<String, String>;
}

#[derive(Debug, Default, Clone)]
pub struct HostExecutor;

#[async_trait]
impl SandboxExecutor for HostExecutor {
    async fn execute(
        &self,
        action: &PlannedAction,
        params: &str,
        mcp: &dyn McpClient,
    ) -> Result<String, String> {
        mcp.call_tool(&action.tool, params).await
    }
}

#[derive(Debug, Default, Clone)]
pub struct SandboxdExecutor;

#[derive(Debug, Clone)]
pub struct SandboxdSettings {
    pub api_base: String,
    pub api_token: String,
    pub sandbox_id: String,
    pub agent: String,
    pub poll_attempts: usize,
    pub poll_interval_ms: u64,
    pub poll_backoff_multiplier: u64,
    pub poll_max_interval_ms: u64,
    pub capture_events: bool,
    pub events_max_time_s: u64,
    pub require_auth: bool,
    pub https_only: bool,
}

impl SandboxdSettings {
    pub fn is_ready(&self) -> bool {
        !self.api_base.is_empty() && !self.sandbox_id.is_empty()
    }
}

#[derive(Debug, Clone)]
pub struct SandboxdApiExecutor {
    settings: SandboxdSettings,
}

impl SandboxdApiExecutor {
    pub fn new(settings: SandboxdSettings) -> Self {
        Self { settings }
    }
}

#[async_trait]
impl SandboxExecutor for SandboxdExecutor {
    async fn execute(
        &self,
        action: &PlannedAction,
        params: &str,
        mcp: &dyn McpClient,
    ) -> Result<String, String> {
        match action.kind {
            ActionKind::Read => mcp.call_tool(&action.tool, params).await,
            ActionKind::Write | ActionKind::Shell | ActionKind::Network => Ok(format!(
                "sandboxd-stub executed tool={} params={}",
                action.tool, params
            )),
        }
    }
}

#[async_trait]
impl SandboxExecutor for SandboxdApiExecutor {
    async fn execute(
        &self,
        action: &PlannedAction,
        params: &str,
        mcp: &dyn McpClient,
    ) -> Result<String, String> {
        match action.kind {
            ActionKind::Read => mcp.call_tool(&action.tool, params).await,
            ActionKind::Write | ActionKind::Shell | ActionKind::Network => {
                if !self.settings.is_ready() {
                    return Ok(format!(
                        "sandboxd_api_not_ready tool={} params={}",
                        action.tool, params
                    ));
                }
                validate_sandboxd_transport(&self.settings)?;

                let prompt = format!("tool={} params={}", action.tool, params);
                let payload = build_sandboxd_task_payload(&prompt, &self.settings.agent);
                let submit_body = submit_task(&self.settings, &payload).await?;
                let submit = parse_sandboxd_task_submit(&submit_body)?;
                let task_id = submit.id.unwrap_or_default();
                if task_id.is_empty() {
                    return Ok(format!("sandboxd_submit_only: {}", submit_body));
                }

                let task_payload = poll_task_status(&self.settings, &task_id).await?;
                let status = parse_sandboxd_task_status(&task_payload)?;
                let normalized = format_task_status(&task_id, &status);
                let policy = policy_summary(&self.settings);

                if self.settings.capture_events {
                    let events = capture_events(&self.settings, &task_id).await?;
                    Ok(format!(
                        "sandboxd_task={}\npolicy={}\nstatus_payload={}\nnormalized={}\nevents={}",
                        task_id, policy, task_payload, normalized, events
                    ))
                } else {
                    Ok(format!(
                        "sandboxd_task={}\npolicy={}\nstatus_payload={}\nnormalized={}",
                        task_id, policy, task_payload, normalized
                    ))
                }
            }
        }
    }
}

async fn submit_task(settings: &SandboxdSettings, payload: &str) -> Result<String, String> {
    let url = format!(
        "{}/v1/sandboxes/{}/tasks",
        settings.api_base.trim_end_matches('/'),
        settings.sandbox_id
    );
    let output = curl_json("POST", &url, settings, Some(payload)).await?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(format_curl_error(&output))
    }
}

async fn poll_task_status(settings: &SandboxdSettings, task_id: &str) -> Result<String, String> {
    let url = format!(
        "{}/v1/sandboxes/{}/tasks/{}",
        settings.api_base.trim_end_matches('/'),
        settings.sandbox_id,
        task_id
    );

    let attempts = if settings.poll_attempts == 0 {
        1
    } else {
        settings.poll_attempts
    };

    let mut poll_delay_ms = if settings.poll_interval_ms == 0 {
        100
    } else {
        settings.poll_interval_ms
    };
    let backoff = if settings.poll_backoff_multiplier < 1 {
        1
    } else {
        settings.poll_backoff_multiplier
    };
    let max_delay = if settings.poll_max_interval_ms == 0 {
        poll_delay_ms
    } else {
        settings.poll_max_interval_ms
    };

    let mut last_payload = String::new();
    for _ in 0..attempts {
        let output = curl_json("GET", &url, settings, None).await?;
        if !output.status.success() {
            return Err(format_curl_error(&output));
        }

        last_payload = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let status = parse_sandboxd_task_status(&last_payload)
            .map(|task| task.status.unwrap_or_default().to_lowercase())
            .unwrap_or_default();
        if is_terminal_status(&status) {
            return Ok(last_payload);
        }

        http_client::sleep_ms(poll_delay_ms).await;
        poll_delay_ms = poll_delay_ms.saturating_mul(backoff).min(max_delay);
    }

    Ok(last_payload)
}

async fn capture_events(settings: &SandboxdSettings, task_id: &str) -> Result<String, String> {
    let url = format!(
        "{}/v1/sandboxes/{}/tasks/{}/events",
        settings.api_base.trim_end_matches('/'),
        settings.sandbox_id,
        task_id
    );
    let auth = auth_header_arg(settings);
    let cmd = format!(
        "curl -sS --max-time {} {} {} | tail -n 40",
        settings.events_max_time_s,
        shell_quote(&url),
        auth
    );
    let output = http_client::shell_output(&cmd).await?;

    if output.status.success() {
        let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let capture = parse_sandboxd_sse_capture(&raw);
        let encoded = serialize_sandboxd_event_capture(&capture);
        let output_path = format!(".ozr/audit/sandboxd-events-{}.json", task_id);
        if let Some(parent) = Path::new(&output_path).parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::write(&output_path, &encoded);
        Ok(encoded)
    } else {
        Err(format_curl_error(&output))
    }
}

fn format_task_status(task_id: &str, status: &SandboxdTaskStatus) -> String {
    format!(
        "task_id={} status={} result={} error={}",
        task_id,
        empty_as(status.status.as_deref().unwrap_or(""), "unknown"),
        empty_as(&status.result_text(), "n/a"),
        empty_as(&status.error_text(), "n/a")
    )
}

async fn curl_json(
    method: &str,
    url: &str,
    settings: &SandboxdSettings,
    payload: Option<&str>,
) -> Result<std::process::Output, String> {
    validate_sandboxd_transport(settings)?;
    let auth = auth_header_arg(settings);
    let data_flag = match payload {
        Some(body) => format!(
            "-H {} -d {}",
            shell_quote("content-type: application/json"),
            shell_quote(body)
        ),
        None => String::new(),
    };
    let cmd = format!(
        "curl -sS -X{} {} {} {}",
        method,
        shell_quote(url),
        auth,
        data_flag
    );
    http_client::shell_output(&cmd).await
}

fn format_curl_error(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).trim().to_string()
}

fn empty_as(value: &str, fallback: &str) -> String {
    if value.trim().is_empty() {
        fallback.to_string()
    } else {
        value.to_string()
    }
}

fn is_terminal_status(status: &str) -> bool {
    matches!(
        status,
        "succeeded" | "success" | "completed" | "failed" | "error" | "cancelled"
    )
}

fn auth_header_arg(settings: &SandboxdSettings) -> String {
    if settings.api_token.trim().is_empty() {
        String::new()
    } else {
        let header = format!("Authorization: Bearer {}", settings.api_token.trim());
        format!("-H {}", shell_quote(&header))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_status_values_are_recognized() {
        assert!(is_terminal_status("succeeded"));
        assert!(is_terminal_status("failed"));
        assert!(!is_terminal_status("running"));
    }
}
