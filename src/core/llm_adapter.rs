use crate::core::config::AppConfig;
use crate::core::http_client::{shell_output, shell_quote};
use crate::core::json_util::{
    build_anthropic_payload, build_gemini_payload, build_ollama_payload, build_openai_chat_payload,
    parse_anthropic_content, parse_gemini_content, parse_ollama_content, parse_openai_chat_content,
};
use crate::core::llm_schema::UnifiedChatRequest;
use crate::core::mcp_tool_catalog::{action_kind_for_tool, tool_names, McpToolDefinition};
use crate::core::policy::ActionKind;
use async_trait::async_trait;

#[derive(Debug, Clone)]
pub struct ToolCallPlan {
    pub tool: String,
    pub params: String,
    pub action_kind: ActionKind,
    pub estimated_tokens: usize,
}

#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn propose_plan(&self, prompt: &str, catalog: &[McpToolDefinition]) -> ToolCallPlan;
    async fn summarize(&self, prompt: &str, tool_result: &str) -> String;
}

#[async_trait]
impl LlmProvider for Box<dyn LlmProvider> {
    async fn propose_plan(&self, prompt: &str, catalog: &[McpToolDefinition]) -> ToolCallPlan {
        self.as_ref().propose_plan(prompt, catalog).await
    }

    async fn summarize(&self, prompt: &str, tool_result: &str) -> String {
        self.as_ref().summarize(prompt, tool_result).await
    }
}

pub fn build_llm_provider(cfg: &AppConfig) -> Box<dyn LlmProvider> {
    match cfg.llm_backend.as_str() {
        "openai-compatible" => Box::new(HttpLlmProvider {
            api_url: cfg.llm_api_url.clone(),
            api_key: cfg.llm_api_key.clone(),
            model: cfg.llm_model.clone(),
            provider: LlmWireFormat::OpenAiCompatible,
        }),
        "anthropic" => Box::new(HttpLlmProvider {
            api_url: cfg.llm_api_url.clone(),
            api_key: cfg.llm_api_key.clone(),
            model: cfg.llm_model.clone(),
            provider: LlmWireFormat::Anthropic,
        }),
        "gemini" => Box::new(HttpLlmProvider {
            api_url: cfg.llm_api_url.clone(),
            api_key: cfg.llm_api_key.clone(),
            model: cfg.llm_model.clone(),
            provider: LlmWireFormat::Gemini,
        }),
        "ollama" => Box::new(HttpLlmProvider {
            api_url: cfg.llm_api_url.clone(),
            api_key: String::new(),
            model: cfg.llm_model.clone(),
            provider: LlmWireFormat::Ollama,
        }),
        _ => Box::new(MockLlmProvider::default()),
    }
}

#[derive(Debug, Clone, Copy)]
enum LlmWireFormat {
    OpenAiCompatible,
    Anthropic,
    Gemini,
    Ollama,
}

#[derive(Debug, Default, Clone)]
pub struct MockLlmProvider;

#[async_trait]
impl LlmProvider for MockLlmProvider {
    async fn propose_plan(&self, prompt: &str, catalog: &[McpToolDefinition]) -> ToolCallPlan {
        let tools = tool_names(catalog);
        let lower = prompt.to_ascii_lowercase();
        let selected = if lower.contains("mystery") || lower.contains("shell") {
            "run_shell".to_string()
        } else if tools.iter().any(|tool| tool == "read_file") {
            "read_file".to_string()
        } else {
            tools
                .first()
                .cloned()
                .unwrap_or_else(|| "list_tools".to_string())
        };

        ToolCallPlan {
            tool: selected.clone(),
            params: format!("prompt={}", prompt),
            action_kind: action_kind_for_tool(catalog, &selected),
            estimated_tokens: 120,
        }
    }

    async fn summarize(&self, prompt: &str, tool_result: &str) -> String {
        format!(
            "ozr summary\n- prompt: {}\n- tool result: {}\n- status: done",
            prompt, tool_result
        )
    }
}

