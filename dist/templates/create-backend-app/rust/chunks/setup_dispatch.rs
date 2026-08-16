"{{Dialect}}" => {
    let pool = {{PoolCtor}};
    sqlx::query({{Prefix}}_MIGRATES_DDL).execute(&pool).await?;
    sqlx::query({{Prefix}}_MIGRATE_LOGS_DDL).execute(&pool).await?;
}
