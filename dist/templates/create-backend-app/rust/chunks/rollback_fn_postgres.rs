async fn rollback_postgres(args: &Args) -> Result<(), Box<dyn std::error::Error>> {
    let pool = PgPool::connect(&args.connection).await?;
    let row = sqlx::query(r#"SELECT "name" FROM "migrates" ORDER BY "name" DESC LIMIT 1"#)
        .fetch_optional(&pool)
        .await?;
    let name = match row {
        Some(r) => r.get::<String, _>(0),
        None => {
            println!("No applied migrations to roll back.");
            return Ok(());
        }
    };
    let downs = discover_down_files(&args.migrate_path)?;
    let path = downs
        .get(&name)
        .ok_or_else(|| format!("Cannot roll back \"{}\": no <stem>_down.sql sibling found", name))?
        .clone();
    let sql = fs::read_to_string(&path)?;

    let mut tx = pool.begin().await?;
    for stmt in split_statements(&sql) {
        tx.execute(stmt.as_str()).await?;
    }
    sqlx::query(r#"DELETE FROM "migrates" WHERE "name" = $1"#)
        .bind(&name)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    println!("Rolled back: {}", name);
    Ok(())
}
