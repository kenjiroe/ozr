use serde_json::{json, Value};
use std::io::{BufRead, Write};

pub const MCP_PROTOCOL_VERSION: &str = "2025-03-26";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpFraming {
    Ndjson,
    ContentLength,
}

impl McpFraming {
    pub fn from_env(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "content-length" | "content_length" | "legacy" => Self::ContentLength,
            _ => Self::Ndjson,
        }
    }
}

pub fn build_initialize_request(id: u64) -> String {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "initialize",
        "params": {
            "protocolVersion": MCP_PROTOCOL_VERSION,
            "capabilities": {},
            "clientInfo": {"name": "ozr", "version": "0.1.0"}
        }
    })
    .to_string()
}

pub fn build_initialized_notification() -> String {
    json!({
        "jsonrpc": "2.0",
        "method": "notifications/initialized"
    })
    .to_string()
}

pub fn build_json_rpc_request(id: u64, method: &str, params: Value) -> String {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": params
    })
    .to_string()
}

pub fn read_message<R: BufRead>(reader: &mut R, framing: McpFraming) -> Result<String, String> {
    match framing {
        McpFraming::Ndjson => read_ndjson_message(reader),
        McpFraming::ContentLength => read_content_length_frame(reader),
    }
}

pub fn write_message<W: Write>(
    writer: &mut W,
    payload: &str,
    framing: McpFraming,
) -> Result<(), String> {
    match framing {
        McpFraming::Ndjson => write_ndjson_message(writer, payload),
        McpFraming::ContentLength => write_content_length_frame(writer, payload),
    }
}

pub fn read_ndjson_message<R: BufRead>(reader: &mut R) -> Result<String, String> {
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .map_err(|e| format!("mcp ndjson read failed: {}", e))?;
    if line.trim().is_empty() {
        return Err("mcp ndjson empty frame".to_string());
    }
    Ok(line.trim_end_matches(['\r', '\n']).to_string())
}

pub fn write_ndjson_message<W: Write>(writer: &mut W, payload: &str) -> Result<(), String> {
    writer
        .write_all(payload.as_bytes())
        .map_err(|e| e.to_string())?;
    writer.write_all(b"\n").map_err(|e| e.to_string())?;
    writer.flush().map_err(|e| e.to_string())
}

pub fn read_content_length_frame<R: BufRead>(reader: &mut R) -> Result<String, String> {
    let mut header = String::new();
    loop {
        let mut byte = [0u8; 1];
        reader
            .read_exact(&mut byte)
            .map_err(|e| format!("mcp frame header read failed: {}", e))?;
        header.push(byte[0] as char);
        if header.ends_with("\r\n\r\n") {
            break;
        }
        if header.len() > 4096 {
            return Err("mcp frame header too large".to_string());
        }
    }

    let mut content_length = None;
    for line in header.lines() {
        let lower = line.to_ascii_lowercase();
        if let Some(rest) = lower.strip_prefix("content-length:") {
            content_length = rest.trim().parse::<usize>().ok();
        }
    }
    let length = content_length.ok_or_else(|| "mcp frame missing Content-Length".to_string())?;
    let mut body = vec![0u8; length];
    reader
        .read_exact(&mut body)
        .map_err(|e| format!("mcp frame body read failed: {}", e))?;
    String::from_utf8(body).map_err(|e| format!("mcp frame body utf8 error: {}", e))
}

pub fn write_content_length_frame<W: Write>(writer: &mut W, payload: &str) -> Result<(), String> {
    let header = format!("Content-Length: {}\r\n\r\n", payload.len());
    writer
        .write_all(header.as_bytes())
        .map_err(|e| e.to_string())?;
    writer
        .write_all(payload.as_bytes())
        .map_err(|e| e.to_string())?;
    writer.flush().map_err(|e| e.to_string())
}

pub fn response_for_id(payload: &str, id: u64) -> Result<Option<Value>, String> {
    let value: Value = serde_json::from_str(payload).map_err(|e| e.to_string())?;
    if value.get("id").and_then(Value::as_u64) == Some(id) {
        return Ok(Some(value));
    }
    Ok(None)
}

