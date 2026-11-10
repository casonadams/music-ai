use std::sync::Arc;

use anyhow::{Context, Result};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    sync::Mutex,
};

pub const LATEST_PROTOCOL: &str = "2025-06-18";
const SUPPORTED_PROTOCOLS: [&str; 3] = [LATEST_PROTOCOL, "2025-03-26", "2024-11-05"];

const PARSE_ERROR: i64 = -32700;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;

#[derive(Debug)]
pub enum ToolError {
    Unknown,
    /// Reported to the model as a tool result with `isError: true`.
    Failed(String),
}

pub trait ToolSet {
    fn instructions(&self) -> &str;
    fn definitions(&self) -> Vec<Value>;
    fn call(
        &self,
        name: &str,
        args: Value,
    ) -> impl Future<Output = Result<Value, ToolError>> + Send;
}

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

fn tool_result(outcome: Result<Value, String>) -> Value {
    match outcome {
        Ok(value) => {
            let text = serde_json::to_string_pretty(&value).unwrap_or_default();
            let mut result = json!({"content": [{"type": "text", "text": text}], "isError": false});
            if value.is_object() {
                result["structuredContent"] = value;
            }
            result
        }
        Err(message) => json!({"content": [{"type": "text", "text": message}], "isError": true}),
    }
}

/// Answers one JSON-RPC message; notifications (no `id`) get no response.
pub async fn handle<T: ToolSet>(tools: &T, message: Value) -> Option<Value> {
    let id = message.get("id").cloned()?;
    let params = message.get("params").cloned().unwrap_or(Value::Null);
    let result = match message["method"].as_str().unwrap_or_default() {
        "initialize" => {
            let requested = params["protocolVersion"].as_str().unwrap_or_default();
            let version = SUPPORTED_PROTOCOLS
                .iter()
                .find(|v| **v == requested)
                .unwrap_or(&LATEST_PROTOCOL);
            json!({
                "protocolVersion": version,
                "capabilities": {"tools": {"listChanged": false}},
                "serverInfo": {"name": "musicai", "version": env!("CARGO_PKG_VERSION")},
                "instructions": tools.instructions(),
            })
        }
        "ping" => json!({}),
        "tools/list" => json!({"tools": tools.definitions()}),
        "tools/call" => {
            let Some(name) = params["name"].as_str() else {
                return Some(error(id, INVALID_PARAMS, "tools/call needs a tool name"));
            };
            let args = match params.get("arguments") {
                None | Some(Value::Null) => json!({}),
                Some(args) => args.clone(),
            };
            match tools.call(name, args).await {
                Ok(value) => tool_result(Ok(value)),
                Err(ToolError::Failed(message)) => tool_result(Err(message)),
                Err(ToolError::Unknown) => {
                    return Some(error(id, INVALID_PARAMS, &format!("unknown tool: {name}")));
                }
            }
        }
        other => {
            return Some(error(
                id,
                METHOD_NOT_FOUND,
                &format!("method not found: {other}"),
            ));
        }
    };
    Some(json!({"jsonrpc": "2.0", "id": id, "result": result}))
}

/// Newline-delimited JSON-RPC over stdin/stdout. Stdout carries protocol
/// messages only; diagnostics go to stderr.
pub async fn serve_stdio<T: ToolSet + Send + Sync + 'static>(tools: Arc<T>) -> Result<()> {
    let stdout = Arc::new(Mutex::new(tokio::io::stdout()));
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut pending = tokio::task::JoinSet::new();
    while let Some(line) = lines.next_line().await.context("reading stdin")? {
        if line.trim().is_empty() {
            continue;
        }
        let tools = Arc::clone(&tools);
        let stdout = Arc::clone(&stdout);
        pending.spawn(async move {
            let response = match serde_json::from_str::<Value>(&line) {
                Ok(message) => handle(tools.as_ref(), message).await,
                Err(e) => Some(error(
                    Value::Null,
                    PARSE_ERROR,
                    &format!("invalid JSON: {e}"),
                )),
            };
            if let Some(response) = response {
                let mut out = response.to_string();
                out.push('\n');
                let mut stdout = stdout.lock().await;
                if let Err(e) = stdout.write_all(out.as_bytes()).await {
                    eprintln!("musicai mcp: writing response failed: {e}");
                } else if let Err(e) = stdout.flush().await {
                    eprintln!("musicai mcp: flushing response failed: {e}");
                }
            }
        });
    }
    while pending.join_next().await.is_some() {}
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake;

    impl ToolSet for Fake {
        fn instructions(&self) -> &str {
            "be nice"
        }

        fn definitions(&self) -> Vec<Value> {
            vec![json!({"name": "echo"})]
        }

        async fn call(&self, name: &str, args: Value) -> Result<Value, ToolError> {
            match name {
                "echo" => Ok(args),
                "boom" => Err(ToolError::Failed("it broke".into())),
                _ => Err(ToolError::Unknown),
            }
        }
    }

    async fn send(message: Value) -> Option<Value> {
        handle(&Fake, message).await
    }

    #[tokio::test]
    async fn initialize_echoes_a_supported_version_else_the_latest() {
        let ask = |v: &str| {
            json!({"jsonrpc": "2.0", "id": 1, "method": "initialize",
                   "params": {"protocolVersion": v}})
        };
        let old = send(ask("2024-11-05")).await.unwrap();
        assert_eq!(old["result"]["protocolVersion"], "2024-11-05");
        let future = send(ask("2099-01-01")).await.unwrap();
        assert_eq!(future["result"]["protocolVersion"], LATEST_PROTOCOL);
        assert_eq!(future["result"]["serverInfo"]["name"], "musicai");
        assert_eq!(future["result"]["instructions"], "be nice");
        assert!(future["result"]["capabilities"]["tools"].is_object());
    }

    #[tokio::test]
    async fn notifications_get_no_response() {
        let note = json!({"jsonrpc": "2.0", "method": "notifications/initialized"});
        assert_eq!(send(note).await, None);
    }

    #[tokio::test]
    async fn lists_tools_and_answers_ping() {
        let list = send(json!({"jsonrpc": "2.0", "id": 7, "method": "tools/list"})).await;
        let list = list.unwrap();
        assert_eq!(list["id"], 7);
        assert_eq!(list["result"]["tools"][0]["name"], "echo");
        let ping = send(json!({"jsonrpc": "2.0", "id": 8, "method": "ping"})).await;
        assert_eq!(ping.unwrap()["result"], json!({}));
    }

    #[tokio::test]
    async fn tool_calls_return_structured_content_and_tool_errors_are_flagged() {
        let call = |name: &str, args: Value| {
            json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
                   "params": {"name": name, "arguments": args}})
        };
        let ok = send(call("echo", json!({"a": 1}))).await.unwrap();
        assert_eq!(ok["result"]["isError"], false);
        assert_eq!(ok["result"]["structuredContent"], json!({"a": 1}));
        assert!(
            ok["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("\"a\"")
        );

        let failed = send(call("boom", json!({}))).await.unwrap();
        assert_eq!(failed["result"]["isError"], true);
        assert_eq!(failed["result"]["content"][0]["text"], "it broke");
    }

    #[tokio::test]
    async fn unknown_tools_and_methods_are_protocol_errors() {
        let tool = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
                          "params": {"name": "nope"}});
        assert_eq!(send(tool).await.unwrap()["error"]["code"], INVALID_PARAMS);
        let method = json!({"jsonrpc": "2.0", "id": 2, "method": "resources/list"});
        assert_eq!(
            send(method).await.unwrap()["error"]["code"],
            METHOD_NOT_FOUND
        );
    }
}
