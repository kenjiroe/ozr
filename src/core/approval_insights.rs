use crate::core::approval_report::{collect_approval_stats, ApprovalStats, ToolStats};
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone)]
pub struct InsightThresholds {
    pub denial_rate: f64,
    pub retry_rate: f64,
    pub high_risk_share: f64,
    pub tool_denial_min: usize,
}

impl Default for InsightThresholds {
    fn default() -> Self {
        Self {
            denial_rate: 0.3,
            retry_rate: 0.2,
            high_risk_share: 0.4,
            tool_denial_min: 2,
        }
    }
}

pub fn generate_approval_insights(
    log_path: &str,
    output_path: &str,
    thresholds: &InsightThresholds,
) -> Result<String, String> {
    let stats = collect_approval_stats(log_path)?;
    if stats.total_runs == 0 && stats.approved + stats.denied + stats.skipped == 0 {
        return Ok("no approval data found yet".to_string());
    }

    let alerts = detect_alerts(&stats, thresholds);
    let suggestions = build_tuning_suggestions(&stats, thresholds);
    let report = render_insights(&stats, &alerts, &suggestions)?;

    if let Some(parent) = Path::new(output_path).parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::write(output_path, &report).map_err(|e| e.to_string())?;

    Ok(format!(
        "approval insights written: {} (alerts={}, suggestions={})",
        output_path,
        alerts.len(),
        suggestions.len()
    ))
}

fn detect_alerts(stats: &ApprovalStats, thresholds: &InsightThresholds) -> Vec<String> {
    let mut alerts = Vec::new();
    let decisions = stats.approved + stats.denied + stats.skipped;
    if decisions > 0 {
        let denial_rate = stats.denied as f64 / decisions as f64;
        if denial_rate >= thresholds.denial_rate {
            alerts.push(format!(
                "ALERT denial_rate={:.0}% exceeds threshold {:.0}%",
                denial_rate * 100.0,
                thresholds.denial_rate * 100.0
            ));
        }
    }

    if stats.plan_evaluated > 0 {
        let retry_rate = stats.retry_requested as f64 / stats.plan_evaluated as f64;
        if retry_rate >= thresholds.retry_rate {
            alerts.push(format!(
                "ALERT retry_rate={:.0}% exceeds threshold {:.0}%",
                retry_rate * 100.0,
                thresholds.retry_rate * 100.0
            ));
        }

        let high_share = stats.risk_high as f64 / stats.plan_evaluated as f64;
        if high_share >= thresholds.high_risk_share {
            alerts.push(format!(
                "ALERT high_risk_share={:.0}% exceeds threshold {:.0}%",
                high_share * 100.0,
                thresholds.high_risk_share * 100.0
            ));
        }
    }

    for (tool, tool_stats) in &stats.per_tool {
        let tool_decisions = tool_stats.approved + tool_stats.denied + tool_stats.skipped;
        if tool_stats.denied >= thresholds.tool_denial_min && tool_decisions > 0 {
            let tool_denial_rate = tool_stats.denied as f64 / tool_decisions as f64;
            if tool_denial_rate >= thresholds.denial_rate {
                alerts.push(format!(
                    "ALERT tool={} denial_rate={:.0}% (denied={})",
                    tool,
                    tool_denial_rate * 100.0,
                    tool_stats.denied
                ));
            }
        }
    }

    if let Some((day, day_stats)) = stats.per_day.iter().max_by_key(|(_, s)| s.denials) {
        let avg_denials = if stats.per_day.is_empty() {
            0.0
        } else {
            stats.per_day.values().map(|d| d.denials).sum::<usize>() as f64
                / stats.per_day.len() as f64
        };
        if avg_denials > 0.0 && day_stats.denials as f64 >= avg_denials * 2.0 {
            alerts.push(format!(
                "ALERT denial_spike day={} denials={} (avg={:.1})",
                day, day_stats.denials, avg_denials
            ));
        }
    }

    alerts
}

