async fn run_mysql(args: &Args) -> Result<(), Box<dyn std::error::Error>> {
    let pool = MySqlPool::connect(&args.connection).await?;
    let mut applied_set: std::collections::HashSet<String> =
        sqlx::query("SELECT `name` FROM `migrates`")
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

        // why no transaction: MySQL DDL auto-commits, so wrapping apply+INSERT gives no atomicity guarantee — mirror runUp's sequential execute+INSERT and accept the connection-failure window.
        for stmt in split_statements(&sql) {
            pool.execute(stmt.as_str()).await?;
        }
        sqlx::query("INSERT INTO `migrates` (`name`, `checksum`) VALUES (?, ?)")
            .bind(&name)
            .bind(&sum)
            .execute(&pool)
            .await?;
        println!("Applied: {}", name);
        applied_set.insert(name);
        applied_count += 1;
        if args.one {
            return Ok(());
        }
    }
}
