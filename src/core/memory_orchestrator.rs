use crate::core::memory::MemoryStore;
use crate::core::memory_sqlite::{score_hit, MemoryHit, MemoryIndex};
use crate::core::vector_backend::VectorBackend;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct RecallBudget {
    pub max_items: usize,
    pub max_tokens: usize,
}

impl Default for RecallBudget {
    fn default() -> Self {
        Self {
            max_items: 3,
            max_tokens: 500,
        }
    }
}

#[derive(Debug, Clone)]
pub struct MemoryBundle {
    pub hits: Vec<MemoryHit>,
    pub query: String,
}

pub trait MemoryOrchestrator {
    fn ingest_event(&self, event: &str) -> Result<(), String>;
    fn recall(&self, query: &str, budget: RecallBudget) -> Result<MemoryBundle, String>;
    fn score(&self, bundle: &MemoryBundle, query: &str) -> f64;
    fn format_bundle(&self, bundle: &MemoryBundle) -> String;
    fn compact(&self, session_id: &str) -> Result<String, String>;
}

#[derive(Debug, Clone)]
pub struct LayeredMemoryOrchestrator {
    store: MemoryStore,
    index: Option<MemoryIndex>,
    vector: VectorBackend,
    trust_threshold: f64,
}

impl LayeredMemoryOrchestrator {
    pub fn new(store: MemoryStore) -> Self {
        Self {
            store,
            index: None,
            vector: VectorBackend::None,
            trust_threshold: 0.5,
        }
    }

    pub fn with_sqlite(mut self, trust_threshold: f64) -> Result<Self, String> {
        let index = MemoryIndex::new(self.store.db_path());
        index.ensure_schema()?;
        self.index_markdown_files(&index)?;
        self.index = Some(index);
        self.trust_threshold = trust_threshold;
        Ok(self)
    }

    pub fn from_config(
        store: MemoryStore,
        backend: &str,
        trust_threshold: f64,
        vector: VectorBackend,
    ) -> Result<Self, String> {
        let mut orchestrator = Self::new(store);
        orchestrator.vector = vector;
        if backend == "sqlite" {
            orchestrator = orchestrator.with_sqlite(trust_threshold)?;
        } else {
            orchestrator.trust_threshold = trust_threshold;
        }
        Ok(orchestrator)
    }
}

impl MemoryOrchestrator for LayeredMemoryOrchestrator {
    fn ingest_event(&self, event: &str) -> Result<(), String> {
        self.store.ensure_layout()?;
        let events_path = self.store.root_path().join("sessions").join("events.log");
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(events_path)
            .map_err(|e| e.to_string())?;
        writeln!(file, "{}", event).map_err(|e| e.to_string())?;

        if let Some(index) = &self.index {
            let (event_type, run_id, content) = parse_event(event);
            index.ingest_session_event(run_id.as_deref(), &event_type, &content, None)?;
            maybe_extract_fact(index, event);
            let _ = self.vector.upsert(&content, &event_type, "session");
        }

        Ok(())
    }