fn build_tuning_suggestions(
    stats: &ApprovalStats,
    thresholds: &InsightThresholds,
) -> Vec<String> {
    let mut suggestions = Vec::new();
    let decisions = stats.approved + stats.denied + stats.skipped;

    if decisions > 0 {
        let denial_rate = stats.denied as f64 / decisions as f64;
        if denial_rate >= thresholds.denial_rate {
            suggestions.push(
                "Set OZR_APPROVAL_MODE=prompt for high-risk tools until denial rate drops."
                    .to_string(),
            );
            suggestions.push(
                "Enable OZR_FEATURE_SANDBOXD_EXECUTOR=true and route high-risk tools through sandboxd."
                    .to_string(),
            );
        }
    }

    if stats.plan_evaluated > 0 {
        let high_share = stats.risk_high as f64 / stats.plan_evaluated as f64;
        if high_share >= thresholds.high_risk_share {
            suggestions.push(
                "Tighten ponytail profile: try OZR_FEATURE_PONYTAIL_PROFILE=full or ultra."
                    .to_string(),
            );
        }

        let retry_rate = stats.retry_requested as f64 / stats.plan_evaluated as f64;
        if retry_rate >= thresholds.retry_rate {
            suggestions.push(
                "Reduce retry churn: constrain tool params in policy or require edit-plan before retry."
                    .to_string(),
            );
        }
    }

    let mut hot_tools: Vec<(&String, &ToolStats)> = stats
        .per_tool
        .iter()
        .filter(|(_, s)| s.denied >= thresholds.tool_denial_min)
        .collect();
    hot_tools.sort_by_key(|(_, s)| std::cmp::Reverse(s.denied));
    for (tool, tool_stats) in hot_tools.into_iter().take(3) {
        suggestions.push(format!(
            "Review policy for tool `{}`: denied={} retry={} edited={}.",
            tool, tool_stats.denied, tool_stats.retry_requested, tool_stats.edited
        ));
    }

    if suggestions.is_empty() {
        suggestions.push("No policy tuning needed yet; current approval metrics look stable.".to_string());
    }

    suggestions
}

fn render_insights(
    stats: &ApprovalStats,
    alerts: &[String],
    suggestions: &[String],
) -> Result<String, String> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_secs();

    let mut out = format!(
        "# Approval Insights\n\n- generated_at_unix: {}\n- total_runs: {}\n- plans_evaluated: {}\n\n",
        now, stats.total_runs, stats.plan_evaluated
    );

    out.push_str("## Alerts\n");
    if alerts.is_empty() {
        out.push_str("- none\n\n");
    } else {
        for alert in alerts {
            out.push_str(&format!("- {}\n", alert));
        }
        out.push('\n');
    }

    out.push_str("## Policy Tuning Suggestions\n");
    for suggestion in suggestions {
        out.push_str(&format!("- {}\n", suggestion));
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn detects_high_denial_rate() {
        let dir = std::env::temp_dir().join(format!("ozr-insights-{}", now_unix()));
        fs::create_dir_all(&dir).unwrap();
        let log_path = dir.join("runs.log");
        let mut file = fs::File::create(&log_path).unwrap();
        writeln!(
            file,
            r#"{{"ts":100,"run_id":"run-1","event":"plan_evaluated:high"}}"#
        )
        .unwrap();
        for _ in 0..3 {
            writeln!(
                file,
                r#"{{"ts":100,"run_id":"run-1","event":"denied:shell_exec:unsafe"}}"#
            )
            .unwrap();
        }
        writeln!(
            file,
            r#"{{"ts":100,"run_id":"run-1","event":"approved:read_file:ok"}}"#
        )
        .unwrap();

        let stats = collect_approval_stats(log_path.to_str().unwrap()).unwrap();
        let alerts = detect_alerts(
            &stats,
            &InsightThresholds {
                denial_rate: 0.5,
                ..InsightThresholds::default()
            },
        );
        assert!(!alerts.is_empty());

        let _ = fs::remove_dir_all(&dir);
    }

    fn now_unix() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }
}
