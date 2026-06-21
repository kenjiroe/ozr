use crate::core::json_util::parse_audit_line;
use crate::core::trace_replay::{build_cross_trace, render_cross_trace_markdown};
use std::fs;
use std::path::Path;
use std::time::UNIX_EPOCH;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunOutcome {
    Completed,
    Denied,
    Skipped,
    Incomplete,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct RunRecord {
    pub run_id: String,
    pub started_at: u64,
    pub ended_at: u64,
    pub events: Vec<(u64, String)>,
    pub outcome: RunOutcome,
    pub failure_reason: String,
    pub tool: String,
    pub sandboxd_task_id: String,
    pub sandboxd_artifacts: Vec<String>,
}

pub fn load_runs(log_path: &str) -> Result<Vec<RunRecord>, String> {
    let path = Path::new(log_path);
    if !path.exists() {
        return Ok(Vec::new());
    }

    let content = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let mut grouped: Vec<(String, Vec<(u64, String)>)> = Vec::new();

    for line in content.lines() {
        let Some((ts, run_id, event)) = parse_audit_line(line) else {
            continue;
        };
        if run_id.is_empty() || run_id == "bootstrap" {
            continue;
        }

        if let Some(entry) = grouped.iter_mut().find(|(id, _)| id == &run_id) {
            entry.1.push((ts, event));
        } else {
            grouped.push((run_id, vec![(ts, event)]));
        }
    }

    Ok(grouped
        .into_iter()
        .map(|(run_id, events)| build_run_record(run_id, events, &[]))
        .collect())
}

pub fn load_runs_with_artifacts(log_path: &str, audit_dir: &str) -> Result<Vec<RunRecord>, String> {
    let mut runs = load_runs(log_path)?;
    let artifacts = list_sandboxd_artifacts(audit_dir)?;
    for run in &mut runs {
        run.sandboxd_artifacts = correlate_artifacts(&artifacts, run.started_at, run.ended_at);
    }
    Ok(runs)
}

pub fn latest_run(runs: &[RunRecord]) -> Option<&RunRecord> {
    runs.iter().max_by_key(|run| run.ended_at)
}

pub fn find_run<'a>(runs: &'a [RunRecord], run_id: &str) -> Option<&'a RunRecord> {
    runs.iter().find(|run| run.run_id == run_id)
}

pub fn generate_replay_report(
    log_path: &str,
    run_id: Option<&str>,
    audit_dir: &str,
    output_path: &str,
) -> Result<String, String> {
    let runs = load_runs_with_artifacts(log_path, audit_dir)?;
    if runs.is_empty() {
        return Ok("no audit runs found yet".to_string());
    }

    let selected = match run_id {
        Some(id) => find_run(&runs, id).ok_or_else(|| format!("run_id not found: {}", id))?,
        None => latest_run(&runs).ok_or_else(|| "no runs available".to_string())?,
    };

    let cross_trace = build_cross_trace(selected, ".ozr").ok();
    let mut report = render_replay_markdown(selected);
    if let Some(trace) = cross_trace {
        report.push_str(&render_cross_trace_markdown(&trace));
    }
    if let Some(parent) = Path::new(output_path).parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::write(output_path, &report).map_err(|e| e.to_string())?;
    Ok(format!("replay report written: {}", output_path))
}

pub fn render_replay_markdown(run: &RunRecord) -> String {
    let mut out = String::from("# Run Replay\n\n");
    out.push_str(&format!("- run_id: `{}`\n", run.run_id));
    out.push_str(&format!("- started_at_unix: {}\n", run.started_at));
    out.push_str(&format!("- ended_at_unix: {}\n", run.ended_at));
    out.push_str(&format!("- outcome: {:?}\n", run.outcome));
    out.push_str(&format!("- tool: `{}`\n", empty_as(&run.tool, "unknown")));
    out.push_str(&format!(
        "- failure_reason: {}\n",
        empty_as(&run.failure_reason, "none")
    ));
    if !run.sandboxd_task_id.is_empty() {
        out.push_str(&format!("- sandboxd_task_id: `{}`\n", run.sandboxd_task_id));
    }
    out.push_str("\n## Timeline\n\n");
    out.push_str("| ts | event |\n|---|---|\n");
    for (ts, event) in &run.events {
        out.push_str(&format!("| {} | {} |\n", ts, event.replace('|', "\\|")));
    }

    if !run.sandboxd_artifacts.is_empty() {
        out.push_str("\n## Sandboxd Artifacts\n\n");
        for artifact in &run.sandboxd_artifacts {
            out.push_str(&format!("- `{}`\n", artifact));
        }
    }

    out.push_str("\n## Recovery Hint\n\n");
    out.push_str(match run.outcome {
        RunOutcome::Completed => "- Run completed successfully; no recovery needed.\n",
        RunOutcome::Denied => "- Re-run with safer plan or adjust approval policy.\n",
        RunOutcome::Skipped => "- Operator skipped action; rerun with revised prompt if needed.\n",
        RunOutcome::Incomplete => "- Use `ozr session resume` if checkpoint exists.\n",
        RunOutcome::Unknown => "- Inspect timeline and audit artifacts manually.\n",
    });
    out
}

