use serde_json::{json, Value};
use std::time::Instant;

use crate::connection;
use crate::protocol::{int_default, required_string, string_param, AgentError};
use crate::session::{self, SESSION_IDLE_TIMEOUT};

pub async fn execute_query(params: &Value) -> Result<Value, AgentError> {
    let sql = required_string(params, "sql")?;
    let schema = string_param(params, "schema");
    let max_rows = int_default(params, "maxRows", 10000) as usize;

    let mut guard = connection::conn_mutex().lock().await;
    let client = guard.as_mut().ok_or_else(AgentError::not_connected)?;

    // Apply schema if provided
    if let Some(ref s) = schema {
        client.set_schema(s).await?;
    }

    let trimmed = sql.trim().trim_end_matches(';');

    // Handle special transaction control statements
    match trimmed.to_uppercase().as_str() {
        "BEGIN" | "BEGIN TRANSACTION" => {
            client.execute("BEGIN").await?;
            return Ok(empty_query_result());
        }
        "COMMIT" => {
            client.execute("COMMIT").await?;
            return Ok(empty_query_result());
        }
        "ROLLBACK" => {
            client.execute("ROLLBACK").await?;
            return Ok(empty_query_result());
        }
        _ => {}
    }

    let start = Instant::now();
    let rows = client.query(trimmed, &[]).await?;
    let elapsed = start.elapsed().as_millis() as u64;

    let columns: Vec<String> = if let Some(first) = rows.first() {
        first.columns().iter().map(|c| c.name.clone()).collect()
    } else {
        Vec::new()
    };

    let total = rows.len();
    let truncated = total > max_rows;
    let taken = if truncated { max_rows } else { total };

    let json_rows: Vec<Value> = rows
        .iter()
        .take(taken)
        .map(|r| crate::value::row_to_json(r))
        .collect();

    Ok(json!({
        "columns": columns,
        "rows": json_rows,
        "affected_rows": 0,
        "execution_time_ms": elapsed,
        "truncated": truncated
    }))
}

pub async fn execute_query_page(params: &Value) -> Result<Value, AgentError> {
    let sql = required_string(params, "sql")?;
    let schema = string_param(params, "schema");
    let page_size = int_default(params, "pageSize", 100) as usize;
    let max_rows = int_default(params, "maxRows", 10000) as usize;

    // Clean up expired sessions
    {
        let mut store = session::SESSION_STORE.lock().await;
        store.expire_idle(SESSION_IDLE_TIMEOUT);
    }

    let mut guard = connection::conn_mutex().lock().await;
    let client = guard.as_mut().ok_or_else(AgentError::not_connected)?;

    // Apply schema if provided
    if let Some(ref s) = schema {
        client.set_schema(s).await?;
    }

    let trimmed = sql.trim().trim_end_matches(';');

    let start = Instant::now();
    let rows = client.query(trimmed, &[]).await?;
    let elapsed = start.elapsed().as_millis() as u64;

    let columns: Vec<String> = if let Some(first) = rows.first() {
        first.columns().iter().map(|c| c.name.clone()).collect()
    } else {
        return Ok(json!({
            "columns": [],
            "rows": [],
            "affected_rows": 0,
            "execution_time_ms": elapsed,
            "truncated": false,
            "session_id": null,
            "has_more": false
        }));
    };

    let total = rows.len();
    let truncated = total > max_rows;
    let taken = total.min(max_rows);

    let all_json_rows: Vec<Value> = rows
        .iter()
        .take(taken)
        .map(|r| crate::value::row_to_json(r))
        .collect();

    let session_id = uuid::Uuid::new_v4().to_string();
    let first_page: Vec<Value> = all_json_rows.iter().take(page_size).cloned().collect();
    let has_more = all_json_rows.len() > page_size;

    let session = session::PagedSession {
        session_id: session_id.clone(),
        columns: columns.clone(),
        all_rows: all_json_rows,
        current_position: page_size.min(taken),
        last_accessed: Instant::now(),
    };

    {
        let mut store = session::SESSION_STORE.lock().await;
        store.insert(session);
    }

    Ok(json!({
        "columns": columns,
        "rows": first_page,
        "affected_rows": 0,
        "execution_time_ms": elapsed,
        "truncated": truncated,
        "session_id": session_id,
        "has_more": has_more
    }))
}

pub async fn fetch_query_page(params: &Value) -> Result<Value, AgentError> {
    let session_id = required_string(params, "sessionId")?;
    let page_size = int_default(params, "pageSize", 100) as usize;

    let mut store = session::SESSION_STORE.lock().await;
    store.expire_idle(SESSION_IDLE_TIMEOUT);

    let session = store
        .get_mut(&session_id)
        .ok_or_else(|| AgentError::protocol(format!("Session not found: {}", session_id)))?;

    session.last_accessed = Instant::now();

    let start_pos = session.current_position;
    let end_pos = (start_pos + page_size).min(session.all_rows.len());
    let page: Vec<Value> = session.all_rows[start_pos..end_pos].to_vec();
    session.current_position = end_pos;
    let has_more = end_pos < session.all_rows.len();

    // Clone data we need before the remove call
    let columns = session.columns.clone();
    let sid = if has_more { Some(session_id.clone()) } else { None };

    if !has_more {
        store.remove(&session_id);
    }
    drop(store);

    Ok(json!({
        "columns": columns,
        "rows": page,
        "affected_rows": 0,
        "execution_time_ms": 0,
        "truncated": false,
        "session_id": sid,
        "has_more": has_more
    }))
}

pub async fn close_query_session(params: &Value) -> Result<Value, AgentError> {
    let session_id = required_string(params, "sessionId")?;
    let mut store = session::SESSION_STORE.lock().await;
    let removed = store.remove(&session_id);
    Ok(Value::Bool(removed))
}

fn empty_query_result() -> Value {
    json!({
        "columns": [],
        "rows": [],
        "affected_rows": 0,
        "execution_time_ms": 0,
        "truncated": false
    })
}
