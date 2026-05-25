use serde_json::Value;
use rust_gaussdb::Row;

/// Convert a database Row to a JSON array of typed values.
/// Uses column type OID to determine the output type.
pub fn row_to_json(row: &Row) -> Value {
    let values: Vec<Value> = (0..row.len())
        .map(|i| cell_to_json(row, i))
        .collect();
    Value::Array(values)
}

/// Convert a single cell to a JSON value, using type OID for type-aware serialization.
pub fn cell_to_json(row: &Row, idx: usize) -> Value {
    let columns = row.columns();
    let raw = row.values().get(idx);

    let type_oid = columns.get(idx).map(|c| c.type_oid).unwrap_or(0);

    match raw {
        None | Some(None) => Value::Null,
        Some(Some(bytes)) => {
            let s = String::from_utf8_lossy(bytes);
            match type_oid {
                // Integer types: int8, int2, int4, oid
                20 | 21 | 23 | 26 => s
                    .parse::<i64>()
                    .map(|v| Value::Number(serde_json::Number::from(v)))
                    .unwrap_or_else(|_| Value::String(s.to_string())),
                // Float types: float4, float8
                700 | 701 => s
                    .parse::<f64>()
                    .ok()
                    .and_then(|v| serde_json::Number::from_f64(v))
                    .map(Value::Number)
                    .unwrap_or_else(|| Value::String(s.to_string())),
                // Numeric
                1700 => s
                    .parse::<f64>()
                    .ok()
                    .and_then(|v| serde_json::Number::from_f64(v))
                    .map(Value::Number)
                    .unwrap_or_else(|| Value::String(s.to_string())),
                // Boolean
                16 => Value::Bool(s == "t"),
                // Everything else: string
                _ => Value::String(s.to_string()),
            }
        }
    }
}
