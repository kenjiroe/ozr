use crate::core::json_util::parse_audit_line;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Default, Clone)]
pub struct ApprovalStats {
    pub total_runs: usize,
    pub plan_evaluated: usize,
    pub risk_low: usize,
    pub risk_medium: usize,
    pub risk_high: usize,
    pub approved: usize,
    pub denied: usize,
    pub skipped: usize,
    pub retry_requested: usize,
    pub plan_edited: usize,
    pub per_tool: HashMap<String, ToolStats>,
    pub per_day: BTreeMap<String, DayStats>,
}

#[derive(Debug, Default, Clone)]
pub struct ToolStats {
    pub approved: usize,
    pub denied: usize,
    pub skipped: usize,
    pub retry_requested: usize,
    pub edited: usize,
    pub executed: usize,
}

#[derive(Debug, Default, Clone)]
pub struct DayStats {
    pub approvals: usize,
    pub denials: usize,
    pub skips: usize,
}

pub fn collect_approval_stats(log_path: &str) -> Result<ApprovalStats, String> {
    let log_file = Path::new(log_path);
    if !log_file.exists() {
        return Ok(ApprovalStats::default());
    }

    let content = fs::read_to_string(log_file).map_err(|e| e.to_string())?;
    let mut stats = ApprovalStats::default();
    let mut runs = HashSet::new();

    for line in content.lines() {
        let Some((ts, run_id, event)) = parse_audit_line(line) else {
            continue;
        };

        if !run_id.is_empty() && run_id != "bootstrap" {
            runs.insert(run_id);
        }

        let day_key = day_bucket(ts);

        if event.starts_with("plan_evaluated:") {
            stats.plan_evaluated += 1;
            let tier = event.split(':').nth(1).unwrap_or("");
            match tier {
                "low" => stats.risk_low += 1,
                "medium" => stats.risk_medium += 1,
                "high" => stats.risk_high += 1,
                _ => {}
            }
        } else if event.starts_with("approved:") {
            stats.approved += 1;
            let tool = outcome_tool(&event);
            stats.per_tool.entry(tool).or_default().approved += 1;
            stats.per_day.entry(day_key).or_default().approvals += 1;
        } else if event.starts_with("denied:") {
            stats.denied += 1;
            let tool = outcome_tool(&event);
            stats.per_tool.entry(tool).or_default().denied += 1;
            stats.per_day.entry(day_key).or_default().denials += 1;
        } else if event.starts_with("skipped:") {
            stats.skipped += 1;
            let tool = outcome_tool(&event);
            stats.per_tool.entry(tool).or_default().skipped += 1;
            stats.per_day.entry(day_key).or_default().skips += 1;
        } else if event.starts_with("retry_requested:") {
            stats.retry_requested += 1;
            let tool = outcome_tool(&event);
            stats.per_tool.entry(tool).or_default().retry_requested += 1;
        } else if event.starts_with("plan_edited:") {
            stats.plan_edited += 1;
            let tool = outcome_tool(&event);
            stats.per_tool.entry(tool).or_default().edited += 1;
        } else if event.starts_with("tool_executed:") {
            let tool = event.split(':').nth(1).unwrap_or("unknown").to_string();
            stats.per_tool.entry(tool).or_default().executed += 1;
        }
    }

    stats.total_runs = runs.len();
    Ok(stats)
}

pub fn generate_approval_dashboard(log_path: &str, output_path: &str) -> Result<String, String> {
    let stats = collect_approval_stats(log_path)?;
    if stats.total_runs == 0 && stats.approved + stats.denied + stats.skipped == 0 {
        return Ok("no audit log found yet".to_string());
    }

    let report = render_report(&stats)?;

    if let Some(parent) = Path::new(output_path).parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::write(output_path, &report).map_err(|e| e.to_string())?;

    Ok(format!("approval dashboard written: {}", output_path))
}

fn render_report(stats: &ApprovalStats) -> Result<String, String> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_secs();

    Ok(format!(
        "# Approval Dashboard\n\n- generated_at_unix: {}\n- total_runs: {}\n- plans_evaluated: {}\n\n## Risk Distribution\n- low: {}\n- medium: {}\n- high: {}\n\n## Approval Outcomes\n- approved: {}\n- denied: {}\n- skipped: {}\n- retry_requested: {}\n- plan_edited: {}\n\n{}\n\n{}\n",
        now,
        stats.total_runs,
        stats.plan_evaluated,
        stats.risk_low,
        stats.risk_medium,
        stats.risk_high,
        stats.approved,
        stats.denied,
        stats.skipped,
        stats.retry_requested,
        stats.plan_edited,
        render_per_tool(stats),
        render_trend(stats),
    ))
}

fn render_per_tool(stats: &ApprovalStats) -> String {
    let mut rows: Vec<(String, ToolStats)> = stats
        .per_tool
        .iter()
        .map(|(tool, s)| (tool.clone(), s.clone()))
        .collect();
    rows.sort_by(|a, b| a.0.cmp(&b.0));

    let mut out = String::from(
        "## Per-Tool Breakdown\n| Tool | Approved | Denied | Skipped | Retry | Edited | Executed |\n|---|---:|---:|---:|---:|---:|---:|\n",
    );
    for (tool, s) in rows {
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} |\n",
            tool, s.approved, s.denied, s.skipped, s.retry_requested, s.edited, s.executed
        ));
    }
    out
}

fn render_trend(stats: &ApprovalStats) -> String {
    let mut out = String::from(
        "## Daily Trend\n| DayBucket | Approvals | Denials | Skips |\n|---|---:|---:|---:|\n",
    );
    for (day, s) in &stats.per_day {
        out.push_str(&format!(
            "| {} | {} | {} | {} |\n",
            day, s.approvals, s.denials, s.skips
        ));
    }
    out
}

fn outcome_tool(event: &str) -> String {
    event.split(':').nth(1).unwrap_or("unknown").to_string()
}

fn day_bucket(ts: u64) -> String {
    if ts == 0 {
        return "unknown".to_string();
    }
    format!("day-{}", ts / 86_400)
}
