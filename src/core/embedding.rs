use crate::core::json_util::parse_openai_embedding;
use serde_json::json;
use std::process::Command;

#[derive(Debug, Clone)]
pub struct EmbeddingSettings {
    pub api_url: String,
    pub api_key: String,
    pub model: String,
    pub dimensions: usize,
}

impl EmbeddingSettings {
    pub fn is_ready(&self) -> bool {
        !self.api_url.trim().is_empty()
            && !self.api_key.trim().is_empty()
            && !self.model.trim().is_empty()
            && self.dimensions > 0
    }

    pub fn embed(&self, text: &str) -> Result<Vec<f64>, String> {
        if !self.is_ready() {
            return Err("embedding provider is not configured".to_string());
        }
        if text.trim().is_empty() {
            return Err("embedding input is empty".to_string());
        }

        let payload = json!({
            "model": self.model,
            "input": text,
            "dimensions": self.dimensions
        })
        .to_string();
        let auth = format!("Authorization: Bearer {}", self.api_key.trim());
        let cmd = format!(
            "curl -sS -XPOST {} -H {} -H {} -d {}",
            shell_quote(&self.api_url),
            shell_quote(&auth),
            shell_quote("content-type: application/json"),
            shell_quote(&payload)
        );
        let output = Command::new("sh")
            .arg("-lc")
            .arg(&cmd)
            .output()
            .map_err(|e| e.to_string())?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
        }
        let body = String::from_utf8_lossy(&output.stdout).trim().to_string();
        parse_openai_embedding(&body)
    }
}

fn shell_quote(value: &str) -> String {
    if value.is_empty() {
        return "''".to_string();
    }
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedding_settings_ready_check() {
        let settings = EmbeddingSettings {
            api_url: "http://localhost".to_string(),
            api_key: "key".to_string(),
            model: "text-embedding-3-small".to_string(),
            dimensions: 1536,
        };
        assert!(settings.is_ready());
    }
}
