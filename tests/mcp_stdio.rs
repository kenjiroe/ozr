use ozr::core::mcp_client::{McpClient, StdioMcpClient};
use ozr::core::mcp_protocol::McpFraming;
use ozr::core::policy::ActionKind;

fn fixture_server_path() -> String {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    format!("{}/tests/fixtures/mcp_minimal_server.py", manifest_dir)
}

fn fixture_client() -> StdioMcpClient {
    StdioMcpClient::new(
        "python3".to_string(),
        vec![fixture_server_path()],
        5_000,
        2,
        McpFraming::Ndjson,
    )
}

#[tokio::test]
async fn mcp_stdio_lists_fixture_tools() {
    let client = fixture_client();
    let tools = client.list_tools().await;
    assert!(tools.iter().any(|tool| tool == "read_file"));
    assert!(tools.iter().any(|tool| tool == "write_file"));
}

#[tokio::test]
async fn mcp_stdio_calls_read_file_fixture() {
    let client = fixture_client();
    let result = client
        .call_tool("read_file", r#"{"path":".ozr/config.env"}"#)
        .await
        .expect("call should succeed");
    assert!(result.contains("fixture read ok"));
}

#[tokio::test]
async fn mcp_stdio_maps_write_file_to_write_kind() {
    let client = fixture_client();
    assert_eq!(client.action_kind_for("write_file").await, ActionKind::Write);
    assert_eq!(client.action_kind_for("read_file").await, ActionKind::Read);
}
