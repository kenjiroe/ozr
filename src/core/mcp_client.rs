use crate::core::config::AppConfig;
use crate::core::json_util::{
    build_mcp_tools_call_payload, mcp_tool_definitions, mcp_tool_names, mcp_tool_text,
    parse_json_rpc_response,
};
use crate::core::mcp_protocol::{
    build_initialize_request, build_initialized_notification, build_json_rpc_request,
    read_message_async, response_for_id, write_message_async, McpFraming,
};
use crate::core::mcp_tool_catalog::{
    action_kind_for_tool, build_tool_definition, McpToolDefinition,
};
use crate::core::policy::ActionKind;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::Mutex;
use tokio::time::sleep;

pub fn build_mcp_client(cfg: &AppConfig) -> Box<dyn McpClient> {
    if cfg.mcp_backend == "stdio" {
        Box::new(StdioMcpClient::new(
            cfg.mcp_stdio_command.clone(),
            split_stdio_args(&cfg.mcp_stdio_args),
            cfg.mcp_stdio_timeout_ms,
            cfg.mcp_stdio_retry_attempts,
            McpFraming::from_env(&cfg.mcp_stdio_framing),
        ))
    } else {
        Box::new(MockMcpClient)
    }
}

pub fn default_filesystem_stdio_config(root: &Path) -> (String, Vec<String>, McpFraming) {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let server = manifest_dir.join(
        "tests/fixtures/mcp-filesystem/node_modules/@modelcontextprotocol/server-filesystem/dist/index.js",
    );
    (
        "node".to_string(),
        vec![
            server.to_string_lossy().into_owned(),
            root.to_string_lossy().into_owned(),
        ],
        McpFraming::Ndjson,
    )
}

fn split_stdio_args(raw: &str) -> Vec<String> {
    raw.split_whitespace()
        .map(|value| value.to_string())
        .collect()
}

#[async_trait]
pub trait McpClient: Send + Sync {
    async fn list_tool_definitions(&self) -> Vec<McpToolDefinition>;

    async fn call_tool(&self, tool: &str, params: &str) -> Result<String, String>;

    async fn list_tools(&self) -> Vec<String> {
        self.list_tool_definitions()
            .await
            .into_iter()
            .map(|tool| tool.name)
            .collect()
    }

    async fn action_kind_for(&self, tool: &str) -> ActionKind {
        let catalog = self.list_tool_definitions().await;
        action_kind_for_tool(&catalog, tool)
    }
}

#[async_trait]
impl McpClient for Box<dyn McpClient> {
    async fn list_tool_definitions(&self) -> Vec<McpToolDefinition> {
        self.as_ref().list_tool_definitions().await
    }

    async fn call_tool(&self, tool: &str, params: &str) -> Result<String, String> {
        self.as_ref().call_tool(tool, params).await
    }
}

#[derive(Debug, Default, Clone)]
pub struct MockMcpClient;

#[async_trait]
impl McpClient for MockMcpClient {
    async fn list_tool_definitions(&self) -> Vec<McpToolDefinition> {
        vec![
            build_tool_definition("read_file", "Read a file"),
            build_tool_definition("list_tools", "List available tools"),
        ]
    }

    async fn call_tool(&self, tool: &str, params: &str) -> Result<String, String> {
        match tool {
            "read_file" => Ok(format!("mock read executed with {}", params)),
            "run_shell" => Ok(format!("mock shell executed with {}", params)),
            "list_tools" => Ok("read_file,list_tools".to_string()),
            _ => Err(format!("unknown tool: {}", tool)),
        }
    }
}

#[derive(Debug, Clone)]
pub struct StdioMcpClient {
    pub command: String,
    pub args: Vec<String>,
    pub timeout_ms: u64,
    pub retry_attempts: usize,
    pub framing: McpFraming,
    cache: std::sync::Arc<Mutex<Option<Vec<McpToolDefinition>>>>,
}

impl StdioMcpClient {
    pub fn new(
        command: String,
        args: Vec<String>,
        timeout_ms: u64,
        retry_attempts: usize,
        framing: McpFraming,
    ) -> Self {
        Self {
            command,
            args,
            timeout_ms,
            retry_attempts: retry_attempts.max(1),
            framing,
            cache: std::sync::Arc::new(Mutex::new(None)),
        }
    }

    pub fn is_ready(&self) -> bool {
        !self.command.trim().is_empty()
    }

    async fn with_retries<T, F, Fut>(&self, op: F) -> Result<T, String>
    where
        F: Fn() -> Fut,
        Fut: std::future::Future<Output = Result<T, String>>,
    {
        let mut last_err = String::from("mcp operation failed");
        for attempt in 0..self.retry_attempts {
            match op().await {
                Ok(value) => return Ok(value),
                Err(err) => {
                    last_err = err;
                    if attempt + 1 < self.retry_attempts {
                        sleep(Duration::from_millis(100 * (attempt as u64 + 1))).await;
                    }
                }
            }
        }
        Err(last_err)
    }

