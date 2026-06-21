use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Running,
    Completed,
    Failed,
    Interrupted,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionCheckpoint {
    pub run_id: String,
    pub prompt: String,
    pub status: SessionStatus,
    pub started_at: u64,
    pub updated_at: u64,
    pub last_event: String,
    pub failure_reason: String,
}

pub fn checkpoint_path() -> String {
    if let Ok(path) = std::env::var("OZR_SESSION_CHECKPOINT_PATH") {
        if !path.trim().is_empty() {
            return path;
        }
    }
    ".ozr/sessions/checkpoint.json".to_string()
}

pub fn detect_interrupted_checkpoint() -> Result<Option<SessionCheckpoint>, String> {
    let Some(mut checkpoint) = load_checkpoint()? else {
        return Ok(None);
    };
    if checkpoint.status == SessionStatus::Running {
        checkpoint.status = SessionStatus::Interrupted;
        checkpoint.failure_reason = "run interrupted before loop_completed".to_string();
        checkpoint.updated_at = unix_now();
        save_checkpoint(&checkpoint)?;
        return Ok(Some(checkpoint));
    }
    Ok(None)
}

pub fn begin_session(run_id: &str, prompt: &str) -> Result<(), String> {
    let checkpoint = SessionCheckpoint {
        run_id: run_id.to_string(),
        prompt: prompt.to_string(),
        status: SessionStatus::Running,
        started_at: unix_now(),
        updated_at: unix_now(),
        last_event: "loop_started".to_string(),
        failure_reason: String::new(),
    };
    ensure_parent(&checkpoint_path())?;
    save_checkpoint(&checkpoint)
}

pub fn touch_session(run_id: &str, last_event: &str) -> Result<(), String> {
    let mut checkpoint =
        load_checkpoint()?.ok_or_else(|| "session checkpoint missing".to_string())?;
    if checkpoint.run_id != run_id {
        return Err("session checkpoint run_id mismatch".to_string());
    }
    checkpoint.updated_at = unix_now();
    checkpoint.last_event = last_event.to_string();
    save_checkpoint(&checkpoint)
}

pub fn complete_session(run_id: &str) -> Result<(), String> {
    update_status(run_id, SessionStatus::Completed, "")
}

pub fn fail_session(run_id: &str, reason: &str) -> Result<(), String> {
    update_status(run_id, SessionStatus::Failed, reason)
}

pub fn load_checkpoint() -> Result<Option<SessionCheckpoint>, String> {
    let path_value = checkpoint_path();
    let path = Path::new(&path_value);
    if !path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(path).map_err(|e| e.to_string())?;
    serde_json::from_str(&content).map_err(|e| format!("invalid session checkpoint: {}", e))
}

pub fn recoverable_prompt() -> Result<Option<String>, String> {
    let Some(checkpoint) = load_checkpoint()? else {
        return Ok(None);
    };
    match checkpoint.status {
        SessionStatus::Interrupted | SessionStatus::Failed => {
            if checkpoint.prompt.trim().is_empty() {
                Ok(None)
            } else {
                Ok(Some(checkpoint.prompt))
            }
        }
        _ => Ok(None),
    }
}

pub fn render_status(checkpoint: &SessionCheckpoint) -> String {
    format!(
        "run_id={}\nstatus={:?}\nstarted_at={}\nupdated_at={}\nlast_event={}\nfailure_reason={}\nprompt_preview={}",
        checkpoint.run_id,
        checkpoint.status,
        checkpoint.started_at,
        checkpoint.updated_at,
        checkpoint.last_event,
        empty_as(&checkpoint.failure_reason, "n/a"),
        preview(&checkpoint.prompt, 120),
    )
}

fn update_status(run_id: &str, status: SessionStatus, reason: &str) -> Result<(), String> {
    let mut checkpoint =
        load_checkpoint()?.ok_or_else(|| "session checkpoint missing".to_string())?;
    if checkpoint.run_id != run_id {
        return Err("session checkpoint run_id mismatch".to_string());
    }
    checkpoint.status = status;
    checkpoint.updated_at = unix_now();
    if !reason.is_empty() {
        checkpoint.failure_reason = reason.to_string();
    }
    save_checkpoint(&checkpoint)
}

fn save_checkpoint(checkpoint: &SessionCheckpoint) -> Result<(), String> {
    ensure_parent(&checkpoint_path())?;
    let encoded = serde_json::to_string_pretty(checkpoint).map_err(|e| e.to_string())?;
    fs::write(&checkpoint_path(), encoded).map_err(|e| e.to_string())
}

fn ensure_parent(path: &str) -> Result<(), String> {
    if let Some(parent) = Path::new(path).parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn preview(text: &str, max_chars: usize) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= max_chars {
        trimmed.to_string()
    } else {
        format!("{}...", trimmed.chars().take(max_chars).collect::<String>())
    }
}

fn empty_as(value: &str, fallback: &str) -> String {
    if value.trim().is_empty() {
        fallback.to_string()
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_checkpoint_path() -> String {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        format!("/tmp/ozr-session-test-{}.json", ts)
    }

    fn with_temp_path<F: FnOnce()>(test: F) {
        let _guard = crate::test_support::env_test_lock();
        let path = temp_checkpoint_path();
        std::env::set_var("OZR_SESSION_CHECKPOINT_PATH", &path);
        test();
        let _ = fs::remove_file(path);
        std::env::remove_var("OZR_SESSION_CHECKPOINT_PATH");
    }

    #[test]
    fn interrupted_running_checkpoint_on_restart() {
        with_temp_path(|| {
            begin_session("run-1", "resume me").expect("begin");
            let interrupted = detect_interrupted_checkpoint()
                .expect("detect")
                .expect("interrupted");
            assert_eq!(interrupted.status, SessionStatus::Interrupted);
            assert_eq!(
                recoverable_prompt().expect("recover").as_deref(),
                Some("resume me")
            );
        });
    }
}
