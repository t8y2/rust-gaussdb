//! # rust-gaussdb
//!
//! A lightweight native GaussDB/openGauss client for Rust.
//!
//! Supports GaussDB's custom SHA256 and MD5_SHA256 SASL authentication
//! mechanisms that standard PostgreSQL drivers don't handle.
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use rust_gaussdb::Client;
//!
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let mut client = Client::connect("host=127.0.0.1 port=5432 user=gaussdb password=Gauss@123 dbname=postgres").await?;
//!
//! let rows = client.query("SELECT version()", &[]).await?;
//! println!("{}", rows[0].get::<String>(0)?);
//! # Ok(())
//! # }
//! ```

mod auth;
mod client;
mod codec;
mod config;
mod connection;
mod error;
mod row;
mod transaction;

pub use client::{double_quote_identifier, quote_string, Client};
pub use config::Config;
pub use error::{DbError, Error};
pub use row::{FromSql, Row, ToSql};
