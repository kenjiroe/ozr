use crate::core::sandbox_executor::SandboxdSettings;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckStatus {
    Pass,
    Warn,
    Fail,
}

#[derive(Debug, Clone)]
pub struct ChecklistItem {
    pub id: String,
    pub status: CheckStatus,
    pub detail: String,
    pub remediation: String,
}

pub fn validate_sandboxd_transport(settings: &SandboxdSettings) -> Result<(), String> {
    if settings.require_auth && settings.api_token.trim().is_empty() {
        return Err("sandboxd auth policy requires OZR_SANDBOXD_API_TOKEN to be set".to_string());
    }
    if settings.https_only && !settings.api_base.to_lowercase().starts_with("https://") {
        return Err("sandboxd transport policy requires HTTPS OZR_SANDBOXD_API_BASE".to_string());
    }
    Ok(())
}

pub fn policy_summary(settings: &SandboxdSettings) -> String {
    format!(
        "sandboxd_policy require_auth={} https_only={} token_set={}",
        settings.require_auth,
        settings.https_only,
        !settings.api_token.trim().is_empty()
    )
}

pub fn evaluate_production_checklist(
    executor_enabled: bool,
    settings: &SandboxdSettings,
) -> Vec<ChecklistItem> {
    let mut items = Vec::new();

    items.push(check(
        "executor-enabled",
        if executor_enabled {
            CheckStatus::Pass
        } else {
            CheckStatus::Warn
        },
        if executor_enabled {
            "OZR_FEATURE_SANDBOXD_EXECUTOR is enabled".to_string()
        } else {
            "sandboxd executor is disabled; checklist assumes production executor usage".to_string()
        },
        "Set OZR_FEATURE_SANDBOXD_EXECUTOR=true for isolated high-risk execution".to_string(),
    ));

    items.push(check(
        "sandbox-id",
        if settings.sandbox_id.trim().is_empty() {
            CheckStatus::Fail
        } else {
            CheckStatus::Pass
        },
        format!("sandbox_id={}", empty_label(&settings.sandbox_id)),
        "Set OZR_SANDBOXD_SANDBOX_ID to a dedicated production sandbox".to_string(),
    ));

    items.push(check(
        "api-token",
        if settings.api_token.trim().is_empty() {
            CheckStatus::Fail
        } else {
            CheckStatus::Pass
        },
        "OZR_SANDBOXD_API_TOKEN is set".to_string(),
        "Provide a scoped bearer token for sandboxd API access".to_string(),
    ));

    items.push(check(
        "require-auth",
        if settings.require_auth {
            CheckStatus::Pass
        } else {
            CheckStatus::Warn
        },
        format!("OZR_SANDBOXD_REQUIRE_AUTH={}", settings.require_auth),
        "Enable OZR_SANDBOXD_REQUIRE_AUTH=true in production".to_string(),
    ));

    items.push(check(
        "https-transport",
        if settings.https_only {
            CheckStatus::Pass
        } else if settings.api_base.to_lowercase().starts_with("https://") {
            CheckStatus::Warn
        } else {
            CheckStatus::Fail
        },
        format!("api_base={}", settings.api_base),
        "Use HTTPS api base and set OZR_SANDBOXD_HTTPS_ONLY=true".to_string(),
    ));

    items.push(check(
        "localhost-api",
        if is_localhost_base(&settings.api_base) {
            CheckStatus::Warn
        } else {
            CheckStatus::Pass
        },
        format!("api_base={}", settings.api_base),
        "Point OZR_SANDBOXD_API_BASE to production endpoint behind TLS termination".to_string(),
    ));

    items.push(check(
        "poll-budget",
        if settings.poll_attempts >= 5 && settings.poll_max_interval_ms >= 1_000 {
            CheckStatus::Pass
        } else {
            CheckStatus::Warn
        },
        format!(
            "poll_attempts={} poll_max_interval_ms={}",
            settings.poll_attempts, settings.poll_max_interval_ms
        ),
        "Increase OZR_SANDBOXD_POLL_ATTEMPTS and OZR_SANDBOXD_POLL_MAX_INTERVAL_MS for long tasks"
            .to_string(),
    ));

    items.push(check(
        "event-capture",
        if settings.capture_events {
            CheckStatus::Pass
        } else {
            CheckStatus::Warn
        },
        format!("OZR_SANDBOXD_CAPTURE_EVENTS={}", settings.capture_events),
        "Enable OZR_SANDBOXD_CAPTURE_EVENTS=true for replay/debug audit artifacts".to_string(),
    ));

    items.push(check(
        "event-timeout",
        if !settings.capture_events || settings.events_max_time_s >= 2 {
            CheckStatus::Pass
        } else {
            CheckStatus::Warn
        },
        format!(
            "OZR_SANDBOXD_EVENTS_MAX_TIME_S={}",
            settings.events_max_time_s
        ),
        "Set OZR_SANDBOXD_EVENTS_MAX_TIME_S>=2 when event capture is enabled".to_string(),
    ));

    items.push(check(
        "token-rotation",
        CheckStatus::Warn,
        "Manual policy: rotate OZR_SANDBOXD_API_TOKEN on schedule".to_string(),
        "Rotate bearer tokens regularly and revoke old tokens at the identity provider".to_string(),
    ));

    items.push(check(
        "egress-control",
        CheckStatus::Warn,
        "Manual policy: restrict sandbox egress to required domains".to_string(),
        "Configure sandboxd network policy/firewall to deny broad outbound access".to_string(),
    ));

    items
}

