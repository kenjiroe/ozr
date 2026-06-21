use std::io::Read;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

/// Avoid 9000 — commonly used by MinIO and other local services.
const DEFAULT_PORT: u16 = 18787;

pub struct OzrProcess {
    child: Mutex<Option<Child>>,
    api_base: String,
}

impl OzrProcess {
    pub fn new() -> Self {
        let port = std::env::var("OZR_GUI_API_PORT")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(DEFAULT_PORT);
        Self {
            child: Mutex::new(None),
            api_base: format!("http://127.0.0.1:{}", port),
        }
    }

    pub fn start(&self) -> Result<(), String> {
        let mut guard = self
            .child
            .lock()
            .map_err(|_| "ozr process lock poisoned".to_string())?;
        if guard.is_some() {
            return Ok(());
        }

        let binary = resolve_ozr_binary()?;
        let repo_root = repo_root_from_manifest()?;
        let bind = self
            .api_base
            .strip_prefix("http://")
            .unwrap_or(&self.api_base)
            .to_string();

        let mut child = Command::new(&binary)
            .arg("serve")
            .current_dir(&repo_root)
            .env("OZR_API_BIND", bind)
            .env("OZR_LLM_BACKEND", "mock")
            .env("OZR_MCP_BACKEND", "mock")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|err| {
                format!(
                    "failed to spawn ozr at {}: {err}. Run `cargo build` in the repo root or set OZR_BINARY",
                    binary.display()
                )
            })?;

        thread::sleep(Duration::from_millis(250));
        if let Some(message) = child_failure_message(&mut child) {
            return Err(message);
        }

        *guard = Some(child);
        Ok(())
    }

    pub fn wait_until_healthy(&self, attempts: usize, interval_ms: u64) -> Result<String, String> {
        let url = format!("{}/health", self.api_base());
        for _ in 0..attempts {
            if let Some(message) = self.check_child_failed()? {
                return Err(message);
            }

            match ureq::get(&url).call() {
                Ok(response) if response.status() == 200 => {
                    let body = response.into_string().unwrap_or_default();
                    if body.trim() == "ok" {
                        return Ok(self.api_base().to_string());
                    }
                }
                Ok(response) => {
                    let body = response.into_string().unwrap_or_default();
                    if !body.trim().is_empty() && body.trim() != "ok" {
                        return Err(format!(
                            "unexpected /health response on {} (another service may own this port). Set OZR_GUI_API_PORT",
                            self.api_base()
                        ));
                    }
                }
                Err(_) => {}
            }

            thread::sleep(Duration::from_millis(interval_ms));
        }

        Err(format!(
            "ozr API not ready at {} after {}s. Try another port: export OZR_GUI_API_PORT=18788",
            self.api_base(),
            (attempts as u64 * interval_ms) / 1000
        ))
    }

    fn check_child_failed(&self) -> Result<Option<String>, String> {
        let mut guard = self
            .child
            .lock()
            .map_err(|_| "ozr process lock poisoned".to_string())?;
        let Some(child) = guard.as_mut() else {
            return Ok(Some("ozr process is not running".to_string()));
        };
        Ok(child_failure_message(child))
    }

    pub fn stop(&self) {
        if let Ok(mut guard) = self.child.lock() {
            if let Some(mut child) = guard.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }

    pub fn api_base(&self) -> &str {
        &self.api_base
    }
}

fn child_failure_message(child: &mut Child) -> Option<String> {
    match child.try_wait() {
        Ok(Some(status)) => {
            let mut stderr = String::new();
            if let Some(err) = child.stderr.as_mut() {
                let _ = err.read_to_string(&mut stderr);
            }
            let mut stdout = String::new();
            if let Some(out) = child.stdout.as_mut() {
                let _ = out.read_to_string(&mut stdout);
            }
            let detail = if !stderr.trim().is_empty() {
                stderr.trim().to_string()
            } else if !stdout.trim().is_empty() {
                stdout.trim().to_string()
            } else {
                "no output captured".to_string()
            };
            Some(format!("ozr exited with {status}: {detail}"))
        }
        Ok(None) => None,
        Err(err) => Some(format!("failed to poll ozr process: {err}")),
    }
}

fn resolve_ozr_binary() -> Result<PathBuf, String> {
    if let Ok(path) = std::env::var("OZR_BINARY") {
        let candidate = PathBuf::from(&path);
        if candidate.is_file() {
            return Ok(candidate);
        }
        return Err(format!("OZR_BINARY points to missing file: {path}"));
    }

    let repo_root = repo_root_from_manifest()?;
    for name in ["target/debug/ozr", "target/release/ozr"] {
        let candidate = repo_root.join(name);
        if candidate.is_file() {
            return candidate.canonicalize().map_err(|err| err.to_string());
        }
    }

    Ok(PathBuf::from("ozr"))
}

fn repo_root_from_manifest() -> Result<PathBuf, String> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    repo_root_from_manifest_dir(&manifest_dir)
}

fn repo_root_from_manifest_dir(manifest_dir: &PathBuf) -> Result<PathBuf, String> {
    manifest_dir
        .join("../..")
        .canonicalize()
        .map_err(|err| format!("failed to resolve ozr repo root: {err}"))
}
