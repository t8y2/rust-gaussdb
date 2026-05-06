# rust-gaussdb

A lightweight native Rust client for GaussDB and openGauss databases.

Solves the authentication problem: GaussDB/openGauss use custom SASL mechanisms (SHA256, MD5_SHA256) that standard PostgreSQL drivers like `tokio-postgres` and `sqlx` don't support. This crate handles them natively — no ODBC drivers needed.

## Quick Start

```rust
use gaussdb_connector::Client;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut client = Client::connect(
        "host=127.0.0.1 port=5432 user=gaussdb password=Gauss@123 dbname=postgres",
    ).await?;

    let rows = client.query("SELECT version()", &[]).await?;
    println!("{}", rows[0].get::<String>(0)?);

    let rows = client.query("SELECT * FROM users", &[]).await?;
    for row in &rows {
        let id: i32 = row.get(0)?;
        let name: String = row.get(1)?;
        println!("{id}: {name}");
    }

    let affected = client.execute("DELETE FROM temp WHERE id = 1").await?;
    println!("Deleted {affected} rows");

    client.close().await?;
    Ok(())
}
```

## Features

- Native GaussDB SHA256 and MD5_SHA256 SASL authentication
- Standard PostgreSQL SCRAM-SHA-256 also supported
- MD5 and cleartext password fallback
- Simple query protocol (text mode)
- Zero ODBC dependencies — pure Rust over TCP
- Works on all platforms (Linux, macOS, Windows)

## Supported Databases

| Database | Authentication |
|----------|---------------|
| openGauss | SHA256, MD5, SCRAM-SHA-256 |
| GaussDB (Huawei Cloud) | SHA256, MD5_SHA256 |
| PostgreSQL | SCRAM-SHA-256, MD5 |

## License

MIT OR Apache-2.0