    async fn refresh_tools(&self) -> Result<Vec<McpToolDefinition>, String> {
        if !self.is_ready() {
            return Err("mcp stdio client not configured".to_string());
        }
        let command = self.command.clone();
        let args = self.args.clone();
        let timeout_ms = self.timeout_ms;
        let framing = self.framing;
        self.with_retries(|| async {
            let mut session =
                AsyncStdioMcpSession::spawn(&command, &args, timeout_ms, framing).await?;
            session.initialize().await?;
            let result = session.request("tools/list", json!({})).await?;
            let tools = mcp_tool_definitions(&result);
            if tools.is_empty() {
                Ok(mcp_tool_names(&result)
                    .into_iter()
                    .map(|name| build_tool_definition(&name, ""))
                    .collect())
            } else {
                Ok(tools)
            }
        })
        .await
    }

    async fn cached_tools(&self) -> Result<Vec<McpToolDefinition>, String> {
        if let Some(tools) = self.cache.lock().await.clone() {
            return Ok(tools);
        }
        let tools = self.refresh_tools().await?;
        *self.cache.lock().await = Some(tools.clone());
        Ok(tools)
    }
}

#[async_trait]
impl McpClient for StdioMcpClient {
    async fn list_tool_definitions(&self) -> Vec<McpToolDefinition> {
        self.cached_tools().await.unwrap_or_default()
    }

    async fn call_tool(&self, tool: &str, params: &str) -> Result<String, String> {
        if !self.is_ready() {
            return Err("mcp stdio client not configured".to_string());
        }
        let payload = build_mcp_tools_call_payload(tool, params);
        let params_value: Value = serde_json::from_str(&payload)
            .unwrap_or_else(|_| json!({"name": tool, "arguments": {}}));
        let command = self.command.clone();
        let args = self.args.clone();
        let timeout_ms = self.timeout_ms;
        let framing = self.framing;
        let tool_name = tool.to_string();
        self.with_retries(|| {
            let params_value = params_value.clone();
            let command = command.clone();
            let args = args.clone();
            async move {
                let mut session =
                    AsyncStdioMcpSession::spawn(&command, &args, timeout_ms, framing).await?;
                session.initialize().await?;
                let result = session.request("tools/call", params_value).await?;
                if let Some(text) = mcp_tool_text(&result) {
                    Ok(text)
                } else {
                    Ok(result.to_string())
                }
            }
        })
        .await
        .map_err(|err| format!("{} (tool={})", err, tool_name))
    }
}

struct AsyncStdioMcpSession {
    child: Child,
    reader: BufReader<ChildStdout>,
    stdin: ChildStdin,
    next_id: u64,
    timeout_ms: u64,
    framing: McpFraming,
}

impl AsyncStdioMcpSession {
    async fn spawn(
        command: &str,
        args: &[String],
        timeout_ms: u64,
        framing: McpFraming,
    ) -> Result<Self, String> {
        let mut child = Command::new(command)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| format!("failed to spawn mcp server: {}", e))?;

        if let Some(mut stderr) = child.stderr.take() {
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                loop {
                    match stderr.read(&mut buf).await {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {}
                    }
                }
            });
        }

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| "missing mcp stdin".to_string())?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "missing mcp stdout".to_string())?;

        Ok(Self {
            child,
            reader: BufReader::new(stdout),
            stdin,
            next_id: 1,
            timeout_ms,
            framing,
        })
    }

    async fn initialize(&mut self) -> Result<(), String> {
        let init_id = self.next_id;
        self.next_id += 1;
        self.write_message(&build_initialize_request(init_id))
            .await?;
        let init_response = self.read_until_id(init_id).await?;
        parse_json_rpc_response(&init_response)?;
        self.write_message(&build_initialized_notification())
            .await?;
        Ok(())
    }

    async fn request(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        let payload = build_json_rpc_request(id, method, params);
        self.write_message(&payload).await?;
        let response = self.read_until_id(id).await?;
        parse_json_rpc_response(&response)
    }

    async fn write_message(&mut self, payload: &str) -> Result<(), String> {
        write_message_async(&mut self.stdin, payload, self.framing).await
    }

    async fn read_until_id(&mut self, id: u64) -> Result<String, String> {
        let started = Instant::now();
        loop {
            if started.elapsed() > Duration::from_millis(self.timeout_ms) {
                let _ = self.child.start_kill();
                return Err("mcp request timed out".to_string());
            }
            let frame = read_message_async(&mut self.reader, self.framing).await?;
            if let Some(value) = response_for_id(&frame, id)? {
                if value.get("error").is_some() {
                    return Err(format!("json-rpc error: {}", value));
                }
                return Ok(frame);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn mock_client_maps_tool_action_kinds() {
        let client = MockMcpClient;
        assert_eq!(client.action_kind_for("read_file").await, ActionKind::Read);
    }

    #[test]
    fn default_filesystem_config_uses_ndjson() {
        let (command, args, framing) =
            default_filesystem_stdio_config(Path::new("tests/fixtures/mcp_fs_root"));
        assert_eq!(command, "node");
        assert!(args
            .last()
            .map(|path| path.contains("mcp_fs_root"))
            .unwrap_or(false));
        assert_eq!(framing, McpFraming::Ndjson);
    }
}
