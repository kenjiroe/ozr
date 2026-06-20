use crate::core::approval::ApprovalMode;
use crate::core::policy::PonytailMode;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub feature_spec_kitty_workflow: bool,
    pub feature_sandboxd_executor: bool,
    pub feature_memory_layered: bool,
    pub feature_vector_backend: String,
    pub ponytail_mode: PonytailMode,
    pub approval_mode: ApprovalMode,
    pub llm_backend: String,
    pub llm_api_url: String,
    pub llm_api_key: String,
    pub llm_model: String,
    pub mcp_backend: String,
    pub mcp_stdio_command: String,
    pub mcp_stdio_args: String,
    pub mcp_stdio_timeout_ms: u64,
    pub mcp_stdio_retry_attempts: usize,
    pub mcp_stdio_framing: String,
    pub spec_kitty_command: String,
    pub sandboxd_api_base: String,
    pub sandboxd_api_token: String,
    pub sandboxd_sandbox_id: String,
    pub sandboxd_agent: String,
    pub sandboxd_poll_attempts: usize,
    pub sandboxd_poll_interval_ms: u64,
    pub sandboxd_poll_backoff_multiplier: u64,
    pub sandboxd_poll_max_interval_ms: u64,
    pub sandboxd_capture_events: bool,
    pub sandboxd_events_max_time_s: u64,
    pub sandboxd_require_auth: bool,
    pub sandboxd_https_only: bool,
    pub memory_recall_limit: usize,
    pub memory_backend: String,
    pub memory_trust_threshold: f64,
    pub memory_recall_token_budget: usize,
    pub qdrant_url: String,
    pub qdrant_collection: String,
    pub qdrant_api_key: String,
    pub vector_embeddings: bool,
    pub embedding_api_url: String,
    pub embedding_api_key: String,
    pub embedding_model: String,
    pub embedding_dimensions: usize,
    pub approval_alert_denial_rate: f64,
    pub approval_alert_retry_rate: f64,
    pub approval_alert_high_risk_share: f64,
    pub budget_max_tokens: usize,
    pub budget_max_iterations: usize,
    pub budget_max_run_seconds: u64,
    pub policy_pack: String,
    pub api_bind: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            feature_spec_kitty_workflow: false,
            feature_sandboxd_executor: false,
            feature_memory_layered: false,
            feature_vector_backend: "none".to_string(),
            ponytail_mode: PonytailMode::Off,
            approval_mode: ApprovalMode::Prompt,
            llm_backend: "mock".to_string(),
            llm_api_url: "https://api.openai.com/v1/chat/completions".to_string(),
            llm_api_key: String::new(),
            llm_model: "gpt-4o-mini".to_string(),
            mcp_backend: "mock".to_string(),
            mcp_stdio_command: String::new(),
            mcp_stdio_args: String::new(),
            mcp_stdio_timeout_ms: 5_000,
            mcp_stdio_retry_attempts: 2,
            mcp_stdio_framing: "ndjson".to_string(),
            spec_kitty_command: "spec-kitty".to_string(),
            sandboxd_api_base: "http://127.0.0.1:9090".to_string(),
            sandboxd_api_token: String::new(),
            sandboxd_sandbox_id: String::new(),
            sandboxd_agent: "opencode".to_string(),
            sandboxd_poll_attempts: 10,
            sandboxd_poll_interval_ms: 700,
            sandboxd_poll_backoff_multiplier: 2,
            sandboxd_poll_max_interval_ms: 4_000,
            sandboxd_capture_events: false,
            sandboxd_events_max_time_s: 2,
            sandboxd_require_auth: false,
            sandboxd_https_only: false,
            memory_recall_limit: 3,
            memory_backend: "sqlite".to_string(),
            memory_trust_threshold: 0.5,
            memory_recall_token_budget: 500,
            qdrant_url: "http://127.0.0.1:6333".to_string(),
            qdrant_collection: "ozr_memory".to_string(),
            qdrant_api_key: String::new(),
            vector_embeddings: false,
            embedding_api_url: "https://api.openai.com/v1/embeddings".to_string(),
            embedding_api_key: String::new(),
            embedding_model: "text-embedding-3-small".to_string(),
            embedding_dimensions: 1536,
            approval_alert_denial_rate: 0.3,
            approval_alert_retry_rate: 0.2,
            approval_alert_high_risk_share: 0.4,
            budget_max_tokens: 2_000,
            budget_max_iterations: 5,
            budget_max_run_seconds: 15,
            policy_pack: "balanced".to_string(),
            api_bind: "127.0.0.1:8080".to_string(),
        }
    }
}

