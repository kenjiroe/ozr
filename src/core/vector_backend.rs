use crate::core::embedding::EmbeddingSettings;
use crate::core::json_util::{qdrant_payload_hits, qdrant_search_hits};
use crate::core::memory_sqlite::{trust_for_source, MemoryHit};
use serde_json::{json, Value};
use std::process::Command;

#[derive(Debug, Clone)]
pub enum VectorBackend {
    None,
    Qdrant(QdrantVectorBackend),
}

impl VectorBackend {
    pub fn from_config(
        backend: &str,
        url: &str,
        collection: &str,
        api_key: &str,
        embeddings: Option<EmbeddingSettings>,
    ) -> Self {
        if backend == "qdrant" {
            VectorBackend::Qdrant(QdrantVectorBackend {
                base_url: url.trim_end_matches('/').to_string(),
                collection: collection.to_string(),
                api_key: api_key.to_string(),
                embeddings,
            })
        } else {
            VectorBackend::None
        }
    }

    pub fn upsert(&self, content: &str, source: &str, layer: &str) -> Result<(), String> {
        match self {
            VectorBackend::None => Ok(()),
            VectorBackend::Qdrant(q) => q.upsert(content, source, layer),
        }
    }

    pub fn recall(&self, query: &str, limit: usize) -> Result<Vec<MemoryHit>, String> {
        match self {
            VectorBackend::None => Ok(Vec::new()),
            VectorBackend::Qdrant(q) => q.search(query, limit),
        }
    }
}

#[derive(Debug, Clone)]
pub struct QdrantVectorBackend {
    pub base_url: String,
    pub collection: String,
    pub api_key: String,
    pub embeddings: Option<EmbeddingSettings>,
}

impl QdrantVectorBackend {
    pub fn is_ready(&self) -> bool {
        !self.base_url.is_empty() && !self.collection.is_empty()
    }

    fn vector_size(&self) -> usize {
        self.embeddings
            .as_ref()
            .map(|settings| settings.dimensions)
            .unwrap_or(1)
    }

    pub fn ensure_collection(&self) -> Result<(), String> {
        if !self.is_ready() {
            return Err("qdrant backend is not configured".to_string());
        }
        let url = format!("{}/collections/{}", self.base_url, self.collection);
        let payload = json!({
            "vectors": {"size": self.vector_size(), "distance": "Cosine"}
        })
        .to_string();
        let _ = self.request("PUT", &url, &payload)?;
        Ok(())
    }

    pub fn upsert(&self, content: &str, source: &str, layer: &str) -> Result<(), String> {
        if content.trim().is_empty() {
            return Ok(());
        }
        self.ensure_collection()?;
        let point_id = stable_point_id(content, source);
        let vector = self.build_vector(content)?;
        let url = format!(
            "{}/collections/{}/points?wait=true",
            self.base_url, self.collection
        );
        let payload = json!({
            "points": [{
                "id": point_id,
                "vector": vector,
                "payload": {
                    "content": content,
                    "source": source,
                    "layer": layer
                }
            }]
        })
        .to_string();
        self.request("PUT", &url, &payload)?;
        Ok(())
    }

    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<MemoryHit>, String> {
        if query.trim().is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        if !self.is_ready() {
            return Ok(Vec::new());
        }

        if self
            .embeddings
            .as_ref()
            .is_some_and(EmbeddingSettings::is_ready)
        {
            let vector = self.build_vector(query)?;
            let url = format!(
                "{}/collections/{}/points/search",
                self.base_url, self.collection
            );
            let payload = json!({
                "vector": vector,
                "limit": limit,
                "with_payload": true
            })
            .to_string();
            let body = self.request("POST", &url, &payload)?;
            let raw_hits = qdrant_search_hits(&body, limit)?;
            return Ok(raw_hits
                .into_iter()
                .map(|(content, source, layer, score)| MemoryHit {
                    layer: if layer.is_empty() {
                        "vector".to_string()
                    } else {
                        layer
                    },
                    source: if source.is_empty() {
                        "qdrant".to_string()
                    } else {
                        source
                    },
                    content,
                    trust_score: trust_for_source("inferred"),
                    relevance: score.clamp(0.0, 1.0),
                })
                .collect());
        }

        let url = format!(
            "{}/collections/{}/points/scroll",
            self.base_url, self.collection
        );
        let payload = json!({
            "limit": limit,
            "with_payload": true,
            "filter": {
                "must": [{
                    "key": "content",
                    "match": {"text": query}
                }]
            }
        })
        .to_string();
        let body = self.request("POST", &url, &payload)?;
        let raw_hits = qdrant_payload_hits(&body, query, limit)?;
        Ok(raw_hits
            .into_iter()
            .map(|(content, source, layer)| MemoryHit {
                layer: if layer.is_empty() {
                    "vector".to_string()
                } else {
                    layer
                },
                source: if source.is_empty() {
                    "qdrant".to_string()
                } else {
                    source
                },
                content: content.clone(),
                trust_score: trust_for_source("inferred"),
                relevance: substring_relevance(&content, query),
            })
            .collect())
    }

    fn build_vector(&self, text: &str) -> Result<Value, String> {
        if let Some(settings) = &self.embeddings {
            if settings.is_ready() {
                let vector = settings.embed(text)?;
                return Ok(Value::Array(vector.into_iter().map(Value::from).collect()));
            }
        }
        Ok(json!([0.0]))
    }

    fn request(&self, method: &str, url: &str, payload: &str) -> Result<String, String> {
        let auth = if self.api_key.trim().is_empty() {
            String::new()
        } else {
            format!(
                "-H {} ",
                shell_quote(&format!("api-key: {}", self.api_key.trim()))
            )
        };
        let data_flag = if payload.is_empty() {
            String::new()
        } else {
            format!("-d {} ", shell_quote(payload))
        };
        let cmd = format!(
            "curl -sS -X{} {} {} {}",
            method,
            shell_quote(url),
            auth,
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
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }
}

fn stable_point_id(content: &str, source: &str) -> u64 {
    let mut hash = 1469598103934665603u64;
    for byte in format!("{}::{}", source, content).bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(1099511628211);
    }
    hash
}

fn substring_relevance(content: &str, query: &str) -> f64 {
    let lower_content = content.to_lowercase();
    let lower_query = query.to_lowercase();
    if lower_query.is_empty() {
        return 0.0;
    }
    if lower_content.contains(&lower_query) {
        1.0
    } else {
        let words: Vec<&str> = lower_query.split_whitespace().collect();
        if words.is_empty() {
            return 0.0;
        }
        let matched = words
            .iter()
            .filter(|word| lower_content.contains(*word))
            .count();
        matched as f64 / words.len() as f64
    }
}

fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_point_id_is_deterministic() {
        let a = stable_point_id("hello", "src");
        let b = stable_point_id("hello", "src");
        assert_eq!(a, b);
    }
}
