use std::fs::{create_dir_all, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub struct AuditLogger {
    log_path: PathBuf,
}

impl AuditLogger {
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self, String> {
        let path_buf = path.as_ref().to_path_buf();
        if let Some(parent) = path_buf.parent() {
            create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        Ok(Self { log_path: path_buf })
    }

    pub fn append(&mut self, run_id: &str, message: &str) -> Result<(), String> {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_secs();

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_path)
            .map_err(|e| e.to_string())?;

        writeln!(
            file,
            "{{\"ts\":{},\"run_id\":\"{}\",\"event\":\"{}\"}}",
            ts,
            run_id,
            sanitize(message)
        )
        .map_err(|e| e.to_string())
    }
}

fn sanitize(input: &str) -> String {
    input.replace('"', "'")
}
