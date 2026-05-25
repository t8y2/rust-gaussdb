use serde_json::{json, Value};

use crate::connection;
use crate::protocol::AgentError;

pub async fn handshake(_params: &Value) -> Result<Value, AgentError> {
    Ok(json!({
        "protocolVersion": 1,
        "agentProtocolVersion": 1,
        "capabilities": [
            "connect",
            "test_connection",
            "metadata",
            "query",
            "paged_query",
            "transaction",
            "ddl"
        ]
    }))
}

pub async fn connect(params: &Value) -> Result<Value, AgentError> {
    let config = connection::config_from_params(params)?;
    let client = rust_gaussdb::Client::connect_with_config(&config).await.map_err(|e| {
        AgentError::connection(format!("Failed to connect: {}", e))
    })?;
    connection::set_connection(client).await;
    Ok(json!({"ok": true}))
}

pub async fn test_connection(params: &Value) -> Result<Value, AgentError> {
    let config = connection::config_from_params(params)?;
    let mut client = rust_gaussdb::Client::connect_with_config(&config).await.map_err(|e| {
        AgentError::connection(format!("Connection test failed: {}", e))
    })?;
    // Verify by executing a simple query
    client.query("SELECT 1", &[]).await.map_err(|e| {
        AgentError::connection(format!("Connection test query failed: {}", e))
    })?;
    let _ = client.close().await;
    Ok(json!({"ok": true}))
}

pub async fn disconnect(_params: &Value) -> Result<Value, AgentError> {
    // Close all paged query sessions first
    crate::session::SESSION_STORE.lock().await.clear_all();
    connection::clear_connection().await;
    Ok(json!({"ok": true}))
}

pub async fn shutdown(_params: &Value) -> Result<Value, AgentError> {
    disconnect(&Value::Null).await?;
    std::process::exit(0);
}
