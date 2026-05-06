use futures_util::{SinkExt, StreamExt};

use crate::codec::*;
use crate::config::Config;
use crate::connection::Connection;
use crate::error::Error;
use crate::row::Row;

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

    pub async fn query(&mut self, sql: &str, _params: &[&str]) -> Result<Vec<Row>, Error> {
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
                BackendMessage::ErrorResponse { fields } => {
                    let msg = fields
                        .iter()
                        .find(|(t, _)| *t == b'M')
                        .map(|(_, v)| v.clone())
                        .unwrap_or_else(|| "unknown error".to_string());
                    drain_until_ready(&mut self.conn).await;
                    return Err(Error::Database(msg));
                }
                BackendMessage::NoticeResponse => {}
                _ => {}
            }
        }

        Ok(rows)
    }

    pub async fn execute(&mut self, sql: &str) -> Result<u64, Error> {
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
                    if let Some(count) = tag.rsplit(' ').next().and_then(|s| s.parse::<u64>().ok()) {
                        affected = count;
                    }
                }
                BackendMessage::EmptyQueryResponse => {}
                BackendMessage::ReadyForQuery => break,
                BackendMessage::ErrorResponse { fields } => {
                    let msg = fields
                        .iter()
                        .find(|(t, _)| *t == b'M')
                        .map(|(_, v)| v.clone())
                        .unwrap_or_else(|| "unknown error".to_string());
                    drain_until_ready(&mut self.conn).await;
                    return Err(Error::Database(msg));
                }
                BackendMessage::RowDescription { .. } | BackendMessage::DataRow { .. } => {}
                BackendMessage::NoticeResponse => {}
                _ => {}
            }
        }

        Ok(affected)
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
}

async fn drain_until_ready(conn: &mut Connection) {
    loop {
        match conn.framed.next().await {
            Some(Ok(BackendMessage::ReadyForQuery)) | None => break,
            _ => continue,
        }
    }
}
