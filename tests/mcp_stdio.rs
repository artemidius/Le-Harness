use std::time::Duration;

use le_harness::mcp::{McpConfig, McpManager};

#[tokio::test]
async fn discovers_and_calls_a_stdio_fixture_server() {
    let Some(python) = [
        "/opt/homebrew/bin/python3",
        "/usr/local/bin/python3",
        "/usr/bin/python3",
    ]
    .into_iter()
    .find(|path| std::path::Path::new(path).is_file()) else {
        return;
    };
    let fixture = format!(
        "{}/tests/fixtures/mcp_server.py",
        env!("CARGO_MANIFEST_DIR")
    );
    let config: McpConfig = serde_json::from_value(serde_json::json!({
        "servers": {
            "fixture": {
                "program": python,
                "args": [fixture],
                "tool_effects": {"echo": "read"}
            }
        }
    }))
    .unwrap();
    let manager = McpManager::new(config, Duration::from_secs(5), 4096);
    let tools = manager.discover().await.unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].qualified_name, "mcp__fixture__echo");
    assert_eq!(tools[0].input_schema["required"][0], "message");

    let result = manager
        .call("mcp__fixture__echo", r#"{"message":"bonjour"}"#)
        .await
        .unwrap();
    assert!(result.contains("bonjour"));
}

#[tokio::test]
async fn refuses_unclassified_server_tools() {
    let Some(python) = [
        "/opt/homebrew/bin/python3",
        "/usr/local/bin/python3",
        "/usr/bin/python3",
    ]
    .into_iter()
    .find(|path| std::path::Path::new(path).is_file()) else {
        return;
    };
    let fixture = format!(
        "{}/tests/fixtures/mcp_server.py",
        env!("CARGO_MANIFEST_DIR")
    );
    let config: McpConfig = serde_json::from_value(serde_json::json!({
        "servers": {"fixture": {"program": python, "args": [fixture], "tool_effects": {}}}
    }))
    .unwrap();
    let manager = McpManager::new(config, Duration::from_secs(5), 4096);
    assert!(manager.discover().await.is_err());
}
