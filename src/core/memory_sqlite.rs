use rusqlite::{params, Connection};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone)]
pub struct MemoryHit {
    pub layer: String,
    pub source: String,
    pub content: String,
    pub trust_score: f64,
    pub relevance: f64,
}

#[derive(Debug, Clone)]
pub struct MemoryIndex {
    db_path: PathBuf,
}

impl MemoryIndex {
    pub fn new<P: AsRef<Path>>(db_path: P) -> Self {
        Self {
            db_path: db_path.as_ref().to_path_buf(),
        }
    }

    fn open(&self) -> Result<Connection, String> {
        if let Some(parent) = self.db_path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        Connection::open(&self.db_path).map_err(|e| e.to_string())
    }

    pub fn ensure_schema(&self) -> Result<(), String> {
        self.migrate_legacy_if_needed()?;
        let conn = self.open()?;
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS session_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                ts INTEGER NOT NULL,
                run_id TEXT,
                event_type TEXT NOT NULL,
                content TEXT NOT NULL,
                source_path TEXT
            );

            CREATE VIRTUAL TABLE IF NOT EXISTS session_events_fts USING fts5(
                content,
                source_path UNINDEXED,
                run_id UNINDEXED,
                event_type UNINDEXED
            );

            CREATE TABLE IF NOT EXISTS facts (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                fact_key TEXT NOT NULL,
                fact_value TEXT NOT NULL,
                source TEXT NOT NULL,
                trust_score REAL NOT NULL,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_facts_key ON facts(fact_key);

            CREATE TABLE IF NOT EXISTS indexed_sources (
                source_path TEXT PRIMARY KEY
            );
            ",
        )
        .map_err(|e| e.to_string())
    }

    pub fn ingest_session_event(
        &self,
        run_id: Option<&str>,
        event_type: &str,
        content: &str,
        source_path: Option<&str>,
    ) -> Result<(), String> {
        let conn = self.open()?;
        let ts = now_unix() as i64;
        conn.execute(
            "INSERT INTO session_events (ts, run_id, event_type, content, source_path)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![ts, run_id, event_type, content, source_path],
        )
        .map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT INTO session_events_fts (content, source_path, run_id, event_type)
             VALUES (?1, ?2, ?3, ?4)",
            params![content, source_path, run_id, event_type],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn upsert_fact(&self, key: &str, value: &str, source: &str) -> Result<(), String> {
        let conn = self.open()?;
        let now = now_unix() as i64;
        let base_trust = trust_for_source(source);

        let existing: Option<(i64, f64)> = conn
            .query_row(
                "SELECT id, trust_score FROM facts WHERE fact_key = ?1 AND fact_value = ?2",
                params![key, value],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .ok();

        if let Some((id, old_trust)) = existing {
            let merged = (old_trust + base_trust) / 2.0;
            conn.execute(
                "UPDATE facts SET trust_score = ?1, updated_at = ?2, source = ?3 WHERE id = ?4",
                params![merged, now, source, id],
            )
            .map_err(|e| e.to_string())?;
        } else {
            conn.execute(
                "INSERT INTO facts (fact_key, fact_value, source, trust_score, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![key, value, source, base_trust, now, now],
            )
            .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    pub fn recall_fts(&self, query: &str, limit: usize) -> Result<Vec<MemoryHit>, String> {
        let fts_query = build_fts_query(query);
        if fts_query.is_empty() {
            return Ok(Vec::new());
        }

        let conn = self.open()?;
        let mut stmt = conn
            .prepare(
                "SELECT content, source_path, run_id, event_type
                 FROM session_events_fts
                 WHERE session_events_fts MATCH ?1
                 LIMIT ?2",
            )
            .map_err(|e| e.to_string())?;

        let rows = stmt
            .query_map(params![fts_query, limit as i64], |row| {
                let content: String = row.get(0)?;
                let source_path: Option<String> = row.get(1)?;
                let run_id: Option<String> = row.get(2)?;
                let event_type: String = row.get(3)?;
                Ok((content, source_path, run_id, event_type))
            })
            .map_err(|e| e.to_string())?;

        let mut hits = Vec::new();
        for row in rows {
            let (content, source_path, run_id, event_type) = row.map_err(|e| e.to_string())?;
            let source = source_path.or(run_id).unwrap_or(event_type);
            hits.push(MemoryHit {
                layer: "session".to_string(),
                source,
                content: content.clone(),
                trust_score: trust_for_source("system"),
                relevance: substring_relevance(&content, query),
            });
        }
        Ok(hits)
    }

    pub fn recall_facts(
        &self,
        query: &str,
        trust_threshold: f64,
        limit: usize,
    ) -> Result<Vec<MemoryHit>, String> {
        let conn = self.open()?;
        let needle = format!("%{}%", query.to_lowercase());
        let now = now_unix() as i64;

        let mut stmt = conn
            .prepare(
                "SELECT fact_key, fact_value, source, trust_score, updated_at
                 FROM facts
                 WHERE (LOWER(fact_key) LIKE ?1 OR LOWER(fact_value) LIKE ?1)
                 LIMIT ?2",
            )
            .map_err(|e| e.to_string())?;

        let rows = stmt
            .query_map(params![needle, (limit * 3) as i64], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, f64>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            })
            .map_err(|e| e.to_string())?;

        let mut hits = Vec::new();
        for row in rows {
            let (key, value, source, base_trust, updated_at) = row.map_err(|e| e.to_string())?;
            let effective = effective_trust(base_trust, updated_at, now);
            if effective < trust_threshold {
                continue;
            }
            let content = format!("{} = {}", key, value);
            hits.push(MemoryHit {
                layer: "fact".to_string(),
                source,
                content: content.clone(),
                trust_score: effective,
                relevance: substring_relevance(&content, query),
            });
            if hits.len() >= limit {
                break;
            }
        }
        Ok(hits)
    }

    pub fn index_markdown_file(&self, path: &Path, content: &str) -> Result<(), String> {
        let source = path.display().to_string();
        let conn = self.open()?;
        let already_indexed: bool = conn
            .query_row(
                "SELECT 1 FROM indexed_sources WHERE source_path = ?1",
                params![source],
                |_| Ok(true),
            )
            .unwrap_or(false);
        if already_indexed {
            return Ok(());
        }

        for (idx, line) in content.lines().enumerate().take(200) {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            self.ingest_session_event(
                None,
                "markdown",
                trimmed,
                Some(&format!("{}:{}", source, idx + 1)),
            )?;
        }

        conn.execute(
            "INSERT OR IGNORE INTO indexed_sources (source_path) VALUES (?1)",
            params![source],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn compact_session(&self, run_id: &str, limit: usize) -> Result<String, String> {
        let conn = self.open()?;
        let mut stmt = conn
            .prepare(
                "SELECT event_type, content FROM session_events
                 WHERE run_id = ?1 ORDER BY ts ASC LIMIT ?2",
            )
            .map_err(|e| e.to_string())?;

        let rows = stmt
            .query_map(params![run_id, limit as i64], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| e.to_string())?;

        let mut lines = Vec::new();
        for row in rows {
            let (event_type, content) = row.map_err(|e| e.to_string())?;
            lines.push(format!("[{}] {}", event_type, content));
        }

        if lines.is_empty() {
            Ok(format!("no events for run_id={}", run_id))
        } else {
            Ok(lines.join("\n"))
        }
    }

    fn migrate_legacy_if_needed(&self) -> Result<(), String> {
        if !self.db_path.exists() {
            return Ok(());
        }

        let header = fs::read(&self.db_path)
            .map_err(|e| e.to_string())?
            .into_iter()
            .take(16)
            .collect::<Vec<u8>>();
        if header.starts_with(b"SQLite format 3") {
            return Ok(());
        }

        let raw = fs::read_to_string(&self.db_path).map_err(|e| e.to_string())?;
        let legacy = parse_legacy_state(&raw)?;
        let legacy_path = self.db_path.with_extension("db.legacy");
        fs::rename(&self.db_path, &legacy_path).map_err(|e| e.to_string())?;

        self.ensure_schema()?;
        for event in &legacy.events {
            self.ingest_session_event(
                event.run_id.as_deref(),
                &event.event_type,
                &event.content,
                event.source_path.as_deref(),
            )?;
        }
        for fact in &legacy.facts {
            self.upsert_fact(&fact.fact_key, &fact.fact_value, &fact.source)?;
        }
        let conn = self.open()?;
        for source in &legacy.indexed_sources {
            conn.execute(
                "INSERT OR IGNORE INTO indexed_sources (source_path) VALUES (?1)",
                params![source],
            )
            .map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}

pub fn trust_for_source(source: &str) -> f64 {
    match source {
        "user" => 1.0,
        "system" => 0.85,
        "file" => 0.75,
        "inferred" => 0.6,
        _ => 0.5,
    }
}

pub fn effective_trust(base: f64, updated_at: i64, now: i64) -> f64 {
    let age_secs = (now - updated_at).max(0);
    let age_days = age_secs as f64 / 86_400.0;
    let decay = (age_days / 30.0) * 0.1;
    (base - decay).max(0.1)
}

pub fn score_hit(hit: &MemoryHit, query: &str) -> f64 {
    let text_match = substring_relevance(&hit.content, query);
    hit.trust_score * 0.6 + hit.relevance * 0.3 + text_match * 0.1
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

fn build_fts_query(raw: &str) -> String {
    raw.split_whitespace()
        .filter(|token| !token.is_empty())
        .map(|token| format!("\"{}\"", token.replace('"', "")))
        .collect::<Vec<_>>()
        .join(" OR ")
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[derive(Debug, Default)]
struct LegacyState {
    events: Vec<LegacyEvent>,
    facts: Vec<LegacyFact>,
    indexed_sources: HashSet<String>,
}

#[derive(Debug)]
struct LegacyEvent {
    run_id: Option<String>,
    event_type: String,
    content: String,
    source_path: Option<String>,
}

#[derive(Debug)]
struct LegacyFact {
    fact_key: String,
    fact_value: String,
    source: String,
}

fn parse_legacy_state(raw: &str) -> Result<LegacyState, String> {
    let mut state = LegacyState::default();
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with("event:") {
            let event = parse_legacy_event_line(trimmed)?;
            state.events.push(event);
        } else if trimmed.starts_with("fact:") {
            let fact = parse_legacy_fact_line(trimmed)?;
            state.facts.push(fact);
        } else if trimmed.starts_with("meta:indexed_source:") {
            let source = trimmed.trim_start_matches("meta:indexed_source:");
            state
                .indexed_sources
                .insert(decode_legacy_optional(source).unwrap_or_default());
        }
    }
    Ok(state)
}

fn parse_legacy_event_line(line: &str) -> Result<LegacyEvent, String> {
    let body = line.trim_start_matches("event:");
    let parts: Vec<&str> = body.splitn(7, '|').collect();
    if parts.len() != 7 {
        return Err("invalid legacy event record".to_string());
    }
    Ok(LegacyEvent {
        run_id: decode_legacy_optional(parts[2]),
        event_type: decode_legacy_optional(parts[3]).unwrap_or_default(),
        content: decode_legacy_optional(parts[4]).unwrap_or_default(),
        source_path: decode_legacy_optional(parts[5]),
    })
}

fn parse_legacy_fact_line(line: &str) -> Result<LegacyFact, String> {
    let body = line.trim_start_matches("fact:");
    let parts: Vec<&str> = body.splitn(7, '|').collect();
    if parts.len() != 7 {
        return Err("invalid legacy fact record".to_string());
    }
    Ok(LegacyFact {
        fact_key: decode_legacy_optional(parts[1]).unwrap_or_default(),
        fact_value: decode_legacy_optional(parts[2]).unwrap_or_default(),
        source: decode_legacy_optional(parts[3]).unwrap_or_default(),
    })
}

fn decode_legacy_optional(raw: &str) -> Option<String> {
    if raw.is_empty() {
        None
    } else {
        Some(raw.replace("\\n", "\n").replace("\\|", "|"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trust_decays_over_time() {
        let now = 1_000_000i64;
        let updated = now - (60 * 86_400);
        let effective = effective_trust(0.9, updated, now);
        assert!(effective < 0.9);
        assert!(effective >= 0.1);
    }

    #[test]
    fn fts_recall_finds_ingested_event() {
        let dir = std::env::temp_dir().join(format!("ozr-mem-test-{}", now_unix()));
        fs::create_dir_all(&dir).unwrap();
        let db_path = dir.join("memory.db");
        let index = MemoryIndex::new(&db_path);
        index.ensure_schema().unwrap();
        index
            .ingest_session_event(
                Some("run-test"),
                "prompt",
                "deploy sandboxd with token auth",
                None,
            )
            .unwrap();

        let hits = index.recall_fts("sandboxd token", 5).unwrap();
        assert!(!hits.is_empty());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn migrates_legacy_text_store() {
        let dir = std::env::temp_dir().join(format!("ozr-mem-legacy-{}", now_unix()));
        fs::create_dir_all(&dir).unwrap();
        let db_path = dir.join("memory.db");
        fs::write(
            &db_path,
            "meta:next_event_id:1\nmeta:next_fact_id:1\nevent:0|100|run-test|prompt|legacy sandboxd auth||sandboxd,auth\n",
        )
        .unwrap();

        let index = MemoryIndex::new(&db_path);
        index.ensure_schema().unwrap();
        let hits = index.recall_fts("sandboxd", 5).unwrap();
        assert!(!hits.is_empty());
        assert!(dir.join("memory.db.legacy").exists());

        let _ = fs::remove_dir_all(&dir);
    }
}
