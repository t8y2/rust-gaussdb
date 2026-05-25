use serde_json::{json, Value};

use crate::connection;
use crate::protocol::{required_string, string_param, AgentError};

pub async fn list_databases(_params: &Value) -> Result<Value, AgentError> {
    let mut guard = connection::conn_mutex().lock().await;
    let client = guard.as_mut().ok_or_else(AgentError::not_connected)?;

    let rows = client
        .query(
            "SELECT datname FROM pg_database WHERE datistemplate = false ORDER BY datname",
            &[],
        )
        .await?;
    let databases: Vec<Value> = rows
        .iter()
        .map(|r| {
            let name: String = r.get(0).unwrap_or_default();
            json!({"name": name})
        })
        .collect();
    Ok(Value::Array(databases))
}

pub async fn list_schemas(params: &Value) -> Result<Value, AgentError> {
    let _database = string_param(params, "database");

    let mut guard = connection::conn_mutex().lock().await;
    let client = guard.as_mut().ok_or_else(AgentError::not_connected)?;

    let rows = client
        .query(
            "SELECT schema_name FROM information_schema.schemata WHERE schema_name NOT IN ('pg_catalog','information_schema','pg_toast') ORDER BY schema_name",
            &[],
        )
        .await?;
    let schemas: Vec<Value> = rows
        .iter()
        .map(|r| {
            let name: String = r.get(0).unwrap_or_default();
            Value::String(name)
        })
        .collect();
    Ok(Value::Array(schemas))
}

pub async fn list_tables(params: &Value) -> Result<Value, AgentError> {
    let schema = required_string(params, "schema")?;

    let mut guard = connection::conn_mutex().lock().await;
    let client = guard.as_mut().ok_or_else(AgentError::not_connected)?;

    let schema_val = rust_gaussdb::quote_string(&schema);
    let sql = format!(
        "SELECT table_name, table_type FROM information_schema.tables WHERE table_schema = {} ORDER BY table_name",
        schema_val
    );
    let rows = client.query(&sql, &[]).await?;
    let tables: Vec<Value> = rows
        .iter()
        .map(|r| {
            let name: String = r.get(0).unwrap_or_default();
            let table_type: String = r.get(1).unwrap_or_default();
            let normalized_type = if table_type == "BASE TABLE" {
                "TABLE"
            } else {
                &table_type
            };
            json!({"name": name, "table_type": normalized_type, "comment": null})
        })
        .collect();
    Ok(Value::Array(tables))
}

pub async fn list_objects(params: &Value) -> Result<Value, AgentError> {
    let schema = required_string(params, "schema")?;

    let mut guard = connection::conn_mutex().lock().await;
    let client = guard.as_mut().ok_or_else(AgentError::not_connected)?;

    let schema_val = rust_gaussdb::quote_string(&schema);

    // Tables
    let table_sql = format!(
        "SELECT table_name, table_type FROM information_schema.tables WHERE table_schema = {} ORDER BY table_name",
        schema_val
    );
    let table_rows = client.query(&table_sql, &[]).await?;
    let mut objects: Vec<Value> = table_rows
        .iter()
        .map(|r| {
            let name: String = r.get(0).unwrap_or_default();
            let table_type: String = r.get(1).unwrap_or_default();
            let normalized_type = if table_type == "BASE TABLE" {
                "TABLE"
            } else {
                &table_type
            };
            json!({"name": name, "object_type": normalized_type, "schema": schema, "comment": null})
        })
        .collect();

    // Routines
    let routine_sql = format!(
        "SELECT routine_name, routine_type FROM information_schema.routines WHERE routine_schema = {} ORDER BY routine_name",
        schema_val
    );
    if let Ok(routine_rows) = client.query(&routine_sql, &[]).await {
        for r in &routine_rows {
            let name: String = r.get(0).unwrap_or_default();
            let routine_type: String = r.get(1).unwrap_or_default();
            objects.push(json!({
                "name": name,
                "object_type": routine_type,
                "schema": schema,
                "comment": null
            }));
        }
    }

    Ok(Value::Array(objects))
}
