use crate::core::llm_schema::UnifiedChatRequest;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;

#[derive(Debug, Deserialize)]
struct AuditRecord {
    ts: Option<u64>,
    run_id: Option<String>,
    event: Option<String>,
}

pub fn parse_audit_line(line: &str) -> Option<(u64, String, String)> {
    let record: AuditRecord = serde_json::from_str(line).ok()?;
    let ts = record.ts.unwrap_or(0);
    let run_id = record.run_id.unwrap_or_default();
    let event = record.event.unwrap_or_default();
    if event.is_empty() {
        None
    } else {
        Some((ts, run_id, event))
    }
}

pub fn parse_json(payload: &str) -> Result<Value, String> {
    serde_json::from_str(payload).map_err(|e| format!("invalid json: {}", e))
}

pub fn json_string_field(value: &Value, field: &str) -> String {
    match value.get(field) {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Number(number)) => number.to_string(),
        Some(Value::Bool(flag)) => flag.to_string(),
        Some(Value::Null) | None => String::new(),
        Some(other) => other.to_string(),
    }
}

pub fn json_string_field_raw(payload: &str, field: &str) -> String {
    parse_json(payload)
        .map(|value| json_string_field(&value, field))
        .unwrap_or_default()
}

#[derive(Debug, Deserialize)]
pub struct SandboxdTaskSubmit {
    pub id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SandboxdTaskStatus {
    pub status: Option<String>,
    pub result: Option<Value>,
    pub error: Option<Value>,
}

impl SandboxdTaskStatus {
    pub fn result_text(&self) -> String {
        value_as_text(self.result.as_ref())
    }

