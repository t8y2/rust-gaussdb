use serde_json::{json, Value};
use std::time::Instant;

use crate::connection;
use crate::protocol::{string_param, AgentError};

pub async fn execute_transaction(params: &Value) -> Result<Value, AgentError> {
    let statements: Vec<String> = params
        .get("statements")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    if statements.is_empty() {
        return Err(AgentError::protocol("no statements provided"));
    }

    let schema = string_param(params, "schema");

    let mut guard = connection::conn_mutex().lock().await;
    let client = guard.as_mut().ok_or_else(AgentError::not_connected)?;

    if let Some(ref s) = schema {
        client.set_schema(s).await?;
    }

    let start = Instant::now();
    client.begin().await?;

    let mut total_affected = 0u64;
    for stmt in &statements {
        let trimmed = stmt.trim().trim_end_matches(';');
        if trimmed.is_empty() {
            continue;
        }
        match client.execute(trimmed).await {
            Ok(affected) => total_affected += affected,
            Err(e) => {
                let _ = client.rollback().await;
                return Err(AgentError::from(e));
            }
        }
    }

    client.commit().await?;
    let elapsed = start.elapsed().as_millis() as u64;

    Ok(json!({
        "columns": [],
        "rows": [],
        "affected_rows": total_affected,
        "execution_time_ms": elapsed,
        "truncated": false
    }))
}
