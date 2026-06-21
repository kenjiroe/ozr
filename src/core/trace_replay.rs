use crate::core::json_util::{parse_json, SandboxdEventCapture};
use crate::core::replay::RunRecord;
use crate::core::session_recovery::{load_checkpoint, SessionCheckpoint};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct SandboxdTraceSummary {
    pub path: String,
    pub total_events: usize,
    pub by_event: HashMap<String, usize>,
}

#[derive(Debug, Clone)]
pub struct CrossTrace {
    pub session_checkpoint: Option<SessionCheckpoint>,
    pub memory_events: Vec<String>,
    pub sandboxd_summaries: Vec<SandboxdTraceSummary>,
}

pub fn build_cross_trace(run: &RunRecord, workspace: &str) -> Result<CrossTrace, String> {
    let checkpoint = load_checkpoint()?.filter(|state| state.run_id == run.run_id);
    let memory_events = load_memory_events(workspace, run, 12);
    let sandboxd_summaries = load_sandboxd_summaries(&run.sandboxd_artifacts)?;

    Ok(CrossTrace {
        session_checkpoint: checkpoint,
        memory_events,
        sandboxd_summaries,
    })
}

pub fn render_cross_trace_markdown(trace: &CrossTrace) -> String {
    let mut out = String::from("## Cross Trace\n\n");

    if let Some(checkpoint) = &trace.session_checkpoint {
        out.push_str("### Session Checkpoint\n\n");
        out.push_str(&format!(
            "- status: `{:?}`\n- last_event: `{}`\n- failure_reason: {}\n\n",
            checkpoint.status,
            checkpoint.last_event,
            empty_as(&checkpoint.failure_reason, "none")
        ));
    } else {
        out.push_str("### Session Checkpoint\n\n- none matched for run_id\n\n");
    }

    out.push_str("### Memory Events\n\n");
    if trace.memory_events.is_empty() {
        out.push_str("- no related memory events found\n\n");
    } else {
        for event in &trace.memory_events {
            out.push_str(&format!("- {}\n", event.replace('|', "\\|")));
        }
        out.push('\n');
    }

    out.push_str("### Sandboxd Event Summaries\n\n");
    if trace.sandboxd_summaries.is_empty() {
        out.push_str("- no sandboxd capture artifacts linked\n\n");
    } else {
        for summary in &trace.sandboxd_summaries {
            out.push_str(&format!(
                "- `{}`: total={}\n",
                summary.path, summary.total_events
            ));
            for (event, count) in &summary.by_event {
                out.push_str(&format!("  - {}: {}\n", event, count));
            }
        }
        out.push('\n');
    }

    out
}

fn load_memory_events(workspace: &str, run: &RunRecord, limit: usize) -> Vec<String> {
    let path = Path::new(workspace).join("sessions").join("events.log");
    if !path.exists() {
        return Vec::new();
    }
    let content = fs::read_to_string(path).unwrap_or_default();
    let tool_needle = if run.tool.trim().is_empty() {
        None
    } else {
        Some(run.tool.to_lowercase())
    };

    content
        .lines()
        .filter(|line| {
            if line.trim().is_empty() {
                return false;
            }
            if line.contains(&format!("run_id={}", run.run_id)) {
                return true;
            }
            match &tool_needle {
                Some(tool_name) => line.to_lowercase().contains(tool_name),
                None => false,
            }
        })
        .rev()
        .take(limit)
        .map(|line| line.to_string())
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect()
}

fn load_sandboxd_summaries(paths: &[String]) -> Result<Vec<SandboxdTraceSummary>, String> {
    let mut summaries = Vec::new();
    for path in paths {
        if !Path::new(path).exists() {
            continue;
        }
        let content = fs::read_to_string(path).map_err(|e| e.to_string())?;
        let capture = parse_sandboxd_capture(&content);
        summaries.push(SandboxdTraceSummary {
            path: path.clone(),
            total_events: capture.summary.total,
            by_event: capture.summary.by_event,
        });
    }
    Ok(summaries)
}

fn parse_sandboxd_capture(content: &str) -> SandboxdEventCapture {
    if let Ok(value) = parse_json(content) {
        if let Ok(capture) = serde_json::from_value::<SandboxdEventCapture>(value) {
            return capture;
        }
    }
    crate::core::json_util::parse_sandboxd_sse_capture(content)
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
    use crate::core::replay::{RunOutcome, RunRecord};

    #[test]
    fn renders_cross_trace_sections() {
        let trace = CrossTrace {
            session_checkpoint: None,
            memory_events: vec!["prompt=hello".to_string()],
            sandboxd_summaries: vec![SandboxdTraceSummary {
                path: ".ozr/audit/sandboxd-events-task.json".to_string(),
                total_events: 2,
                by_event: HashMap::from([("message".to_string(), 2)]),
            }],
        };
        let rendered = render_cross_trace_markdown(&trace);
        assert!(rendered.contains("Memory Events"));
        assert!(rendered.contains("prompt=hello"));
        assert!(rendered.contains("total=2"));
    }

    #[test]
    fn builds_cross_trace_for_run_without_files() {
        let run = RunRecord {
            run_id: "run-1".to_string(),
            started_at: 1,
            ended_at: 2,
            events: vec![],
            outcome: RunOutcome::Completed,
            failure_reason: String::new(),
            tool: "read_file".to_string(),
            sandboxd_task_id: String::new(),
            sandboxd_artifacts: vec![],
        };
        let trace = build_cross_trace(&run, "/tmp/missing-ozr").expect("trace");
        assert!(trace.session_checkpoint.is_none());
    }
}