    pub fn error_text(&self) -> String {
        value_as_text(self.error.as_ref())
    }
}

pub fn parse_sandboxd_task_submit(payload: &str) -> Result<SandboxdTaskSubmit, String> {
    serde_json::from_str(payload).map_err(|e| format!("invalid sandboxd submit response: {}", e))
}

pub fn parse_sandboxd_task_status(payload: &str) -> Result<SandboxdTaskStatus, String> {
    serde_json::from_str(payload).map_err(|e| format!("invalid sandboxd task status: {}", e))
}

#[derive(Debug, Deserialize)]
struct JsonRpcResponse {
    result: Option<Value>,
    error: Option<Value>,
}

pub fn parse_json_rpc_response(payload: &str) -> Result<Value, String> {
    let response: JsonRpcResponse =
        serde_json::from_str(payload).map_err(|e| format!("invalid json-rpc response: {}", e))?;
    if let Some(error) = response.error {
        return Err(format!("json-rpc error: {}", error));
    }
    response
        .result
        .ok_or_else(|| "json-rpc response missing result".to_string())
}

#[derive(Debug, Deserialize)]
struct OpenAiChatResponse {
    choices: Option<Vec<OpenAiChoice>>,
}

#[derive(Debug, Deserialize)]
struct OpenAiChoice {
    message: Option<OpenAiMessage>,
}

#[derive(Debug, Deserialize)]
struct OpenAiMessage {
    content: Option<String>,
}

pub fn parse_openai_chat_content(payload: &str) -> Result<String, String> {
    let response: OpenAiChatResponse = serde_json::from_str(payload)
        .map_err(|e| format!("invalid openai chat response: {}", e))?;
    let content = response
        .choices
        .and_then(|choices| choices.into_iter().next())
        .and_then(|choice| choice.message)
        .and_then(|message| message.content)
        .unwrap_or_default();
    if content.trim().is_empty() {
        Err("openai chat response missing content".to_string())
    } else {
        Ok(content)
    }
}

#[derive(Debug, Deserialize)]
struct OpenAiEmbeddingResponse {
    data: Option<Vec<OpenAiEmbeddingItem>>,
}

#[derive(Debug, Deserialize)]
struct OpenAiEmbeddingItem {
    embedding: Option<Vec<f64>>,
}

pub fn parse_openai_embedding(payload: &str) -> Result<Vec<f64>, String> {
    let response: OpenAiEmbeddingResponse =
        serde_json::from_str(payload).map_err(|e| format!("invalid embedding response: {}", e))?;
    let embedding = response
        .data
        .and_then(|items| items.into_iter().next())
        .and_then(|item| item.embedding)
        .unwrap_or_default();
    if embedding.is_empty() {
        Err("embedding response missing vector".to_string())
    } else {
        Ok(embedding)
    }
}

pub fn mcp_tool_names(result: &Value) -> Vec<String> {
    mcp_tool_definitions(result)
        .into_iter()
        .map(|tool| tool.name)
        .collect()
}

pub fn mcp_tool_definitions(
    result: &Value,
) -> Vec<crate::core::mcp_tool_catalog::McpToolDefinition> {
    use crate::core::mcp_tool_catalog::build_tool_definition;
    let tools = result
        .get("tools")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    tools
        .iter()
        .filter_map(|tool| {
            let name = tool.get("name").and_then(Value::as_str)?;
            let description = tool
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or("");
            Some(build_tool_definition(name, description))
        })
        .collect()
}

pub fn mcp_tool_text(result: &Value) -> Option<String> {
    let content = result.get("content")?.as_array()?;
    for item in content {
        if let Some(text) = item.get("text").and_then(Value::as_str) {
            if !text.trim().is_empty() {
                return Some(text.to_string());
            }
        }
    }
    None
}

pub fn qdrant_payload_hits(
    body: &str,
    query: &str,
    limit: usize,
) -> Result<Vec<(String, String, String)>, String> {
    let value = parse_json(body)?;
    let points = value
        .get("result")
        .and_then(|result| result.get("points"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut hits = Vec::new();
    for point in points {
        let payload = match point.get("payload") {
            Some(payload) => payload,
            None => continue,
        };
        let content = json_string_field(payload, "content");
        if content.is_empty() {
            continue;
        }
        let source = json_string_field(payload, "source");
        let layer = json_string_field(payload, "layer");
        hits.push((content, source, layer));
        if hits.len() >= limit {
            break;
        }
    }

    if hits.is_empty() && !query.trim().is_empty() {
        let _ = query;
    }
    Ok(hits)
}

pub fn qdrant_search_hits(
    body: &str,
    limit: usize,
) -> Result<Vec<(String, String, String, f64)>, String> {
    let value = parse_json(body)?;
    let points = value
        .get("result")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut hits = Vec::new();
    for point in points {
        let payload = match point.get("payload") {
            Some(payload) => payload,
            None => continue,
        };
        let content = json_string_field(payload, "content");
        if content.is_empty() {
            continue;
        }
        let source = json_string_field(payload, "source");
        let layer = json_string_field(payload, "layer");
        let score = point.get("score").and_then(Value::as_f64).unwrap_or(0.0);
        hits.push((content, source, layer, score));
        if hits.len() >= limit {
            break;
        }
    }
    Ok(hits)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxdSseEvent {
    pub id: Option<String>,
    pub event: String,
    pub data: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parsed: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxdEventSummary {
    pub total: usize,
    pub by_event: HashMap<String, usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxdEventCapture {
    pub version: u32,
    pub events: Vec<SandboxdSseEvent>,
    pub summary: SandboxdEventSummary,
}

pub fn parse_sandboxd_sse_capture(raw: &str) -> SandboxdEventCapture {
    let mut events = Vec::new();
    let mut current_id = None::<String>;
    let mut current_event = String::new();
    let mut data_lines = Vec::new();

    for line in raw.lines() {
        if let Some(rest) = line.strip_prefix("id:") {
            current_id = Some(rest.trim().to_string());
            continue;
        }
        if let Some(rest) = line.strip_prefix("event:") {
            current_event = rest.trim().to_string();
            continue;
        }
        if let Some(rest) = line.strip_prefix("data:") {
            data_lines.push(rest.trim().to_string());
            continue;
        }
        if line.trim().is_empty() && (!current_event.is_empty() || !data_lines.is_empty()) {
            events.push(build_sse_event(
                current_id.take(),
                &current_event,
                &data_lines,
            ));
            current_event.clear();
            data_lines.clear();
        }
    }

    if !current_event.is_empty() || !data_lines.is_empty() {
        events.push(build_sse_event(
            current_id.take(),
            &current_event,
            &data_lines,
        ));
    }

    if events.is_empty() && !raw.trim().is_empty() {
        events.push(SandboxdSseEvent {
            id: None,
            event: "raw".to_string(),
            data: raw.to_string(),
            parsed: None,
        });
    }

    let mut by_event = HashMap::new();
    for event in &events {
        *by_event.entry(event.event.clone()).or_insert(0) += 1;
    }

    SandboxdEventCapture {
        version: 1,
        summary: SandboxdEventSummary {
            total: events.len(),
            by_event,
        },
        events,
    }
}

pub fn serialize_sandboxd_event_capture(capture: &SandboxdEventCapture) -> String {
    serde_json::to_string(capture).unwrap_or_else(|_| "{}".to_string())
}

fn build_sse_event(id: Option<String>, event: &str, data_lines: &[String]) -> SandboxdSseEvent {
    let data = data_lines.join("\n");
    let parsed = serde_json::from_str::<Value>(&data).ok();
    SandboxdSseEvent {
        id,
        event: empty_as(event, "message"),
        data,
        parsed,
    }
}

pub fn build_sse_event_records(raw: &str) -> String {
    serialize_sandboxd_event_capture(&parse_sandboxd_sse_capture(raw))
}

pub fn build_chat_payload(model: &str, prompt: &str) -> String {
    build_openai_chat_payload(&UnifiedChatRequest::new(model, prompt))
}

pub fn build_openai_chat_payload(request: &UnifiedChatRequest) -> String {
    json!({
        "model": request.model,
        "messages": [{"role": "user", "content": request.user_prompt}]
    })
    .to_string()
}

pub fn build_anthropic_payload(request: &UnifiedChatRequest) -> String {
    json!({
        "model": request.model,
        "max_tokens": 1024,
        "messages": [{"role": "user", "content": request.user_prompt}]
    })
    .to_string()
}

pub fn build_gemini_payload(request: &UnifiedChatRequest) -> String {
    json!({
        "contents": [{
            "parts": [{"text": request.user_prompt}]
        }]
    })
    .to_string()
}

pub fn build_ollama_payload(request: &UnifiedChatRequest) -> String {
    json!({
        "model": request.model,
        "stream": false,
        "messages": [{"role": "user", "content": request.user_prompt}]
    })
    .to_string()
}

#[derive(Debug, Deserialize)]
struct AnthropicResponse {
    content: Option<Vec<AnthropicContent>>,
}

#[derive(Debug, Deserialize)]
struct AnthropicContent {
    text: Option<String>,
}

pub fn parse_anthropic_content(payload: &str) -> Result<String, String> {
    let response: AnthropicResponse =
        serde_json::from_str(payload).map_err(|e| format!("invalid anthropic response: {}", e))?;
    let content = response
        .content
        .and_then(|items| items.into_iter().next())
        .and_then(|item| item.text)
        .unwrap_or_default();
    if content.trim().is_empty() {
        Err("anthropic response missing content".to_string())
    } else {
        Ok(content)
    }
}

#[derive(Debug, Deserialize)]
struct GeminiResponse {
    candidates: Option<Vec<GeminiCandidate>>,
}

#[derive(Debug, Deserialize)]
struct GeminiCandidate {
    content: Option<GeminiContent>,
}

#[derive(Debug, Deserialize)]
struct GeminiContent {
    parts: Option<Vec<GeminiPart>>,
}

#[derive(Debug, Deserialize)]
struct GeminiPart {
    text: Option<String>,
}

pub fn parse_gemini_content(payload: &str) -> Result<String, String> {
    let response: GeminiResponse =
        serde_json::from_str(payload).map_err(|e| format!("invalid gemini response: {}", e))?;
    let content = response
        .candidates
        .and_then(|items| items.into_iter().next())
        .and_then(|item| item.content)
        .and_then(|content| content.parts)
        .and_then(|parts| parts.into_iter().next())
        .and_then(|part| part.text)
        .unwrap_or_default();
    if content.trim().is_empty() {
        Err("gemini response missing content".to_string())
    } else {
        Ok(content)
    }
}

#[derive(Debug, Deserialize)]
struct OllamaChatResponse {
    message: Option<OllamaMessage>,
}

#[derive(Debug, Deserialize)]
struct OllamaMessage {
    content: Option<String>,
}

pub fn parse_ollama_content(payload: &str) -> Result<String, String> {
    let response: OllamaChatResponse =
        serde_json::from_str(payload).map_err(|e| format!("invalid ollama response: {}", e))?;
    let content = response
        .message
        .and_then(|message| message.content)
        .unwrap_or_default();
    if content.trim().is_empty() {
        Err("ollama response missing content".to_string())
    } else {
        Ok(content)
    }
}

pub fn build_sandboxd_task_payload(prompt: &str, agent: &str) -> String {
    json!({
        "prompt": prompt,
        "agent": agent
    })
    .to_string()
}

pub fn build_mcp_tools_call_payload(tool: &str, params: &str) -> String {
    let arguments = if let Ok(value) = serde_json::from_str::<Value>(params) {
        if value.is_object() {
            value
        } else {
            json!({"input": params})
        }
    } else if params.contains('=') {
        let mut map = serde_json::Map::new();
        for part in params.split_whitespace() {
            if let Some((key, value)) = part.split_once('=') {
                map.insert(key.to_string(), Value::String(value.to_string()));
            }
        }
        if map.is_empty() {
            json!({"input": params})
        } else {
            Value::Object(map)
        }
    } else {
        json!({"input": params})
    };
    json!({
        "name": tool,
        "arguments": arguments
    })
    .to_string()
}

fn value_as_text(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(text)) => text.clone(),
        Some(other) => other.to_string(),
    }
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

    #[test]
    fn parses_sandboxd_task_status() {
        let status =
            parse_sandboxd_task_status(r#"{"status":"succeeded","result":"done","error":null}"#)
                .unwrap();
        assert_eq!(status.status.as_deref(), Some("succeeded"));
        assert_eq!(status.result_text(), "done");
    }

    #[test]
    fn parses_openai_content() {
        let content =
            parse_openai_chat_content(r#"{"choices":[{"message":{"content":"read_file"}}]}"#)
                .unwrap();
        assert_eq!(content, "read_file");
    }

    #[test]
    fn parses_mcp_tools() {
        let result =
            parse_json(r#"{"tools":[{"name":"read_file"},{"name":"list_tools"}]}"#).unwrap();
        assert_eq!(mcp_tool_names(&result), vec!["read_file", "list_tools"]);
    }

    #[test]
    fn parses_sse_capture_with_summary() {
        let raw = "id:1\nevent:log\ndata:{\"level\":\"info\"}\n\nevent:done\ndata:ok\n\n";
        let capture = parse_sandboxd_sse_capture(raw);
        assert_eq!(capture.events.len(), 2);
        assert_eq!(capture.summary.by_event.get("log"), Some(&1));
        assert!(capture.events[0].parsed.is_some());
    }

    #[test]
    fn parses_openai_embedding_vector() {
        let vector = parse_openai_embedding(r#"{"data":[{"embedding":[0.1,0.2]}]}"#).unwrap();
        assert_eq!(vector, vec![0.1, 0.2]);
    }

    #[test]
    fn parses_anthropic_content() {
        let content =
            parse_anthropic_content(r#"{"content":[{"type":"text","text":"read_file"}]}"#).unwrap();
        assert_eq!(content, "read_file");
    }

    #[test]
    fn parses_gemini_content() {
        let content =
            parse_gemini_content(r#"{"candidates":[{"content":{"parts":[{"text":"hello"}]}}]}"#)
                .unwrap();
        assert_eq!(content, "hello");
    }

    #[test]
    fn parses_ollama_content() {
        let content = parse_ollama_content(r#"{"message":{"content":"hi"}}"#).unwrap();
        assert_eq!(content, "hi");
    }
}
