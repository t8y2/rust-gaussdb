use futures_util::{SinkExt, StreamExt};

use crate::codec::*;
use crate::config::Config;
use crate::connection::Connection;
use crate::error::Error;
use crate::row::{Row, ToSql};

pub struct Client {
    conn: Connection,
}

impl Client {
    pub async fn connect(dsn: &str) -> Result<Self, Error> {
        let config = Config::parse(dsn)?;
        Self::connect_with_config(&config).await
    }

    pub async fn connect_with_config(config: &Config) -> Result<Self, Error> {
        let conn = Connection::connect(config).await?;
        Ok(Client { conn })
    }

    /// Execute a simple query without parameters.
    pub async fn query(&mut self, sql: &str, _params: &[&str]) -> Result<Vec<Row>, Error> {
        self.query_simple(sql).await
    }

    /// Execute a query with typed parameters.
    /// Placeholders `$1`, `$2`, etc. are substituted with escaped literal values.
    ///
    /// For agent-internal SQL where all parameters come from DBX, this is safe.
    /// For user-provided SQL with untrusted values, consider the extended query
    /// protocol instead.
    pub async fn query_params(&mut self, sql: &str, params: &[&dyn ToSql]) -> Result<Vec<Row>, Error> {
        let escaped = substitute_params(sql, params);
        self.query_simple(&escaped).await
    }

    /// Execute a statement without parameters.
    pub async fn execute(&mut self, sql: &str) -> Result<u64, Error> {
        self.execute_simple(sql).await
    }

    /// Execute a statement with typed parameters.
    pub async fn execute_params(&mut self, sql: &str, params: &[&dyn ToSql]) -> Result<u64, Error> {
        let escaped = substitute_params(sql, params);
        self.execute_simple(&escaped).await
    }

    /// Set the search path (schema) for subsequent queries.
    pub async fn set_schema(&mut self, schema: &str) -> Result<(), Error> {
        let quoted = double_quote_identifier(schema);
        let sql = format!("SET search_path TO {}", quoted);
        self.execute(&sql).await.map(|_| ())
    }

    pub async fn close(mut self) -> Result<(), Error> {
        let msg = build_terminate_message();
        self.conn.framed.send(FrontendMessage(msg)).await?;
        Ok(())
    }

    pub fn server_parameter(&self, name: &str) -> Option<&str> {
        self.conn
            .parameters
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    async fn query_simple(&mut self, sql: &str) -> Result<Vec<Row>, Error> {
        let msg = build_query_message(sql);
        self.conn.framed.send(FrontendMessage(msg)).await?;

        let mut columns = Vec::new();
        let mut rows = Vec::new();

        loop {
            let msg = self
                .conn
                .framed
                .next()
                .await
                .ok_or_else(|| Error::Protocol("connection closed".into()))??;

            match msg {
                BackendMessage::RowDescription { columns: cols } => {
                    columns = cols;
                }
                BackendMessage::DataRow { values } => {
                    rows.push(Row::new(columns.clone(), values));
                }
                BackendMessage::CommandComplete { .. } => {}
                BackendMessage::EmptyQueryResponse => {}
                BackendMessage::ReadyForQuery => break,
                BackendMessage::ErrorResponse { error } => {
                    drain_until_ready(&mut self.conn).await;
                    return Err(Error::Database(error));
                }
                BackendMessage::NoticeResponse => {}
                _ => {}
            }
        }

        Ok(rows)
    }

    async fn execute_simple(&mut self, sql: &str) -> Result<u64, Error> {
        let msg = build_query_message(sql);
        self.conn.framed.send(FrontendMessage(msg)).await?;

        let mut affected = 0u64;

        loop {
            let msg = self
                .conn
                .framed
                .next()
                .await
                .ok_or_else(|| Error::Protocol("connection closed".into()))??;

            match msg {
                BackendMessage::CommandComplete { tag } => {
                    if let Some(count) = tag.rsplit(' ').next().and_then(|s| s.parse::<u64>().ok())
                    {
                        affected = count;
                    }
                }
                BackendMessage::EmptyQueryResponse => {}
                BackendMessage::ReadyForQuery => break,
                BackendMessage::ErrorResponse { error } => {
                    drain_until_ready(&mut self.conn).await;
                    return Err(Error::Database(error));
                }
                BackendMessage::RowDescription { .. } | BackendMessage::DataRow { .. } => {}
                BackendMessage::NoticeResponse => {}
                _ => {}
            }
        }

        Ok(affected)
    }
}

async fn drain_until_ready(conn: &mut Connection) {
    loop {
        match conn.framed.next().await {
            Some(Ok(BackendMessage::ReadyForQuery)) | None => break,
            _ => continue,
        }
    }
}

/// Substitute `$1`, `$2`, ... placeholders with escaped literal values.
///
/// String types are single-quoted with internal `'` escaped as `''`.
/// Numeric and boolean types use their text representation.
/// NULL values are represented as the literal `NULL`.
fn substitute_params(sql: &str, params: &[&dyn ToSql]) -> String {
    let mut result = sql.to_string();
    for (i, param) in params.iter().enumerate().rev() {
        let placeholder = format!("${}", i + 1);
        let value = param.to_sql();
        let escaped = if value.is_empty() && param.oid() == 0 {
            // Option::None
            "NULL".to_string()
        } else {
            let s = String::from_utf8_lossy(&value);
            match param.oid() {
                25 | 1043 => {
                    // TEXT, VARCHAR: quote and escape
                    let escaped = s.replace('\'', "''");
                    format!("'{}'", escaped)
                }
                16 => {
                    // BOOL: use unquoted t/f
                    s.to_string()
                }
                17 => {
                    // BYTEA: hex format
                    format!("E'\\\\x{}'", hex::encode(&value))
                }
                _ => {
                    // Numeric types: use as-is
                    s.to_string()
                }
            }
        };
        result = result.replace(&placeholder, &escaped);
    }
    result
}

/// Double-quote an identifier (schema, table, column name) for safe SQL embedding.
pub fn double_quote_identifier(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

/// Single-quote a string literal for safe SQL embedding.
pub fn quote_string(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_substitute_params_string() {
        let sql = "SELECT * FROM t WHERE name = $1 AND age > $2";
        let params: &[&dyn ToSql] = &[&"hello", &42i64];
        let result = substitute_params(sql, params);
        assert_eq!(
            result,
            "SELECT * FROM t WHERE name = 'hello' AND age > 42"
        );
    }

    #[test]
    fn test_substitute_params_quote_escape() {
        let sql = "SELECT * FROM t WHERE name = $1";
        let params: &[&dyn ToSql] = &[&"it's"];
        let result = substitute_params(sql, params);
        assert_eq!(result, "SELECT * FROM t WHERE name = 'it''s'");
    }

    #[test]
    fn test_double_quote_identifier() {
        assert_eq!(double_quote_identifier("public"), "\"public\"");
        assert_eq!(double_quote_identifier("my\"table"), "\"my\"\"table\"");
    }
}
