//! A tiny canned MCP stdio server for the fm-mcp handshake/dispatch gate.
//! Reads newline-delimited JSON-RPC requests, replies with fixed
//! `initialize` / `tools/list` / `tools/call` responses.

use std::io::{BufRead, Write};

fn main() {
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        if line.trim().is_empty() {
            continue;
        }
        let v: serde_json::Value = match serde_json::from_str(line.trim()) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let method = v.get("method").and_then(|m| m.as_str()).unwrap_or("");
        // Notifications carry no id and get no response.
        let id = match v.get("id") {
            Some(id) if !id.is_null() => id.clone(),
            _ => continue,
        };
        let result = match method {
            "initialize" => serde_json::json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "serverInfo": { "name": "mock", "version": "0.1" }
            }),
            "tools/list" => serde_json::json!({ "tools": [
                { "name": "web_search", "description": "search the web", "inputSchema": { "type": "object" } },
                { "name": "read_markdown", "description": "read a page", "inputSchema": { "type": "object" } }
            ]}),
            "tools/call" => {
                let name = v.pointer("/params/name").and_then(|n| n.as_str()).unwrap_or("");
                serde_json::json!({ "content": [{ "type": "text", "text": format!("called {name}") }] })
            }
            _ => serde_json::Value::Null,
        };
        let resp = serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": result });
        if writeln!(out, "{}", serde_json::to_string(&resp).unwrap()).is_err() {
            break;
        }
        let _ = out.flush();
    }
}
