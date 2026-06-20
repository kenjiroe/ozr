use std::fs::create_dir_all;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct MemoryStore {
    root: PathBuf,
}

impl MemoryStore {
    pub fn new<P: AsRef<Path>>(root: P) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
        }
    }

    pub fn ensure_layout(&self) -> Result<(), String> {
        let dirs = [
            self.root.join("sessions"),
            self.root.join("project"),
            self.root.join("feedback"),
            self.root.join("artifacts"),
            self.root.join("audit"),
        ];

        for dir in &dirs {
            create_dir_all(dir).map_err(|e| e.to_string())?;
        }

        Ok(())
    }

    pub fn root_path(&self) -> &Path {
        &self.root
    }

    pub fn db_path(&self) -> PathBuf {
        self.root.join("memory.db")
    }

    pub fn append_session_event(
        &self,
        run_id: &str,
        event_type: &str,
        content: &str,
    ) -> Result<(), String> {
        use std::fs::OpenOptions;
        use std::io::Write;

        self.ensure_layout()?;
        let path = self.root.join("sessions").join("events.log");
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|e| e.to_string())?;
        writeln!(file, "run_id={} type={} {}", run_id, event_type, content)
            .map_err(|e| e.to_string())
    }
}