#[derive(Debug, Clone)]
pub struct HttpLlmProvider {
    pub api_url: String,
    pub api_key: String,
    pub model: String,
    provider: LlmWireFormat,
}

impl HttpLlmProvider {
    pub fn is_ready(&self) -> bool {
        !self.api_url.trim().is_empty() && !self.model.trim().is_empty()
    }

    pub(crate) fn build_curl_command(&self, prompt: &str) -> Result<String, String> {
        if !self.is_ready() {
            return Err("llm provider is not configured".to_string());
        }
        let request = UnifiedChatRequest::new(&self.model, prompt);
        let (payload, headers) = match self.provider {
            LlmWireFormat::OpenAiCompatible => (
                build_openai_chat_payload(&request),
                vec![(
                    "Authorization".to_string(),
                    format!("Bearer {}", self.api_key.trim()),
                )],
            ),
            LlmWireFormat::Anthropic => (
                build_anthropic_payload(&request),
                vec![
                    ("x-api-key".to_string(), self.api_key.trim().to_string()),
                    (
                        "anthropic-version".to_string(),
                        "2023-06-01".to_string(),
                    ),
                ],
            ),
            LlmWireFormat::Gemini => (build_gemini_payload(&request), Vec::new()),
            LlmWireFormat::Ollama => (build_ollama_payload(&request), Vec::new()),
        };

        let url = match self.provider {
            LlmWireFormat::Gemini if self.api_url.contains('?') => self.api_url.clone(),
            LlmWireFormat::Gemini => format!(
                "{}?key={}",
                self.api_url.trim_end_matches('/'),
                self.api_key.trim()
            ),
            _ => self.api_url.clone(),
        };

        let mut header_args = String::from("-H 'content-type: application/json'");
        for (name, value) in headers {
            header_args.push_str(&format!(" -H '{}: {}'", name, value.replace('\'', "'\\''")));
        }
        Ok(format!(
            "curl -sS -XPOST {} {} -d {}",
            shell_quote(&url),
            header_args,
            shell_quote(&payload)
        ))
    }

    async fn chat(&self, prompt: &str) -> Result<String, String> {
        let cmd = self.build_curl_command(prompt)?;
        let output = shell_output(&cmd).await?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
        }
        let body = String::from_utf8_lossy(&output.stdout).trim().to_string();
        match self.provider {
            LlmWireFormat::OpenAiCompatible => {
                parse_openai_chat_content(&body).or_else(|_| Ok(body))
            }
            LlmWireFormat::Anthropic => parse_anthropic_content(&body).or_else(|_| Ok(body)),
            LlmWireFormat::Gemini => parse_gemini_content(&body).or_else(|_| Ok(body)),
            LlmWireFormat::Ollama => parse_ollama_content(&body).or_else(|_| Ok(body)),
        }
    }

    async fn build_plan(&self, prompt: &str, catalog: &[McpToolDefinition]) -> ToolCallPlan {
        let tools = tool_names(catalog);
        let tool_list = if tools.is_empty() {
            "none".to_string()
        } else {
            tools.join(", ")
        };
        let request = format!(
            "Choose one tool name from [{}] for this prompt and reply with only the tool name: {}",
            tool_list, prompt
        );
        let reply = self.chat(&request).await.unwrap_or_default();
        let selected = select_tool_from_reply(&reply, &tools);

        ToolCallPlan {
            tool: selected.clone(),
            params: format!("prompt={}", prompt),
            action_kind: action_kind_for_tool(catalog, &selected),
            estimated_tokens: 400,
        }
    }

    async fn build_summary(&self, prompt: &str, tool_result: &str) -> String {
        let request = format!(
            "Summarize this task for the user. Prompt: {} Tool result: {}",
            prompt, tool_result
        );
        self.chat(&request).await.unwrap_or_else(|_| {
            format!(
                "ozr summary\n- prompt: {}\n- tool result: {}\n- status: done",
                prompt, tool_result
            )
        })
    }
}