impl AppConfig {
    pub fn from_env() -> Self {
        let file_values = load_config_file(".ozr/config.env");
        Self {
            feature_spec_kitty_workflow: parse_bool(&resolve_value(
                "OZR_FEATURE_SPEC_KITTY_WORKFLOW",
                &file_values,
                "",
            )),
            feature_sandboxd_executor: parse_bool(&resolve_value(
                "OZR_FEATURE_SANDBOXD_EXECUTOR",
                &file_values,
                "",
            )),
            feature_memory_layered: parse_bool(&resolve_value(
                "OZR_FEATURE_MEMORY_LAYERED",
                &file_values,
                "",
            )),
            feature_vector_backend: resolve_value(
                "OZR_FEATURE_VECTOR_BACKEND",
                &file_values,
                "none",
            ),
            ponytail_mode: PonytailMode::from_env(
                &resolve_value("OZR_FEATURE_PONYTAIL_PROFILE", &file_values, ""),
            ),
            approval_mode: ApprovalMode::from_env(
                &resolve_value("OZR_APPROVAL_MODE", &file_values, ""),
            ),
            llm_backend: resolve_value("OZR_LLM_BACKEND", &file_values, "mock"),
            llm_api_url: resolve_value(
                "OZR_LLM_API_URL",
                &file_values,
                "https://api.openai.com/v1/chat/completions",
            ),
            llm_api_key: resolve_value("OZR_LLM_API_KEY", &file_values, ""),
            llm_model: resolve_value("OZR_LLM_MODEL", &file_values, "gpt-4o-mini"),
            mcp_backend: resolve_value("OZR_MCP_BACKEND", &file_values, "mock"),
            mcp_stdio_command: resolve_value("OZR_MCP_STDIO_COMMAND", &file_values, ""),
            mcp_stdio_args: resolve_value("OZR_MCP_STDIO_ARGS", &file_values, ""),
            mcp_stdio_timeout_ms: parse_u64(
                &resolve_value("OZR_MCP_STDIO_TIMEOUT_MS", &file_values, "5000"),
                5_000,
            ),
            mcp_stdio_retry_attempts: parse_usize(
                &resolve_value("OZR_MCP_STDIO_RETRY_ATTEMPTS", &file_values, "2"),
                2,
            ),
            mcp_stdio_framing: resolve_value("OZR_MCP_STDIO_FRAMING", &file_values, "ndjson"),
            spec_kitty_command: resolve_value("OZR_SPEC_KITTY_COMMAND", &file_values, "spec-kitty"),
            sandboxd_api_base: resolve_value(
                "OZR_SANDBOXD_API_BASE",
                &file_values,
                "http://127.0.0.1:9090",
            ),
            sandboxd_api_token: resolve_value("OZR_SANDBOXD_API_TOKEN", &file_values, ""),
            sandboxd_sandbox_id: resolve_value("OZR_SANDBOXD_SANDBOX_ID", &file_values, ""),
            sandboxd_agent: resolve_value("OZR_SANDBOXD_AGENT", &file_values, "opencode"),
            sandboxd_poll_attempts: parse_usize(
                &resolve_value("OZR_SANDBOXD_POLL_ATTEMPTS", &file_values, "10"),
                10,
            ),
            sandboxd_poll_interval_ms: parse_u64(
                &resolve_value("OZR_SANDBOXD_POLL_INTERVAL_MS", &file_values, "700"),
                700,
            ),
            sandboxd_poll_backoff_multiplier: parse_u64(
                &resolve_value(
                    "OZR_SANDBOXD_POLL_BACKOFF_MULTIPLIER",
                    &file_values,
                    "2",
                ),
                2,
            ),
            sandboxd_poll_max_interval_ms: parse_u64(
                &resolve_value(
                    "OZR_SANDBOXD_POLL_MAX_INTERVAL_MS",
                    &file_values,
                    "4000",
                ),
                4_000,
            ),
            sandboxd_capture_events: parse_bool(
                &resolve_value("OZR_SANDBOXD_CAPTURE_EVENTS", &file_values, ""),
            ),
            sandboxd_events_max_time_s: parse_u64(
                &resolve_value("OZR_SANDBOXD_EVENTS_MAX_TIME_S", &file_values, "2"),
                2,
            ),
            sandboxd_require_auth: parse_bool(
                &resolve_value("OZR_SANDBOXD_REQUIRE_AUTH", &file_values, ""),
            ),
            sandboxd_https_only: parse_bool(
                &resolve_value("OZR_SANDBOXD_HTTPS_ONLY", &file_values, ""),
            ),
            memory_recall_limit: parse_usize(
                &resolve_value("OZR_MEMORY_RECALL_LIMIT", &file_values, "3"),
                3,
            ),
            memory_backend: resolve_value("OZR_MEMORY_BACKEND", &file_values, "sqlite"),
            memory_trust_threshold: parse_f64(
                &resolve_value("OZR_MEMORY_TRUST_THRESHOLD", &file_values, "0.5"),
                0.5,
            ),
            memory_recall_token_budget: parse_usize(
                &resolve_value("OZR_MEMORY_RECALL_TOKEN_BUDGET", &file_values, "500"),
                500,
            ),
            qdrant_url: resolve_value("OZR_QDRANT_URL", &file_values, "http://127.0.0.1:6333"),
            qdrant_collection: resolve_value("OZR_QDRANT_COLLECTION", &file_values, "ozr_memory"),
            qdrant_api_key: resolve_value("OZR_QDRANT_API_KEY", &file_values, ""),
            vector_embeddings: parse_bool(
                &resolve_value("OZR_VECTOR_EMBEDDINGS", &file_values, ""),
            ),
            embedding_api_url: resolve_value(
                "OZR_EMBEDDING_API_URL",
                &file_values,
                "https://api.openai.com/v1/embeddings",
            ),
            embedding_api_key: resolve_value("OZR_EMBEDDING_API_KEY", &file_values, ""),
            embedding_model: resolve_value(
                "OZR_EMBEDDING_MODEL",
                &file_values,
                "text-embedding-3-small",
            ),
            embedding_dimensions: parse_usize(
                &resolve_value("OZR_EMBEDDING_DIMENSIONS", &file_values, "1536"),
                1536,
            ),
            approval_alert_denial_rate: parse_f64(
                &resolve_value("OZR_APPROVAL_ALERT_DENIAL_RATE", &file_values, "0.3"),
                0.3,
            ),
            approval_alert_retry_rate: parse_f64(
                &resolve_value("OZR_APPROVAL_ALERT_RETRY_RATE", &file_values, "0.2"),
                0.2,
            ),
            approval_alert_high_risk_share: parse_f64(
                &resolve_value("OZR_APPROVAL_ALERT_HIGH_RISK_SHARE", &file_values, "0.4"),
                0.4,
            ),
            budget_max_tokens: parse_usize(
                &resolve_value("OZR_BUDGET_MAX_TOKENS", &file_values, "2000"),
                2_000,
            ),
            budget_max_iterations: parse_usize(
                &resolve_value("OZR_BUDGET_MAX_ITERATIONS", &file_values, "5"),
                5,
            ),
            budget_max_run_seconds: parse_u64(
                &resolve_value("OZR_BUDGET_MAX_RUN_SECONDS", &file_values, "15"),
                15,
            ),
            policy_pack: resolve_value("OZR_POLICY_PACK", &file_values, "balanced"),
            api_bind: resolve_value("OZR_API_BIND", &file_values, "127.0.0.1:8080"),
        }
    }
}

