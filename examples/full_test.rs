use rust_gaussdb::Client;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut client = Client::connect(
        "host=127.0.0.1 port=15432 user=gaussdb password=Gauss@123 dbname=postgres",
    )
    .await?;

    println!("=== rust-gaussdb integration tests ===\n");

    // 1. Version query
    print!("1. SELECT version() ... ");
    let rows = client.query("SELECT version()", &[]).await?;
    let version: String = rows[0].get(0)?;
    assert!(version.contains("openGauss") || version.contains("GaussDB"));
    println!("OK ({} chars)", version.len());

    // 2. Drop test table if exists
    print!("2. DROP TABLE IF EXISTS ... ");
    client.execute("DROP TABLE IF EXISTS rust_test").await?;
    println!("OK");

    // 3. CREATE TABLE
    print!("3. CREATE TABLE ... ");
    client
        .execute(
            "CREATE TABLE rust_test (
                id SERIAL PRIMARY KEY,
                name VARCHAR(100) NOT NULL,
                score DECIMAL(5,2),
                active BOOLEAN DEFAULT true,
                created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
            )",
        )
        .await?;
    println!("OK");

    // 4. INSERT single row
    print!("4. INSERT single row ... ");
    let affected = client
        .execute("INSERT INTO rust_test (name, score, active) VALUES ('Alice', 95.5, true)")
        .await?;
    assert_eq!(affected, 1);
    println!("OK (affected={affected})");

    // 5. INSERT multiple rows
    print!("5. INSERT multiple rows ... ");
    let affected = client
        .execute(
            "INSERT INTO rust_test (name, score, active) VALUES
                ('Bob', 87.3, true),
                ('Charlie', 72.0, false),
                ('Diana', 91.8, true),
                ('Eve', NULL, false)",
        )
        .await?;
    assert_eq!(affected, 4);
    println!("OK (affected={affected})");

    // 6. SELECT all rows
    print!("6. SELECT * ... ");
    let rows = client
        .query(
            "SELECT id, name, score, active FROM rust_test ORDER BY id",
            &[],
        )
        .await?;
    assert_eq!(rows.len(), 5);
    println!("OK ({} rows)", rows.len());

    // 7. Type conversions
    print!("7. Type conversions ... ");
    let id: i32 = rows[0].get(0)?;
    let name: String = rows[0].get(1)?;
    let score: String = rows[0].get(2)?;
    let active: String = rows[0].get(3)?;
    assert_eq!(id, 1);
    assert_eq!(name, "Alice");
    assert!(score.starts_with("95.5"));
    assert_eq!(active, "t");
    println!("OK (id={id}, name={name}, score={score}, active={active})");

    // 8. NULL handling
    print!("8. NULL handling ... ");
    let eve_score: Option<String> = rows[4].get(2)?;
    assert_eq!(eve_score, None);
    println!("OK (Eve.score=NULL)");

    // 9. get_by_name
    print!("9. get_by_name ... ");
    let name: String = rows[1].get_by_name("name")?;
    assert_eq!(name, "Bob");
    println!("OK (name={name})");

    // 10. UPDATE
    print!("10. UPDATE ... ");
    let affected = client
        .execute("UPDATE rust_test SET score = 99.9 WHERE name = 'Alice'")
        .await?;
    assert_eq!(affected, 1);
    let rows = client
        .query("SELECT score FROM rust_test WHERE name = 'Alice'", &[])
        .await?;
    let score: String = rows[0].get(0)?;
    assert!(score.starts_with("99.9"));
    println!("OK (score={score})");

    // 11. DELETE
    print!("11. DELETE ... ");
    let affected = client
        .execute("DELETE FROM rust_test WHERE active = false")
        .await?;
    assert_eq!(affected, 2);
    println!("OK (affected={affected})");

    // 12. COUNT after delete
    print!("12. COUNT ... ");
    let rows = client.query("SELECT COUNT(*) FROM rust_test", &[]).await?;
    let count: i64 = rows[0].get(0)?;
    assert_eq!(count, 3);
    println!("OK (count={count})");

    // 13. WHERE with multiple conditions
    print!("13. WHERE conditions ... ");
    let rows = client
        .query(
            "SELECT name FROM rust_test WHERE active = true AND score > 90 ORDER BY name",
            &[],
        )
        .await?;
    assert_eq!(rows.len(), 2);
    let n1: String = rows[0].get(0)?;
    let n2: String = rows[1].get(0)?;
    assert_eq!(n1, "Alice");
    assert_eq!(n2, "Diana");
    println!("OK ({n1}, {n2})");

    // 14. Aggregate functions
    print!("14. Aggregates ... ");
    let rows = client
        .query(
            "SELECT MIN(score), MAX(score), AVG(score) FROM rust_test WHERE score IS NOT NULL",
            &[],
        )
        .await?;
    assert_eq!(rows.len(), 1);
    let min: String = rows[0].get(0)?;
    let max: String = rows[0].get(1)?;
    println!("OK (min={min}, max={max})");

    // 15. Subquery
    print!("15. Subquery ... ");
    let rows = client
        .query(
            "SELECT name FROM rust_test WHERE score = (SELECT MAX(score) FROM rust_test)",
            &[],
        )
        .await?;
    assert_eq!(rows.len(), 1);
    let name: String = rows[0].get(0)?;
    assert_eq!(name, "Alice");
    println!("OK (top_scorer={name})");

    // 16. CTE (WITH)
    print!("16. CTE query ... ");
    let rows = client
        .query(
            "WITH ranked AS (SELECT name, score, ROW_NUMBER() OVER (ORDER BY score DESC) AS rank FROM rust_test WHERE score IS NOT NULL) SELECT name, rank FROM ranked WHERE rank <= 2",
            &[],
        )
        .await?;
    assert_eq!(rows.len(), 2);
    println!("OK ({} rows)", rows.len());

    // 17. UNION
    print!("17. UNION ... ");
    let rows = client
        .query(
            "SELECT name FROM rust_test WHERE score > 95 UNION ALL SELECT name FROM rust_test WHERE name = 'Bob'",
            &[],
        )
        .await?;
    assert!(rows.len() >= 2);
    println!("OK ({} rows)", rows.len());

    // 18. Chinese characters
    print!("18. Unicode/Chinese ... ");
    client
        .execute("INSERT INTO rust_test (name, score) VALUES ('张三', 88.0)")
        .await?;
    let rows = client
        .query("SELECT name FROM rust_test WHERE name = '张三'", &[])
        .await?;
    assert_eq!(rows.len(), 1);
    let name: String = rows[0].get(0)?;
    assert_eq!(name, "张三");
    println!("OK (name={name})");

    // 19. Empty result set
    print!("19. Empty result ... ");
    let rows = client
        .query("SELECT * FROM rust_test WHERE 1 = 0", &[])
        .await?;
    assert_eq!(rows.len(), 0);
    println!("OK (0 rows)");

    // 20. Error handling - bad SQL
    print!("20. Error handling ... ");
    let result = client.execute("SELECT * FROM nonexistent_table_xyz").await;
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("nonexistent_table_xyz") || err.contains("does not exist"));
    println!("OK (caught: {})", &err[..err.len().min(60)]);

    // 21. Connection still works after error
    print!("21. Recovery after error ... ");
    let rows = client.query("SELECT 1 + 1 AS result", &[]).await?;
    let result: i32 = rows[0].get(0)?;
    assert_eq!(result, 2);
    println!("OK (1+1={result})");

    // 22. Large result set
    print!("22. Large result ... ");
    client
        .execute("DROP TABLE IF EXISTS rust_test_large")
        .await?;
    client
        .execute("CREATE TABLE rust_test_large AS SELECT generate_series(1, 1000) AS id")
        .await?;
    let rows = client
        .query("SELECT COUNT(*) FROM rust_test_large", &[])
        .await?;
    let count: i64 = rows[0].get(0)?;
    assert_eq!(count, 1000);
    client.execute("DROP TABLE rust_test_large").await?;
    println!("OK ({count} rows generated)");

    // 23. server_parameter
    print!("23. Server parameters ... ");
    let encoding = client.server_parameter("client_encoding");
    assert_eq!(encoding, Some("UTF8"));
    let version = client.server_parameter("server_version");
    assert!(version.is_some());
    println!(
        "OK (encoding={}, version={})",
        encoding.unwrap(),
        version.unwrap()
    );

    // Cleanup
    print!("24. Cleanup ... ");
    client.execute("DROP TABLE rust_test").await?;
    println!("OK");

    client.close().await?;

    println!("\n=== All 24 tests passed! ===");
    Ok(())
}
