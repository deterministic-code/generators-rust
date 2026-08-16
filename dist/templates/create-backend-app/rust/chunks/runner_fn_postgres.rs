async fn run_postgres(args: &Args) -> Result<(), Box<dyn std::error::Error>> {
    let pool = PgPool::connect(&args.connection).await?;
    let mut applied_set: std::collections::HashSet<String> =
        sqlx::query(r#"SELECT "name" FROM "migrates""#)
            .fetch_all(&pool)
            .await?
            .into_iter()
            .map(|r| r.get::<String, _>(0))
            .collect();

    let files = discover_up_files(&args.migrate_path)?;
    let mut applied_count = 0usize;
    loop {
        let next = files.iter().find(|(n, _)| !applied_set.contains(n)).cloned();
        let (name, path) = match next {
            Some(t) => t,
            None => {
                if applied_count == 0 {
                    println!("No pending migrations.");
                } else {
                    println!("No more pending migrations.");
                }
                return Ok(());
            }
        };
        let sql = fs::read_to_string(&path)?;
        let sum = checksum_hex(&sql);

        let mut tx = pool.begin().await?;
        for stmt in split_statements(&sql) {
            tx.execute(stmt.as_str()).await?;
        }
        sqlx::query(r#"INSERT INTO "migrates" ("name", "checksum") VALUES ($1, $2)"#)
            .bind(&name)
            .bind(&sum)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        println!("Applied: {}", name);
        applied_set.insert(name);
        applied_count += 1;
        if args.one {
            return Ok(());
        }
    }
}
