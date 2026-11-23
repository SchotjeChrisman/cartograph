//! JSON-RPC 2.0 framing for MCP over stdio: one JSON message per line.

use serde_json::{Value, json};

pub const PARSE_ERROR: i64 = -32700;
pub const INVALID_REQUEST: i64 = -32600;
pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;

/// A request or notification. A notification has no `id` and gets no reply.
pub struct Message {
    pub id: Option<Value>,
    pub method: String,
    pub params: Value,
}

pub fn parse(line: &str) -> Result<Message, Value> {
    let value: Value = serde_json::from_str(line)
        .map_err(|error| error_reply(Value::Null, PARSE_ERROR, &error.to_string()))?;
    let id = value.get("id").cloned();
    let method = value["method"].as_str().ok_or_else(|| {
        error_reply(
            id.clone().unwrap_or(Value::Null),
            INVALID_REQUEST,
            "missing `method`",
        )
    })?;
    Ok(Message {
        id,
        method: method.to_string(),
        params: value.get("params").cloned().unwrap_or(Value::Null),
    })
}

pub fn result_reply(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

pub fn error_reply(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}
