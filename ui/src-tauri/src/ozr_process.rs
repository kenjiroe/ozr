use std::io::Read;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

/// Avoid 9000 — commonly used by MinIO and other local services.
const DEFAULT_PORT: u16 = 18787;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionMode {
    Spawned,
    External,
}

enum ApiConnection {
    Spawned { api_base: String },
    External { api_base: String },
}

pub struct OzrProcess {
    connection: ApiConnection,
    child: Mutex<Option<Child>>,
}

impl OzrProcess {
    pub fn new() -> Self {
        if let Some(api_base) = external_api_base_from_env() {
            Self {
                connection: ApiConnection::External { api_base },
                child: Mutex::new(None),
            }
        } else {
            let port = std::env::var("OZR_GUI_API_PORT")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(DEFAULT_PORT);
            Self {
                connection: ApiConnection::Spawned {
                    api_base: format!("http://127.0.0.1:{}", port),
                },
                child: Mutex::new(None),
            }
        }
    }

    pub fn connection_mode(&self) -> ConnectionMode {
        match self.connection {
            ApiConnection::Spawned { .. } => ConnectionMode::Spawned,
            ApiConnection::External { .. } => ConnectionMode::External,
        }
    }

    pub fn start(&self) -> Result<(), String> {
        if self.connection_mode() == ConnectionMode::External {
            return Ok(());
        }

        let mut guard = self
            .child
            .lock()
            .map_err(|_| "ozr process lock poisoned".to_string())?;
        if guard.is_some() {
            return Ok(());
        }

        let ApiConnection::Spawned { ref api_base } = self.connection else {
            return Ok(());
        };

        let binary = resolve_ozr_binary()?;
        let repo_root = repo_root_from_manifest()?;
        let bind = api_base
            .strip_prefix("http://")
            .or_else(|| api_base.strip_prefix("https://"))
            .unwrap_or(api_base)
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
            if self.connection_mode() == ConnectionMode::Spawned {
                if let Some(message) = self.check_child_failed()? {
                    return Err(message);
                }
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
                            "unexpected /health response on {} (another service may own this port). Set OZR_GUI_API_PORT or OZR_GUI_API_BASE",
                            self.api_base()
                        ));
                    }
                }
                Err(_) => {}
            }

            thread::sleep(Duration::from_millis(interval_ms));
        }

        let hint = match self.connection_mode() {
            ConnectionMode::External => {
                "Start the Docker stack: ./scripts/docker-up-stack.sh".to_string()
            }
            ConnectionMode::Spawned => format!(
                "Try another port: export OZR_GUI_API_PORT=18788"
            ),
        };

        Err(format!(
            "ozr API not ready at {} after {}s. {}",
            self.api_base(),
            (attempts as u64 * interval_ms) / 1000,
            hint
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
        if self.connection_mode() == ConnectionMode::External {
            return;
        }
        if let Ok(mut guard) = self.child.lock() {
            if let Some(mut child) = guard.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }

    pub fn api_base(&self) -> &str {
        match &self.connection {
            ApiConnection::Spawned { api_base } | ApiConnection::External { api_base } => api_base,
        }
    }
}

fn external_api_base_from_env() -> Option<String> {
    let raw = std::env::var("OZR_GUI_API_BASE")
        .ok()?
        .trim()
        .trim_end_matches('/')
        .to_string();
    if raw.is_empty() {
        return None;
    }
    normalize_api_base(&raw).ok()
}

fn normalize_api_base(raw: &str) -> Result<String, String> {
    if !raw.starts_with("http://") && !raw.starts_with("https://") {
        return Err(format!(
            "OZR_GUI_API_BASE must start with http:// or https:// (got {raw})"
        ));
    }
    Ok(raw.to_string())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_api_base_requires_scheme() {
        assert!(normalize_api_base("127.0.0.1:8080").is_err());
        assert_eq!(
            normalize_api_base("http://127.0.0.1:8080").unwrap(),
            "http://127.0.0.1:8080"
        );
    }
}
