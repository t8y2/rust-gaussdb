use serde_json::{json, Value};

use crate::connection;
use crate::methods::schema_info::{ColumnInfo, ForeignKeyInfo, IndexInfo};
use crate::protocol::{required_string, AgentError};

pub async fn get_object_source(params: &Value) -> Result<Value, AgentError> {
    let schema = required_string(params, "schema")?;
    let name = required_string(params, "name")?;
    let object_type = required_string(params, "object_type")?;

    let mut guard = connection::conn_mutex().lock().await;
    let client = guard.as_mut().ok_or_else(AgentError::not_connected)?;

    let source = match object_type.to_uppercase().as_str() {
        "VIEW" => {
            let schema_val = rust_gaussdb::quote_string(&schema);
            let name_val = rust_gaussdb::quote_string(&name);
            let sql = format!(
                "SELECT pg_get_viewdef(c.oid, true) \
                 FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace \
                 WHERE c.relkind = 'v' AND n.nspname = {} AND c.relname = {}",
                schema_val, name_val
            );
            let rows = client.query(&sql, &[]).await?;
            rows.first()
                .and_then(|r| r.try_get::<String>(0))
                .unwrap_or_default()
        }
        "FUNCTION" => {
            let schema_val = rust_gaussdb::quote_string(&schema);
            let name_val = rust_gaussdb::quote_string(&name);
            let sql = format!(
                "SELECT pg_get_functiondef(p.oid) \
                 FROM pg_proc p JOIN pg_namespace n ON n.oid = p.pronamespace \
                 WHERE n.nspname = {} AND p.proname = {} AND p.prokind = 'f' \
                 ORDER BY p.oid LIMIT 1",
                schema_val, name_val
            );
            let rows = client.query(&sql, &[]).await?;
            rows.first()
                .and_then(|r| r.try_get::<String>(0))
                .unwrap_or_default()
        }
        "PROCEDURE" => {
            let schema_val = rust_gaussdb::quote_string(&schema);
            let name_val = rust_gaussdb::quote_string(&name);
            let sql = format!(
                "SELECT pg_get_functiondef(p.oid) \
                 FROM pg_proc p JOIN pg_namespace n ON n.oid = p.pronamespace \
                 WHERE n.nspname = {} AND p.proname = {} AND p.prokind = 'p' \
                 ORDER BY p.oid LIMIT 1",
                schema_val, name_val
            );
            let rows = client.query(&sql, &[]).await?;
            rows.first()
                .and_then(|r| r.try_get::<String>(0))
                .unwrap_or_default()
        }
        _ => {
            return Err(AgentError::protocol(format!(
                "Unsupported object type: {}",
                object_type
            )))
        }
    };

    Ok(json!({
        "name": name,
        "object_type": object_type,
        "schema": schema,
        "source": source
    }))
}

pub async fn get_table_ddl(params: &Value) -> Result<Value, AgentError> {
    let schema = required_string(params, "schema")?;
    let table = required_string(params, "table")?;

    let mut guard = connection::conn_mutex().lock().await;
    let client = guard.as_mut().ok_or_else(AgentError::not_connected)?;

    let columns = fetch_columns(client, &schema, &table).await?;
    let indexes = fetch_indexes(client, &schema, &table).await?;
    let foreign_keys = fetch_foreign_keys(client, &schema, &table).await?;

    let ddl = crate::ddl::build_table_ddl(&schema, &table, &columns, &indexes, &foreign_keys);
    Ok(Value::String(ddl))
}

async fn fetch_columns(
    client: &mut rust_gaussdb::Client,
    schema: &str,
    table: &str,
) -> Result<Vec<ColumnInfo>, AgentError> {
    let schema_val = rust_gaussdb::quote_string(schema);
    let table_val = rust_gaussdb::quote_string(table);

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

    Ok(rows
        .iter()
        .map(|r| {
            let name: String = r.get::<String>(0).unwrap_or_default();
            ColumnInfo {
                is_primary_key: r.try_get::<bool>(7).unwrap_or(false),
                name,
                data_type: r.get::<String>(1).unwrap_or_default(),
                is_nullable: {
                    let s: String = r.get::<String>(2).unwrap_or_default();
                    s.to_uppercase() == "YES"
                },
                column_default: r.try_get::<Option<String>>(3).flatten(),
                numeric_precision: r.try_get::<Option<i32>>(4).flatten(),
                numeric_scale: r.try_get::<Option<i32>>(5).flatten(),
                character_maximum_length: r.try_get::<Option<i32>>(6).flatten(),
                extra: None,
                comment: None,
            }
        })
        .collect())
}

async fn fetch_indexes(
    client: &mut rust_gaussdb::Client,
    schema: &str,
    table: &str,
) -> Result<Vec<IndexInfo>, AgentError> {
    let schema_val = rust_gaussdb::quote_string(schema);
    let table_val = rust_gaussdb::quote_string(table);

    let sql = format!(
        "SELECT ic.relname, i.indisunique, i.indisprimary, am.amname, \
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
    Ok(rows
        .iter()
        .map(|r| {
            let columns_str: String = r.get::<String>(4).unwrap_or_default();
            IndexInfo {
                name: r.get::<String>(0).unwrap_or_default(),
                is_unique: r.try_get::<bool>(1).unwrap_or(false),
                is_primary: r.try_get::<bool>(2).unwrap_or(false),
                index_type: r.get::<String>(3).unwrap_or_default(),
                columns: columns_str
                    .trim_start_matches('{')
                    .trim_end_matches('}')
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect(),
                filter: None,
                included_columns: Vec::new(),
                comment: None,
            }
        })
        .collect())
}

async fn fetch_foreign_keys(
    client: &mut rust_gaussdb::Client,
    schema: &str,
    table: &str,
) -> Result<Vec<ForeignKeyInfo>, AgentError> {
    let schema_val = rust_gaussdb::quote_string(schema);
    let table_val = rust_gaussdb::quote_string(table);

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
    Ok(rows
        .iter()
        .map(|r| ForeignKeyInfo {
            name: r.get::<String>(0).unwrap_or_default(),
            column: r.get::<String>(1).unwrap_or_default(),
            ref_table: r.get::<String>(3).unwrap_or_default(),
            ref_column: r.get::<String>(4).unwrap_or_default(),
        })
        .collect())
}
