use crate::core::config::AppConfig;
use crate::core::embedding::EmbeddingSettings;
use crate::core::json_util::parse_json;
use crate::core::sandbox_executor::SandboxdSettings;
use crate::core::sandboxd_policy::{evaluate_production_checklist, validate_sandboxd_transport, CheckStatus};
use crate::core::vector_backend::QdrantVectorBackend;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone)]
pub struct FixtureReport {
    pub name: String,
    pub detail: String,
}

pub fn integration_enabled() -> bool {
    matches!(
        std::env::var("OZR_RUN_INTEGRATION").ok().as_deref(),
        Some("1") | Some("true") | Some("TRUE")
    )
}

pub fn sandboxd_settings_from_config(cfg: &AppConfig) -> SandboxdSettings {
    SandboxdSettings::from_config(cfg)
}

pub fn run_sandboxd_fixture(cfg: &AppConfig) -> Result<FixtureReport, String> {
    let settings = sandboxd_settings_from_config(cfg);
    if !settings.is_ready() {
        return Err(
            "sandboxd fixture requires OZR_SANDBOXD_API_BASE and OZR_SANDBOXD_SANDBOX_ID".to_string(),
        );
    }

    validate_sandboxd_transport(&settings)?;

    let url = format!(
        "{}/v1/sandboxes/{}",
        settings.api_base.trim_end_matches('/'),
        settings.sandbox_id
    );
    let (status, body) = curl_request("GET", &url, auth_headers(&settings), None)?;
    if status == 0 {
        return Err(format!("sandboxd unreachable at {}", settings.api_base));
    }
    if (200..300).contains(&status) {
        parse_json(&body)
            .map_err(|e| format!("sandboxd response not json (status={}): {}", status, e))?;
    }

    let checklist = evaluate_production_checklist(cfg.feature_sandboxd_executor, &settings);
    let fail_count = checklist
        .iter()
        .filter(|item| item.status == CheckStatus::Fail)
        .count();

    Ok(FixtureReport {
        name: "sandboxd".to_string(),
        detail: format!(
            "sandbox_id={} http_status={} checklist_fail={}",
            settings.sandbox_id, status, fail_count
        ),
    })
}

pub fn run_qdrant_fixture(cfg: &AppConfig) -> Result<FixtureReport, String> {
    if cfg.feature_vector_backend != "qdrant" {
        return Err("qdrant fixture requires OZR_FEATURE_VECTOR_BACKEND=qdrant".to_string());
    }

    let collection = integration_collection_name(&cfg.qdrant_collection);
    let embeddings = build_embedding_settings(cfg);
    let backend = QdrantVectorBackend {
        base_url: cfg.qdrant_url.trim_end_matches('/').to_string(),
        collection: collection.clone(),
        api_key: cfg.qdrant_api_key.clone(),
        embeddings,
    };

    if !backend.is_ready() {
        return Err("qdrant fixture requires OZR_QDRANT_URL and collection name".to_string());
    }

    let marker = format!("ozr-integration-marker-{}", unix_now());
    backend.ensure_collection()?;
    backend.upsert(&marker, "integration", "vector")?;
    let hits = backend.search(&marker, 3)?;
    let matched = hits.iter().any(|hit| hit.content.contains(&marker));
    if !matched {
        return Err(format!(
            "qdrant roundtrip failed to recall marker in collection={}",
            collection
        ));
    }

    let _ = delete_collection(&backend);

    Ok(FixtureReport {
        name: "qdrant".to_string(),
        detail: format!(
            "collection={} hits={} embeddings={}",
            collection,
            hits.len(),
            cfg.vector_embeddings
        ),
    })
}

fn build_embedding_settings(cfg: &AppConfig) -> Option<EmbeddingSettings> {
    if !cfg.vector_embeddings {
        return None;
    }
    let api_key = if !cfg.embedding_api_key.trim().is_empty() {
        cfg.embedding_api_key.clone()
    } else {
        cfg.llm_api_key.clone()
    };
    Some(EmbeddingSettings {
        api_url: cfg.embedding_api_url.clone(),
        api_key,
        model: cfg.embedding_model.clone(),
        dimensions: cfg.embedding_dimensions,
    })
}

fn integration_collection_name(base: &str) -> String {
    let prefix = if base.trim().is_empty() {
        "ozr_integration"
    } else {
        base.trim()
    };
    format!("{}_{}", prefix, unix_now())
}

fn delete_collection(backend: &QdrantVectorBackend) -> Result<(), String> {
    let url = format!(
        "{}/collections/{}",
        backend.base_url, backend.collection
    );
    let _ = curl_request("DELETE", &url, qdrant_headers(backend), None)?;
    Ok(())
}

fn auth_headers(settings: &SandboxdSettings) -> Vec<(String, String)> {
    if settings.api_token.trim().is_empty() {
        Vec::new()
    } else {
        vec![(
            "Authorization".to_string(),
            format!("Bearer {}", settings.api_token.trim()),
        )]
    }
}

fn qdrant_headers(backend: &QdrantVectorBackend) -> Vec<(String, String)> {
    if backend.api_key.trim().is_empty() {
        Vec::new()
    } else {
        vec![("api-key".to_string(), backend.api_key.trim().to_string())]
    }
}

fn curl_request(
    method: &str,
    url: &str,
    headers: Vec<(String, String)>,
    body: Option<&str>,
) -> Result<(i32, String), String> {
    let mut header_args = String::new();
    for (name, value) in headers {
        header_args.push_str(&format!(" -H {}", shell_quote(&format!("{}: {}", name, value))));
    }
    let data_flag = match body {
        Some(payload) => format!(
            " -H {} -d {}",
            shell_quote("content-type: application/json"),
            shell_quote(payload)
        ),
        None => String::new(),
    };
    let cmd = format!(
        "curl -sS -o /tmp/ozr-integration-body -w '%{{http_code}}' -X{} {}{}{}",
        method,
        shell_quote(url),
        header_args,
        data_flag
    );
    let output = Command::new("sh")
        .arg("-lc")
        .arg(&cmd)
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }

    let status_text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let status = status_text.parse::<i32>().unwrap_or(0);
    let response_body = std::fs::read_to_string("/tmp/ozr-integration-body").unwrap_or_default();
    Ok((status, response_body))
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integration_flag_defaults_off() {
        std::env::remove_var("OZR_RUN_INTEGRATION");
        assert!(!integration_enabled());
    }
}