pub fn render_checklist_markdown(items: &[ChecklistItem], policy: &str) -> String {
    let pass = items
        .iter()
        .filter(|i| i.status == CheckStatus::Pass)
        .count();
    let warn = items
        .iter()
        .filter(|i| i.status == CheckStatus::Warn)
        .count();
    let fail = items
        .iter()
        .filter(|i| i.status == CheckStatus::Fail)
        .count();

    let mut out = String::from("# Sandboxd Production Checklist\n\n");
    out.push_str(&format!("- policy: `{}`\n", policy));
    out.push_str(&format!(
        "- summary: pass={} warn={} fail={}\n\n",
        pass, warn, fail
    ));
    out.push_str("## Checks\n\n");
    out.push_str("| ID | Status | Detail | Remediation |\n");
    out.push_str("|---|---|---|---|\n");
    for item in items {
        out.push_str(&format!(
            "| {} | {} | {} | {} |\n",
            item.id,
            status_label(&item.status),
            item.detail.replace('|', "\\|"),
            item.remediation.replace('|', "\\|")
        ));
    }
    out.push_str("\n## Ops Notes\n\n");
    out.push_str("- Terminate TLS at ingress and keep sandboxd API private where possible.\n");
    out.push_str("- Store tokens in secret manager; avoid committing `.ozr/config.env` secrets.\n");
    out.push_str("- Review `.ozr/audit/sandboxd-events-*.json` after high-risk runs.\n");
    out
}

pub fn checklist_template() -> &'static str {
    "# Sandboxd Production Checklist (Template)\n\n\
     Use `ozr sandboxd-checklist` to evaluate current config.\n\n\
     ## Required in production\n\n\
     - OZR_FEATURE_SANDBOXD_EXECUTOR=true\n\
     - OZR_SANDBOXD_SANDBOX_ID=<dedicated sandbox>\n\
     - OZR_SANDBOXD_API_TOKEN=<scoped bearer token>\n\
     - OZR_SANDBOXD_REQUIRE_AUTH=true\n\
     - OZR_SANDBOXD_HTTPS_ONLY=true\n\
     - OZR_SANDBOXD_API_BASE=https://<sandboxd-host>\n\n\
     ## Recommended\n\n\
     - OZR_SANDBOXD_CAPTURE_EVENTS=true\n\
     - OZR_SANDBOXD_EVENTS_MAX_TIME_S=5\n\
     - Rotate API tokens on schedule\n\
     - Restrict sandbox egress to required domains\n"
}

fn check(id: &str, status: CheckStatus, detail: String, remediation: String) -> ChecklistItem {
    ChecklistItem {
        id: id.to_string(),
        status,
        detail,
        remediation,
    }
}

fn status_label(status: &CheckStatus) -> &'static str {
    match status {
        CheckStatus::Pass => "PASS",
        CheckStatus::Warn => "WARN",
        CheckStatus::Fail => "FAIL",
    }
}

fn empty_label(value: &str) -> String {
    if value.trim().is_empty() {
        "<empty>".to_string()
    } else {
        value.to_string()
    }
}

fn is_localhost_base(api_base: &str) -> bool {
    let lower = api_base.to_lowercase();
    lower.contains("127.0.0.1") || lower.contains("localhost")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_settings() -> SandboxdSettings {
        SandboxdSettings {
            api_base: "https://sandboxd.prod.example".to_string(),
            api_token: "token".to_string(),
            sandbox_id: "sb-prod".to_string(),
            agent: "opencode".to_string(),
            poll_attempts: 10,
            poll_interval_ms: 700,
            poll_backoff_multiplier: 2,
            poll_max_interval_ms: 4_000,
            capture_events: true,
            events_max_time_s: 5,
            require_auth: true,
            https_only: true,
        }
    }

    #[test]
    fn rejects_missing_token_when_required() {
        let mut settings = base_settings();
        settings.require_auth = true;
        settings.api_token.clear();
        assert!(validate_sandboxd_transport(&settings).is_err());
    }

    #[test]
    fn rejects_http_when_https_only() {
        let mut settings = base_settings();
        settings.api_base = "http://127.0.0.1:9090".to_string();
        settings.https_only = true;
        assert!(validate_sandboxd_transport(&settings).is_err());
    }

    #[test]
    fn production_checklist_flags_missing_token() {
        let mut settings = base_settings();
        settings.api_token.clear();
        let items = evaluate_production_checklist(true, &settings);
        assert!(items
            .iter()
            .any(|item| item.id == "api-token" && item.status == CheckStatus::Fail));
    }
}
