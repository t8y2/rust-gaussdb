use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::connection;
use crate::protocol::{required_string, AgentError};

#[derive(Debug, Serialize, Deserialize)]
pub struct ColumnInfo {
    pub name: String,
    pub data_type: String,
    pub is_nullable: bool,
    pub column_default: Option<String>,
    pub is_primary_key: bool,
    #[serde(default)]
    pub extra: Option<String>,
    #[serde(default)]
    pub comment: Option<String>,
    pub numeric_precision: Option<i32>,
    pub numeric_scale: Option<i32>,
    pub character_maximum_length: Option<i32>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct IndexInfo {
    pub name: String,
    pub columns: Vec<String>,
    pub is_unique: bool,
    pub is_primary: bool,
    #[serde(default)]
    pub filter: Option<String>,
    #[serde(default)]
    pub index_type: String,
    #[serde(default)]
    pub included_columns: Vec<String>,
    #[serde(default)]
    pub comment: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ForeignKeyInfo {
    pub name: String,
    pub column: String,
    pub ref_table: String,
    pub ref_column: String,
}

pub async fn get_columns(params: &Value) -> Result<Value, AgentError> {
    let schema = required_string(params, "schema")?;
    let table = required_string(params, "table")?;

    let mut guard = connection::conn_mutex().lock().await;
    let client = guard.as_mut().ok_or_else(AgentError::not_connected)?;
    let schema_val = rust_gaussdb::quote_string(&schema);
    let table_val = rust_gaussdb::quote_string(&table);

    // Single query: LEFT JOIN primary key info onto columns
    let sql = format!(
        "SELECT \
            c.column_name, c.data_type, c.is_nullable, c.column_default, \
            c.numeric_precision, c.numeric_scale, c.character_maximum_length, \
            (pk.column_name IS NOT NULL) AS is_primary_key \
         FROM information_schema.columns c \
         LEFT JOIN ( \
             SELECT kcu.column_name \
             FROM information_schema.table_constraints tc \
             JOIN information_schema.key_column_usage kcu \
               ON tc.constraint_name = kcu.constraint_name \
              AND tc.table_schema = kcu.table_schema \
             WHERE tc.constraint_type = 'PRIMARY KEY' \
               AND tc.table_schema = {} AND tc.table_name = {} \
         ) pk ON c.column_name = pk.column_name \
         WHERE c.table_schema = {} AND c.table_name = {} \
         ORDER BY c.ordinal_position",
        schema_val, table_val, schema_val, table_val
    );
    let rows = client.query(&sql, &[]).await?;

    let columns: Vec<Value> = rows
        .iter()
        .map(|r| {
            let name: String = r.get(0).unwrap_or_default();
            let data_type: String = r.get::<String>(1).unwrap_or_default();
            let is_nullable_str: String = r.get::<String>(2).unwrap_or_default();
            let is_nullable = is_nullable_str.to_uppercase() == "YES";
            let column_default: Option<String> = r.try_get::<Option<String>>(3).flatten();
            let numeric_precision: Option<i32> = r.try_get::<Option<i32>>(4).flatten();
            let numeric_scale: Option<i32> = r.try_get::<Option<i32>>(5).flatten();
            let character_maximum_length: Option<i32> = r.try_get::<Option<i32>>(6).flatten();
            let is_primary_key: bool = r.try_get::<bool>(7).unwrap_or(false);

            json!({
                "name": name,
                "data_type": data_type,
                "is_nullable": is_nullable,
                "column_default": column_default,
                "is_primary_key": is_primary_key,
                "extra": null,
                "comment": null,
                "numeric_precision": numeric_precision,
                "numeric_scale": numeric_scale,
                "character_maximum_length": character_maximum_length
            })
        })
        .collect();

    Ok(Value::Array(columns))
}

pub async fn list_indexes(params: &Value) -> Result<Value, AgentError> {
    let schema = required_string(params, "schema")?;
    let table = required_string(params, "table")?;

    let mut guard = connection::conn_mutex().lock().await;
    let client = guard.as_mut().ok_or_else(AgentError::not_connected)?;
    let schema_val = rust_gaussdb::quote_string(&schema);
    let table_val = rust_gaussdb::quote_string(&table);

    let sql = format!(
        "SELECT \
            ic.relname AS index_name, \
            i.indisunique AS is_unique, \
            i.indisprimary AS is_primary, \
            am.amname AS index_type, \
            array_agg(a.attname ORDER BY a.attnum) AS columns \
         FROM pg_index i \
         JOIN pg_class tc ON tc.oid = i.indrelid \
         JOIN pg_namespace tn ON tn.oid = tc.relnamespace \
         JOIN pg_class ic ON ic.oid = i.indexrelid \
         JOIN pg_am am ON am.oid = ic.relam \
         JOIN pg_namespace ixns ON ixns.oid = ic.relnamespace \
         JOIN pg_attribute a ON a.attrelid = i.indrelid AND a.attnum = ANY(i.indkey) \
         WHERE tn.nspname = {} AND tc.relname = {} AND ixns.nspname = {} \
         GROUP BY ic.relname, i.indisunique, i.indisprimary, am.amname \
         ORDER BY ic.relname",
        schema_val, table_val, schema_val
    );

    let rows = client.query(&sql, &[]).await?;
    let indexes: Vec<Value> = rows
        .iter()
        .map(|r| {
            let name: String = r.get(0).unwrap_or_default();
            let is_unique: bool = r.try_get::<bool>(1).unwrap_or(false);
            let is_primary: bool = r.try_get::<bool>(2).unwrap_or(false);
            let index_type: String = r.get::<String>(3).unwrap_or_default();
            let columns_str: String = r.get::<String>(4).unwrap_or_default();
            let columns: Vec<String> = columns_str
                .trim_start_matches('{')
                .trim_end_matches('}')
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();

            json!({
                "name": name,
                "columns": columns,
                "is_unique": is_unique,
                "is_primary": is_primary,
                "filter": null,
                "index_type": index_type,
                "included_columns": [],
                "comment": null
            })
        })
        .collect();

    Ok(Value::Array(indexes))
}

pub async fn list_foreign_keys(params: &Value) -> Result<Value, AgentError> {
    let schema = required_string(params, "schema")?;
    let table = required_string(params, "table")?;

    let mut guard = connection::conn_mutex().lock().await;
    let client = guard.as_mut().ok_or_else(AgentError::not_connected)?;
    let schema_val = rust_gaussdb::quote_string(&schema);
    let table_val = rust_gaussdb::quote_string(&table);

    let sql = format!(
        "SELECT tc.constraint_name, kcu.column_name, \
                ccu.table_schema AS ref_schema, ccu.table_name AS ref_table, ccu.column_name AS ref_column \
         FROM information_schema.table_constraints tc \
         JOIN information_schema.key_column_usage kcu \
           ON tc.constraint_name = kcu.constraint_name AND tc.table_schema = kcu.table_schema \
         JOIN information_schema.constraint_column_usage ccu \
           ON tc.constraint_name = ccu.constraint_name AND tc.table_schema = ccu.table_schema \
         WHERE tc.constraint_type = 'FOREIGN KEY' \
           AND tc.table_schema = {} AND tc.table_name = {}",
        schema_val, table_val
    );

    let rows = client.query(&sql, &[]).await?;
    let fks: Vec<Value> = rows
        .iter()
        .map(|r| {
            json!({
                "name": r.get::<String>(0).unwrap_or_default(),
                "column": r.get::<String>(1).unwrap_or_default(),
                "ref_table": r.get::<String>(3).unwrap_or_default(),
                "ref_column": r.get::<String>(4).unwrap_or_default(),
            })
        })
        .collect();

    Ok(Value::Array(fks))
}

pub async fn list_triggers(params: &Value) -> Result<Value, AgentError> {
    let schema = required_string(params, "schema")?;
    let table = required_string(params, "table")?;

    let mut guard = connection::conn_mutex().lock().await;
    let client = guard.as_mut().ok_or_else(AgentError::not_connected)?;
    let schema_val = rust_gaussdb::quote_string(&schema);
    let table_val = rust_gaussdb::quote_string(&table);

    let sql = format!(
        "SELECT trigger_name, event_manipulation, action_timing \
         FROM information_schema.triggers \
         WHERE event_object_schema = {} AND event_object_table = {} \
         ORDER BY trigger_name",
        schema_val, table_val
    );

    let rows = client.query(&sql, &[]).await?;
    let triggers: Vec<Value> = rows
        .iter()
        .map(|r| {
            json!({
                "name": r.get::<String>(0).unwrap_or_default(),
                "event": r.get::<String>(1).unwrap_or_default(),
                "timing": r.get::<String>(2).unwrap_or_default(),
            })
        })
        .collect();

    Ok(Value::Array(triggers))
}