    fn recall(&self, query: &str, budget: RecallBudget) -> Result<MemoryBundle, String> {
        self.store.ensure_layout()?;

        let mut hits = Vec::new();

        if let Some(index) = &self.index {
            hits.extend(index.recall_fts(query, budget.max_items)?);
            hits.extend(index.recall_facts(query, self.trust_threshold, budget.max_items)?);
        }

        if hits.len() < budget.max_items {
            hits.extend(self.vector.recall(query, budget.max_items - hits.len())?);
        }

        if hits.len() < budget.max_items {
            let file_hits = recall_from_files(&self.store, query, budget.max_items - hits.len())?;
            hits.extend(file_hits);
        }

        hits.sort_by(|a, b| {
            score_hit(b, query)
                .partial_cmp(&score_hit(a, query))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        hits.truncate(budget.max_items);
        hits = trim_to_token_budget(hits, budget.max_tokens);

        Ok(MemoryBundle {
            hits,
            query: query.to_string(),
        })
    }

    fn score(&self, bundle: &MemoryBundle, query: &str) -> f64 {
        if bundle.hits.is_empty() {
            return 0.0;
        }
        let total: f64 = bundle.hits.iter().map(|hit| score_hit(hit, query)).sum();
        total / bundle.hits.len() as f64
    }

    fn format_bundle(&self, bundle: &MemoryBundle) -> String {
        if bundle.hits.is_empty() {
            return "no_memory_hit".to_string();
        }

        bundle
            .hits
            .iter()
            .map(|hit| {
                format!(
                    "[{}] {} (trust={:.2}, score={:.2}) :: {}",
                    hit.layer,
                    hit.source,
                    hit.trust_score,
                    score_hit(hit, &bundle.query),
                    hit.content
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn compact(&self, session_id: &str) -> Result<String, String> {
        if let Some(index) = &self.index {
            index.compact_session(session_id, 50)
        } else {
            Ok(format!("sqlite backend disabled; no compact for {}", session_id))
        }
    }
}

impl LayeredMemoryOrchestrator {
    fn index_markdown_files(&self, index: &MemoryIndex) -> Result<(), String> {
        let root = self.store.root_path().to_path_buf();
        for subdir in &["project", "feedback"] {
            let dir = root.join(subdir);
            if !dir.exists() {
                continue;
            }
            let mut files = Vec::new();
            collect_files(&dir, &mut files)?;
            for file_path in files {
                if !is_markdown_file(&file_path) {
                    continue;
                }
                let content = fs::read_to_string(&file_path).unwrap_or_default();
                if content.trim().is_empty() {
                    continue;
                }
                index.index_markdown_file(&file_path, &content)?;
            }
        }
        Ok(())
    }
}

fn parse_event(event: &str) -> (String, Option<String>, String) {
    if let Some(rest) = event.strip_prefix("run_id=") {
        if let Some(sep) = rest.find('|') {
            let run_id = &rest[..sep];
            let content = &rest[sep + 1..];
            return ("run".to_string(), Some(run_id.to_string()), content.to_string());
        }
    }
    if let Some(content) = event.strip_prefix("prompt=") {
        return ("prompt".to_string(), None, content.to_string());
    }
    if let Some(content) = event.strip_prefix("run_completed:") {
        return ("run_completed".to_string(), None, content.to_string());
    }
    ("event".to_string(), None, event.to_string())
}

fn maybe_extract_fact(index: &MemoryIndex, event: &str) {
    if let Some(rest) = event.strip_prefix("fact:") {
        if let Some(sep) = rest.find('=') {
            let key = rest[..sep].trim();
            let value = rest[sep + 1..].trim();
            let _ = index.upsert_fact(key, value, "user");
        }
    }
}

fn recall_from_files(store: &MemoryStore, query: &str, limit: usize) -> Result<Vec<MemoryHit>, String> {
    if limit == 0 {
        return Ok(Vec::new());
    }

    let mut files = Vec::new();
    let root = store.root_path().to_path_buf();
    collect_files(&root.join("project"), &mut files)?;
    collect_files(&root.join("sessions"), &mut files)?;
    collect_files(&root.join("feedback"), &mut files)?;

    let needle = query.to_lowercase();
    let mut hits = Vec::new();

    for file_path in &files {
        if hits.len() >= limit {
            break;
        }

        let content = fs::read_to_string(file_path).unwrap_or_default();
        let lower = content.to_lowercase();
        if !lower.contains(&needle) {
            continue;
        }

        let snippet = content
            .lines()
            .find(|line| line.to_lowercase().contains(&needle))
            .unwrap_or("")
            .to_string();

        hits.push(MemoryHit {
            layer: "file".to_string(),
            source: file_path.display().to_string(),
            content: snippet,
            trust_score: crate::core::memory_sqlite::trust_for_source("file"),
            relevance: 1.0,
        });
    }

    Ok(hits)
}

fn trim_to_token_budget(mut hits: Vec<MemoryHit>, max_tokens: usize) -> Vec<MemoryHit> {
    let mut used = 0usize;
    let mut kept = Vec::new();
    for hit in hits.drain(..) {
        let tokens = estimate_tokens(&hit.content);
        if used + tokens > max_tokens && !kept.is_empty() {
            break;
        }
        used += tokens;
        kept.push(hit);
    }
    kept
}

fn estimate_tokens(text: &str) -> usize {
    text.split_whitespace().count().max(1)
}

fn is_markdown_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|ext| ext.to_str()),
        Some("md") | Some("txt") | Some("log")
    )
}

fn collect_files(dir: &Path, files: &mut Vec<std::path::PathBuf>) -> Result<(), String> {
    if !dir.exists() {
        return Ok(());
    }

    for entry in fs::read_dir(dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, files)?;
        } else {
            files.push(path);
        }
    }
    Ok(())
}
