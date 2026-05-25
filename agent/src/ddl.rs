/// Generate a CREATE TABLE DDL statement from column, index, and foreign key metadata.
use crate::methods::schema_info::{ColumnInfo, ForeignKeyInfo, IndexInfo};

pub fn build_table_ddl(
    schema: &str,
    table: &str,
    columns: &[ColumnInfo],
    indexes: &[IndexInfo],
    foreign_keys: &[ForeignKeyInfo],
) -> String {
    let full_name = format!("\"{}\".\"{}\"", schema.replace('"', "\"\""), table.replace('"', "\"\""));

    let mut lines = Vec::new();
    lines.push(format!("CREATE TABLE {} (", full_name));

    // Column definitions
    for (i, col) in columns.iter().enumerate() {
        let comma = if i + 1 < columns.len()
            || indexes.iter().any(|idx| idx.is_primary)
            || !foreign_keys.is_empty()
        {
            ","
        } else {
            ""
        };
        let null_str = if col.is_nullable { "" } else { " NOT NULL" };
        let default_str = if let Some(ref def) = col.column_default {
            format!(" DEFAULT {}", def)
        } else {
            String::new()
        };
        lines.push(format!(
            "    \"{}\" {}{}{}{}",
            col.name.replace('"', "\"\""),
            col_type_sql(&col.data_type, col.character_maximum_length, col.numeric_precision, col.numeric_scale),
            null_str,
            default_str,
            comma,
        ));
    }

    // Primary key constraint
    let pk_cols: Vec<&str> = columns
        .iter()
        .filter(|c| c.is_primary_key)
        .map(|c| c.name.as_str())
        .collect();
    if !pk_cols.is_empty() {
        let pk_list = pk_cols
            .iter()
            .map(|n| format!("\"{}\"", n.replace('"', "\"\"")))
            .collect::<Vec<_>>()
            .join(", ");
        let comma = if !foreign_keys.is_empty() { "," } else { "" };
        lines.push(format!("    PRIMARY KEY ({}){}", pk_list, comma));
    }

    // Foreign key constraints
    for (i, fk) in foreign_keys.iter().enumerate() {
        let comma = if i + 1 < foreign_keys.len() { "," } else { "" };
        let constraint = if !fk.name.is_empty() {
            format!("CONSTRAINT \"{}\" ", fk.name.replace('"', "\"\""))
        } else {
            String::new()
        };
        lines.push(format!(
            "    {}FOREIGN KEY (\"{}\") REFERENCES \"{}\"( \"{}\"){}",
            constraint,
            fk.column.replace('"', "\"\""),
            fk.ref_table.replace('"', "\"\""),
            fk.ref_column.replace('"', "\"\""),
            comma,
        ));
    }

    lines.push(");".to_string());

    // Indexes (non-primary)
    for idx in indexes {
        if idx.is_primary {
            continue;
        }
        let unique = if idx.is_unique { "UNIQUE " } else { "" };
        let col_list = idx
            .columns
            .iter()
            .map(|n| format!("\"{}\"", n.replace('"', "\"\"")))
            .collect::<Vec<_>>()
            .join(", ");
        let index_name = format!(
            "\"{}\".\"{}\"",
            schema.replace('"', "\"\""),
            idx.name.replace('"', "\"\"")
        );
        let using_clause = if !idx.index_type.is_empty() {
            format!(" USING {}", idx.index_type)
        } else {
            String::new()
        };
        lines.push(format!(
            "CREATE {}{}INDEX {} ON {} ({});",
            unique, using_clause, index_name, full_name, col_list
        ));
    }

    lines.join("\n")
}

fn col_type_sql(
    data_type: &str,
    char_max_len: Option<i32>,
    num_precision: Option<i32>,
    num_scale: Option<i32>,
) -> String {
    let dt_lower = data_type.to_lowercase();
    match dt_lower.as_str() {
        "character varying" | "varchar" => {
            if let Some(len) = char_max_len {
                format!("VARCHAR({})", len)
            } else {
                "VARCHAR".to_string()
            }
        }
        "character" | "char" => {
            if let Some(len) = char_max_len {
                format!("CHAR({})", len)
            } else {
                "CHAR".to_string()
            }
        }
        "numeric" | "decimal" => match (num_precision, num_scale) {
            (Some(p), Some(s)) if s > 0 => format!("NUMERIC({}, {})", p, s),
            (Some(p), _) => format!("NUMERIC({})", p),
            _ => "NUMERIC".to_string(),
        },
        "timestamp without time zone" => "TIMESTAMP".to_string(),
        "timestamp with time zone" => "TIMESTAMPTZ".to_string(),
        "time without time zone" => "TIME".to_string(),
        "time with time zone" => "TIMETZ".to_string(),
        _ => data_type.to_uppercase(),
    }
}
