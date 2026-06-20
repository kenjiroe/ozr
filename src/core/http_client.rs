use tokio::process::Command;
use tokio::time::{sleep, Duration};

pub async fn shell_output(cmd: &str) -> Result<std::process::Output, String> {
    Command::new("sh")
        .arg("-lc")
        .arg(cmd)
        .output()
        .await
        .map_err(|e| e.to_string())
}

pub async fn sleep_ms(ms: u64) {
    if ms > 0 {
        sleep(Duration::from_millis(ms)).await;
    }
}

pub fn shell_quote(value: &str) -> String {
    if value.is_empty() {
        return "''".to_string();
    }
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}