fn load_config_file(path: &str) -> HashMap<String, String> {
    let cfg_path = Path::new(path);
    if !cfg_path.exists() {
        return HashMap::new();
    }

    let content = match fs::read_to_string(cfg_path) {
        Ok(content) => content,
        Err(_) => return HashMap::new(),
    };

    let mut values = HashMap::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some(eq) = trimmed.find('=') {
            let key = trimmed[..eq].trim();
            let value = trimmed[eq + 1..].trim();
            values.insert(key.to_string(), value.to_string());
        }
    }
    values
}

fn resolve_value(key: &str, file_values: &HashMap<String, String>, default_value: &str) -> String {
    match env::var(key) {
        Ok(value) => value,
        Err(_) => file_values
            .get(key)
            .cloned()
            .unwrap_or_else(|| default_value.to_string()),
    }
}

fn parse_bool(value: &str) -> bool {
    matches!(value.to_lowercase().as_str(), "1" | "true" | "yes" | "on")
}

fn parse_usize(value: &str, default_value: usize) -> usize {
    value.parse::<usize>().unwrap_or(default_value)
}

fn parse_u64(value: &str, default_value: u64) -> u64 {
    value.parse::<u64>().unwrap_or(default_value)
}

fn parse_f64(value: &str, default_value: f64) -> f64 {
    value.parse::<f64>().unwrap_or(default_value)
}