fn build_run_record(
    run_id: String,
    events: Vec<(u64, String)>,
    artifacts: &[(u64, String)],
) -> RunRecord {
    let started_at = events.first().map(|(ts, _)| *ts).unwrap_or(0);
    let ended_at = events.last().map(|(ts, _)| *ts).unwrap_or(started_at);
    let tool = events
        .iter()
        .find_map(|(_, event)| {
            if event.starts_with("plan_tool:") {
                Some(event.split(':').nth(1).unwrap_or("unknown").to_string())
            } else {
                None
            }
        })
        .unwrap_or_default();
    let sandboxd_task_id = events
        .iter()
        .find_map(|(_, event)| {
            if event.starts_with("sandboxd_task:") {
                Some(event.split(':').nth(1).unwrap_or("").to_string())
            } else {
                None
            }
        })
        .unwrap_or_default();
    let (outcome, failure_reason) = classify_run(&events);

    RunRecord {
        run_id,
        started_at,
        ended_at,
        events,
        outcome,
        failure_reason,
        tool,
        sandboxd_task_id,
        sandboxd_artifacts: correlate_artifacts(artifacts, started_at, ended_at),
    }
}

fn classify_run(events: &[(u64, String)]) -> (RunOutcome, String) {
    let mut outcome = RunOutcome::Unknown;
    let mut failure_reason = String::new();

    for (_, event) in events {
        if event == "loop_completed" {
            outcome = RunOutcome::Completed;
            failure_reason.clear();
        } else if event.starts_with("denied:") {
            outcome = RunOutcome::Denied;
            failure_reason = event.clone();
        } else if event.starts_with("skipped:") {
            outcome = RunOutcome::Skipped;
            failure_reason = event.clone();
        } else if event.starts_with("loop_failed:") {
            outcome = RunOutcome::Incomplete;
            failure_reason = event.split(':').skip(1).collect::<Vec<_>>().join(":");
        }
    }

    if outcome == RunOutcome::Unknown {
        let started = events.iter().any(|(_, event)| event == "loop_started");
        let completed = events.iter().any(|(_, event)| event == "loop_completed");
        if started && !completed {
            outcome = RunOutcome::Incomplete;
            if failure_reason.is_empty() {
                failure_reason = "run interrupted before loop_completed".to_string();
            }
        }
    }

    (outcome, failure_reason)
}

fn list_sandboxd_artifacts(audit_dir: &str) -> Result<Vec<(u64, String)>, String> {
    let dir = Path::new(audit_dir);
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut artifacts = Vec::new();
    for entry in fs::read_dir(dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if !name.starts_with("sandboxd-events-") || !name.ends_with(".json") {
            continue;
        }
        let modified = entry
            .metadata()
            .ok()
            .and_then(|meta| meta.modified().ok())
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_secs())
            .unwrap_or(0);
        artifacts.push((modified, path.display().to_string()));
    }
    Ok(artifacts)
}

fn correlate_artifacts(artifacts: &[(u64, String)], started_at: u64, ended_at: u64) -> Vec<String> {
    let window_start = started_at.saturating_sub(5);
    let window_end = ended_at.saturating_add(300);
    artifacts
        .iter()
        .filter_map(|(modified, path)| {
            if (window_start..=window_end).contains(modified) {
                Some(path.clone())
            } else {
                None
            }
        })
        .collect()
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

    #[test]
    fn classifies_denied_run() {
        let events = vec![
            (1, "loop_started".to_string()),
            (2, "plan_tool:shell".to_string()),
            (3, "denied:shell:operator said no".to_string()),
        ];
        let run = build_run_record("run-1".to_string(), events, &[]);
        assert_eq!(run.outcome, RunOutcome::Denied);
        assert!(run.failure_reason.contains("operator said no"));
    }

    #[test]
    fn classifies_incomplete_run() {
        let events = vec![
            (1, "loop_started".to_string()),
            (2, "plan_tool:read_file".to_string()),
        ];
        let run = build_run_record("run-2".to_string(), events, &[]);
        assert_eq!(run.outcome, RunOutcome::Incomplete);
    }
}
