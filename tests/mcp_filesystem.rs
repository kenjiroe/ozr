use ozr::core::mcp_client::{default_filesystem_stdio_config, McpClient, StdioMcpClient};
use ozr::core::policy::ActionKind;
use std::path::Path;

fn filesystem_server_installed() -> bool {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    Path::new(manifest_dir)
        .join("tests/fixtures/mcp-filesystem/node_modules/@modelcontextprotocol/server-filesystem/dist/index.js")
        .exists()
}

fn filesystem_client() -> StdioMcpClient {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let root = Path::new(manifest_dir).join("tests/fixtures/mcp_fs_root");
    let (command, args, framing) = default_filesystem_stdio_config(&root);
    StdioMcpClient::new(command, args, 10_000, 2, framing)
}

#[tokio::test]
async fn mcp_filesystem_lists_official_tools() {
    if !filesystem_server_installed() {
        eprintln!("skip: run npm install --prefix tests/fixtures/mcp-filesystem");
        return;
    }
    let client = filesystem_client();
    let tools = client.list_tools().await;
    assert!(
        tools.iter().any(|tool| tool == "read_text_file"),
        "expected read_text_file in {:?}",
        tools
    );
    assert!(
        tools.iter().any(|tool| tool == "write_file"),
        "expected write_file in {:?}",
        tools
    );
    assert!(tools.len() >= 10, "expected full catalog, got {:?}", tools);
}

#[tokio::test]
async fn mcp_filesystem_reads_sample_file() {
    if !filesystem_server_installed() {
        eprintln!("skip: run npm install --prefix tests/fixtures/mcp-filesystem");
        return;
    }
    let client = filesystem_client();
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let path = format!("{}/tests/fixtures/mcp_fs_root/sample.txt", manifest_dir);
    let result = client
        .call_tool("read_text_file", &format!(r#"{{"path":"{}"}}"#, path))
        .await
        .expect("read_text_file should succeed");
    assert!(result.contains("hello from real mcp fs"), "got: {}", result);
}

#[tokio::test]
async fn mcp_filesystem_maps_action_kinds() {
    if !filesystem_server_installed() {
        eprintln!("skip: run npm install --prefix tests/fixtures/mcp-filesystem");
        return;
    }
    let client = filesystem_client();
    assert_eq!(client.action_kind_for("read_text_file").await, ActionKind::Read);
    assert_eq!(client.action_kind_for("list_directory").await, ActionKind::Read);
    assert_eq!(client.action_kind_for("write_file").await, ActionKind::Write);
    assert_eq!(client.action_kind_for("edit_file").await, ActionKind::Write);
    assert_eq!(client.action_kind_for("move_file").await, ActionKind::Write);
}

#[tokio::test]
async fn mcp_filesystem_lists_allowed_directories() {
    if !filesystem_server_installed() {
        eprintln!("skip: run npm install --prefix tests/fixtures/mcp-filesystem");
        return;
    }
    let client = filesystem_client();
    let result = client
        .call_tool("list_allowed_directories", "{}")
        .await
        .expect("list_allowed_directories should succeed");
    assert!(result.contains("mcp_fs_root"), "got: {}", result);
}
