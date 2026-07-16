//! Handshake + dispatch gate: drive the real blocking client against the
//! committed mock stdio server binary (spawned as a child process).

use fm_mcp::McpClient;

#[test]
fn handshake_list_and_call() {
    let bin = env!("CARGO_BIN_EXE_mock_mcp_server");
    let mut client = McpClient::connect(bin, &[]).expect("connect + handshake");

    let tools = client.list_tools().expect("list_tools");
    assert_eq!(tools.len(), 2);
    assert_eq!(tools[0].name, "web_search");
    assert_eq!(tools[0].description, "search the web");
    assert!(tools[0].input_schema.get("type").is_some());

    let res = client
        .call_tool(
            "web_search",
            serde_json::json!({ "query": "nestle annual report" }),
        )
        .expect("call_tool");
    assert!(res.to_string().contains("called web_search"));
}

#[test]
fn connect_bad_command_errs() {
    let err = McpClient::connect("this_binary_does_not_exist_xyz", &[]);
    assert!(err.is_err());
}
