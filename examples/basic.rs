use rust_gaussdb::Client;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut client = Client::connect(
        "host=127.0.0.1 port=15432 user=gaussdb password=Gauss@123 dbname=postgres",
    )
    .await?;

    println!("Connected!");

    let rows = client.query("SELECT version()", &[]).await?;
    let version: String = rows[0].get(0)?;
    println!("Version: {version}");

    let rows = client.query("SELECT * FROM users", &[]).await?;
    println!("\nUsers ({} rows):", rows.len());
    for row in &rows {
        let id: i32 = row.get(0)?;
        let name: String = row.get(1)?;
        let email: String = row.get(2)?;
        println!("  {id}: {name} <{email}>");
    }

    let affected = client.execute("UPDATE users SET name = 'Test' WHERE id = 999").await?;
    println!("\nAffected rows: {affected}");

    client.close().await?;
    println!("Connection closed.");

    Ok(())
}
