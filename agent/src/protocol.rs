use serde::{Deserialize, Serialize};
use serde_json::{Value};

use crate::methods;

#[derive(Debug, Deserialize)]
pub struct JsonRpcRequest {
    pub id: Value,
    pub method: String,
    #[serde(default)]
    pub params: Option<Value>,
}

#[derive(Debug, Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: &'static str,
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
}

/// Top-level dispatch. Reads a JSON-RPC line, delegates to the appropriate handler,
/// and returns the JSON response string.
pub async fn handle_request(line: &str) -> String {
    let req: JsonRpcRequest = match serde_json::from_str(line) {
        Ok(r) => r,
        Err(e) => {
            return error_string(Value::Null, -32700, format!("Parse error: {}", e));
        }
    };

    match dispatch(&req.method, req.params.as_ref()).await {
        Ok(result) => success_string(req.id, result),
        Err(e) => error_string(req.id, -1, e.to_string()),
    }
}

async fn dispatch(method: &str, params: Option<&Value>) -> Result<Value, AgentError> {
    let params = params.unwrap_or(&Value::Null);
    match method {
        "handshake" => methods::handshake(params).await,
        "connect" => methods::connect(params).await,
        "test_connection" => methods::test_connection(params).await,
        "list_databases" => methods::list_databases(params).await,
        "list_schemas" => methods::list_schemas(params).await,
        "list_tables" => methods::list_tables(params).await,
        "list_objects" => methods::list_objects(params).await,
        "get_object_source" => methods::get_object_source(params).await,
        "get_table_ddl" => methods::get_table_ddl(params).await,
        "get_columns" => methods::get_columns(params).await,
        "list_indexes" => methods::list_indexes(params).await,
        "list_foreign_keys" => methods::list_foreign_keys(params).await,
        "list_triggers" => methods::list_triggers(params).await,
        "execute_query" => methods::execute_query(params).await,
        "execute_query_page" => methods::execute_query_page(params).await,
        "fetch_query_page" => methods::fetch_query_page(params).await,
        "close_query_session" => methods::close_query_session(params).await,
        "execute_transaction" => methods::execute_transaction(params).await,
        "disconnect" => methods::disconnect(params).await,
        "shutdown" => methods::shutdown(params).await,
        _ => Err(AgentError::MethodNotFound(method.to_string())),
    }
}

fn success_string(id: Value, result: Value) -> String {
    let resp = JsonRpcResponse {
        jsonrpc: "2.0",
        id,
        result: Some(result),
        error: None,
    };
    serde_json::to_string(&resp).unwrap_or_else(|e| {
        format!(
            "{{\"jsonrpc\":\"2.0\",\"id\":null,\"error\":{{\"code\":-1,\"message\":\"{}\"}}}}",
            e
        )
    })
}

fn error_string(id: Value, code: i32, message: String) -> String {
    let resp = JsonRpcResponse {
        jsonrpc: "2.0",
        id,
        result: None,
        error: Some(JsonRpcError { code, message }),
    };
    serde_json::to_string(&resp).unwrap_or_else(|_| {
        "{{\"jsonrpc\":\"2.0\",\"id\":null,\"error\":{{\"code\":-1,\"message\":\"internal serialization error\"}}}}".to_string()
    })
}

#[derive(Debug)]
pub enum AgentError {
    NotConnected,
    Connection(String),
    Database(String),
    Protocol(String),
    Serialization(String),
    MethodNotFound(String),
}

impl AgentError {
    pub fn not_connected() -> Self {
        AgentError::NotConnected
    }

    pub fn connection(msg: impl Into<String>) -> Self {
        AgentError::Connection(msg.into())
    }

    pub fn protocol(msg: impl Into<String>) -> Self {
        AgentError::Protocol(msg.into())
    }
}

impl std::fmt::Display for AgentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentError::NotConnected => write!(f, "Not connected"),
            AgentError::Connection(msg) => write!(f, "Connection error: {}", msg),
            AgentError::Database(msg) => write!(f, "Database error: {}", msg),
            AgentError::Protocol(msg) => write!(f, "Protocol error: {}", msg),
            AgentError::Serialization(msg) => write!(f, "Serialization error: {}", msg),
            AgentError::MethodNotFound(m) => write!(f, "Unknown method: {}", m),
        }
    }
}

impl std::error::Error for AgentError {}

impl From<serde_json::Error> for AgentError {
    fn from(e: serde_json::Error) -> Self {
        AgentError::Serialization(e.to_string())
    }
}

impl From<rust_gaussdb::Error> for AgentError {
    fn from(e: rust_gaussdb::Error) -> Self {
        match e {
            rust_gaussdb::Error::Io(io) => AgentError::Connection(format!("IO error: {}", io)),
            rust_gaussdb::Error::Protocol(msg) => AgentError::Protocol(msg),
            rust_gaussdb::Error::Authentication(msg) => {
                AgentError::Connection(format!("Authentication failed: {}", msg))
            }
            rust_gaussdb::Error::Database(db) => AgentError::Database(db.to_string()),
            rust_gaussdb::Error::Config(msg) => AgentError::Connection(format!("Config error: {}", msg)),
        }
    }
}

/// Extract an optional string parameter from a JSON object.
pub fn string_param(params: &Value, key: &str) -> Option<String> {
    params
        .get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// Extract a required string parameter.
pub fn required_string(params: &Value, key: &str) -> Result<String, AgentError> {
    string_param(params, key)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| AgentError::protocol(format!("missing required parameter: {}", key)))
}

/// Extract an optional i32 parameter.
pub fn int_param(params: &Value, key: &str) -> Option<i32> {
    params.get(key).and_then(|v| v.as_i64()).map(|v| v as i32)
}

/// Extract an optional i32 parameter with a default.
pub fn int_default(params: &Value, key: &str, default: i32) -> i32 {
    int_param(params, key).unwrap_or(default)
}