pub async fn read_message_async<R: tokio::io::AsyncBufReadExt + Unpin>(
    reader: &mut R,
    framing: McpFraming,
) -> Result<String, String> {
    match framing {
        McpFraming::Ndjson => read_ndjson_message_async(reader).await,
        McpFraming::ContentLength => read_content_length_frame_async(reader).await,
    }
}

pub async fn write_message_async<W: tokio::io::AsyncWriteExt + Unpin>(
    writer: &mut W,
    payload: &str,
    framing: McpFraming,
) -> Result<(), String> {
    match framing {
        McpFraming::Ndjson => write_ndjson_message_async(writer, payload).await,
        McpFraming::ContentLength => write_content_length_frame_async(writer, payload).await,
    }
}

pub async fn read_ndjson_message_async<R: tokio::io::AsyncBufReadExt + Unpin>(
    reader: &mut R,
) -> Result<String, String> {
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .await
        .map_err(|e| format!("mcp ndjson read failed: {}", e))?;
    if line.trim().is_empty() {
        return Err("mcp ndjson empty frame".to_string());
    }
    Ok(line.trim_end_matches(['\r', '\n']).to_string())
}

pub async fn write_ndjson_message_async<W: tokio::io::AsyncWriteExt + Unpin>(
    writer: &mut W,
    payload: &str,
) -> Result<(), String> {
    writer
        .write_all(payload.as_bytes())
        .await
        .map_err(|e| e.to_string())?;
    writer
        .write_all(b"\n")
        .await
        .map_err(|e| e.to_string())?;
    writer.flush().await.map_err(|e| e.to_string())
}

pub async fn read_content_length_frame_async<R: tokio::io::AsyncReadExt + Unpin>(
    reader: &mut R,
) -> Result<String, String> {
    let mut header = String::new();
    loop {
        let mut byte = [0u8; 1];
        reader
            .read_exact(&mut byte)
            .await
            .map_err(|e| format!("mcp frame header read failed: {}", e))?;
        header.push(byte[0] as char);
        if header.ends_with("\r\n\r\n") {
            break;
        }
        if header.len() > 4096 {
            return Err("mcp frame header too large".to_string());
        }
    }

    let mut content_length = None;
    for line in header.lines() {
        let lower = line.to_ascii_lowercase();
        if let Some(rest) = lower.strip_prefix("content-length:") {
            content_length = rest.trim().parse::<usize>().ok();
        }
    }
    let length = content_length.ok_or_else(|| "mcp frame missing Content-Length".to_string())?;
    let mut body = vec![0u8; length];
    reader
        .read_exact(&mut body)
        .await
        .map_err(|e| format!("mcp frame body read failed: {}", e))?;
    String::from_utf8(body).map_err(|e| format!("mcp frame body utf8 error: {}", e))
}

pub async fn write_content_length_frame_async<W: tokio::io::AsyncWriteExt + Unpin>(
    writer: &mut W,
    payload: &str,
) -> Result<(), String> {
    let header = format!("Content-Length: {}\r\n\r\n", payload.len());
    writer
        .write_all(header.as_bytes())
        .await
        .map_err(|e| e.to_string())?;
    writer
        .write_all(payload.as_bytes())
        .await
        .map_err(|e| e.to_string())?;
    writer.flush().await.map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn roundtrips_content_length_frame() {
        let payload = r#"{"jsonrpc":"2.0","id":1,"result":{}}"#;
        let mut buffer = Vec::new();
        write_message(&mut buffer, payload, McpFraming::ContentLength).expect("write");
        let mut cursor = Cursor::new(buffer);
        let decoded = read_message(&mut cursor, McpFraming::ContentLength).expect("read");
        assert_eq!(decoded, payload);
    }

    #[test]
    fn roundtrips_ndjson_frame() {
        let payload = r#"{"jsonrpc":"2.0","id":1,"result":{}}"#;
        let mut buffer = Vec::new();
        write_message(&mut buffer, payload, McpFraming::Ndjson).expect("write");
        let mut cursor = Cursor::new(buffer);
        let decoded = read_message(&mut cursor, McpFraming::Ndjson).expect("read");
        assert_eq!(decoded, payload);
    }
}