#[async_trait]
impl LlmProvider for HttpLlmProvider {
    async fn propose_plan(&self, prompt: &str, catalog: &[McpToolDefinition]) -> ToolCallPlan {
        self.build_plan(prompt, catalog).await
    }

    async fn summarize(&self, prompt: &str, tool_result: &str) -> String {
        self.build_summary(prompt, tool_result).await
    }
}

#[derive(Debug, Clone)]
pub struct CurlLlmProvider {
    pub api_url: String,
    pub api_key: String,
    pub model: String,
}

impl CurlLlmProvider {
    pub fn from_openai_config(cfg: &AppConfig) -> Self {
        Self {
            api_url: cfg.llm_api_url.clone(),
            api_key: cfg.llm_api_key.clone(),
            model: cfg.llm_model.clone(),
        }
    }

    fn as_http_provider(&self) -> HttpLlmProvider {
        HttpLlmProvider {
            api_url: self.api_url.clone(),
            api_key: self.api_key.clone(),
            model: self.model.clone(),
            provider: LlmWireFormat::OpenAiCompatible,
        }
    }
}

#[async_trait]
impl LlmProvider for CurlLlmProvider {
    async fn propose_plan(&self, prompt: &str, catalog: &[McpToolDefinition]) -> ToolCallPlan {
        self.as_http_provider()
            .build_plan(prompt, catalog)
            .await
    }

    async fn summarize(&self, prompt: &str, tool_result: &str) -> String {
        self.as_http_provider()
            .build_summary(prompt, tool_result)
            .await
    }
}

fn select_tool_from_reply(reply: &str, tools: &[String]) -> String {
    tools
        .iter()
        .find(|tool| reply.contains(tool.as_str()))
        .cloned()
        .unwrap_or_else(|| {
            if tools.iter().any(|t| t == "read_file") {
                "read_file".to_string()
            } else {
                tools
                    .first()
                    .cloned()
                    .unwrap_or_else(|| "list_tools".to_string())
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::mcp_tool_catalog::build_tool_definition;

    #[tokio::test]
    async fn build_llm_provider_defaults_to_mock() {
        let cfg = AppConfig::default();
        let provider = build_llm_provider(&cfg);
        let catalog = vec![build_tool_definition("read_file", "read a file")];
        let plan = provider.propose_plan("hello", &catalog).await;
        assert_eq!(plan.tool, "read_file");
        assert_eq!(plan.action_kind, ActionKind::Read);
    }

    #[tokio::test]
    async fn mock_provider_syncs_write_action_kind_from_catalog() {
        let provider = MockLlmProvider::default();
        let catalog = vec![
            build_tool_definition("write_file", "write a file"),
            build_tool_definition("read_file", "read a file"),
        ];
        let plan = provider.propose_plan("write config", &catalog).await;
        assert_eq!(plan.tool, "read_file");
        assert_eq!(plan.action_kind, ActionKind::Read);
    }

    #[tokio::test]
    async fn mock_provider_uses_catalog_when_read_file_missing() {
        let provider = MockLlmProvider::default();
        let catalog = vec![build_tool_definition("write_file", "write a file")];
        let plan = provider.propose_plan("write config", &catalog).await;
        assert_eq!(plan.tool, "write_file");
        assert_eq!(plan.action_kind, ActionKind::Write);
    }

    #[test]
    fn select_tool_from_reply_matches_known_tool() {
        let selected = select_tool_from_reply("use read_file", &["read_file".to_string()]);
        assert_eq!(selected, "read_file");
    }

    #[test]
    fn http_provider_builds_curl_command_for_openai() {
        let provider = HttpLlmProvider {
            api_url: "https://api.openai.com/v1/chat/completions".to_string(),
            api_key: "test-key".to_string(),
            model: "gpt-4o-mini".to_string(),
            provider: LlmWireFormat::OpenAiCompatible,
        };
        let cmd = provider
            .build_curl_command("hello")
            .expect("curl command should build");
        assert!(cmd.contains("curl -sS -XPOST"));
        assert!(cmd.contains("api.openai.com"));
        assert!(cmd.contains("Authorization"));
    }
}
