use rust_gaussdb::Client;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::protocol::AgentError;

/// Global connection state. The agent process handles one connection at a time.
static CONNECTION: std::sync::LazyLock<Arc<Mutex<Option<Client>>>> =
    std::sync::LazyLock::new(|| Arc::new(Mutex::new(None)));

/// Set the global connection (after connect).
pub async fn set_connection(client: Client) {
    let mut guard = CONNECTION.lock().await;
    *guard = Some(client);
}

/// Remove the global connection (after disconnect/shutdown).
pub async fn clear_connection() {
    let mut guard = CONNECTION.lock().await;
    if let Some(client) = guard.take() {
        let _ = client.close().await;
    }
}

/// Get a reference to the global connection mutex.
/// Callers should lock this to access the client.
pub fn conn_mutex() -> &'static Arc<Mutex<Option<Client>>> {
    &CONNECTION
}

/// Build a rust_gaussdb Config from DBX ConnectParams.
pub fn config_from_params(params: &serde_json::Value) -> Result<rust_gaussdb::Config, AgentError> {
    use crate::protocol::{required_string, string_param};

    // If connection_string is provided, use it directly
    if let Some(cs) = string_param(params, "connection_string") {
        if !cs.is_empty() {
            return rust_gaussdb::Config::parse(&cs).map_err(|e| {
                AgentError::connection(format!("Invalid connection string: {}", e))
            });
        }
    }

    let host = string_param(params, "host").unwrap_or_else(|| "127.0.0.1".into());
    let port = crate::protocol::int_default(params, "port", 5432) as u16;
    let username = required_string(params, "username")?;
    let password = string_param(params, "password").unwrap_or_default();
    let database = string_param(params, "database").unwrap_or_else(|| "postgres".into());

    Ok(rust_gaussdb::Config {
        host,
        port,
        user: username,
        password,
        dbname: database,
        application_name: Some("dbx-agent".into()),
        connect_timeout: Some(std::time::Duration::from_secs(10)),
    })
}
