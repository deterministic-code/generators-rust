use deterministic::repositories::sql_builder::{
    build_delete, build_insert, build_select_all, build_select_by_column, build_select_by_id,
    build_update, Dialect,
};

// SQLite

#[test]
fn sqlite_select_by_id() {
    let sql = build_select_by_id(Dialect::Sqlite, "contacts", "id").unwrap();
    assert_eq!(sql, "SELECT * FROM \"contacts\" WHERE \"id\" = ?");
}

#[test]
fn sqlite_select_all() {
    let sql = build_select_all(Dialect::Sqlite, "contacts", "id").unwrap();
    assert_eq!(sql, "SELECT * FROM \"contacts\" ORDER BY \"id\" ASC");
}

#[test]
fn sqlite_select_by_column() {
    let sql = build_select_by_column(Dialect::Sqlite, "contacts", "name", "id").unwrap();
    assert_eq!(
        sql,
        "SELECT * FROM \"contacts\" WHERE \"name\" = ? ORDER BY \"id\" ASC"
    );
}

#[test]
fn sqlite_insert() {
    let sql = build_insert(Dialect::Sqlite, "contacts", &["name", "age"], "id").unwrap();
    assert_eq!(
        sql,
        "INSERT INTO \"contacts\" (\"name\", \"age\") VALUES (?, ?) RETURNING *"
    );
}

#[test]
fn sqlite_update() {
    let sql = build_update(Dialect::Sqlite, "contacts", &["name", "age"], "id").unwrap();
    assert_eq!(
        sql,
        "UPDATE \"contacts\" SET \"name\" = ?, \"age\" = ? WHERE \"id\" = ?"
    );
}

#[test]
fn sqlite_delete_returns_id_so_missing_rows_yield_404() {
    let sql = build_delete(Dialect::Sqlite, "contacts", "id").unwrap();
    assert_eq!(
        sql,
        "DELETE FROM \"contacts\" WHERE \"id\" = ? RETURNING \"id\""
    );
}

// Postgres

#[test]
fn postgres_select_by_id_uses_dollar_placeholder() {
    let sql = build_select_by_id(Dialect::Postgres, "contacts", "id").unwrap();
    assert_eq!(sql, "SELECT * FROM \"contacts\" WHERE \"id\" = $1");
}

#[test]
fn postgres_insert_returns_star() {
    let sql = build_insert(Dialect::Postgres, "contacts", &["name", "age"], "id").unwrap();
    assert_eq!(
        sql,
        "INSERT INTO \"contacts\" (\"name\", \"age\") VALUES ($1, $2) RETURNING *"
    );
}

#[test]
fn postgres_update_uses_indexed_placeholders_and_returns_star() {
    let sql = build_update(Dialect::Postgres, "contacts", &["name", "age"], "id").unwrap();
    assert_eq!(
        sql,
        "UPDATE \"contacts\" SET \"name\" = $1, \"age\" = $2 WHERE \"id\" = $3 RETURNING *"
    );
}

#[test]
fn postgres_delete_returns_id() {
    let sql = build_delete(Dialect::Postgres, "contacts", "id").unwrap();
    assert_eq!(
        sql,
        "DELETE FROM \"contacts\" WHERE \"id\" = $1 RETURNING \"id\""
    );
}

// MySQL

#[test]
fn mysql_quotes_with_backticks() {
    let sql = build_select_by_id(Dialect::Mysql, "contacts", "id").unwrap();
    assert_eq!(sql, "SELECT * FROM `contacts` WHERE `id` = ?");
}

#[test]
fn mysql_insert_no_returning() {
    let sql = build_insert(Dialect::Mysql, "contacts", &["name"], "id").unwrap();
    assert_eq!(sql, "INSERT INTO `contacts` (`name`) VALUES (?)");
}

#[test]
fn mysql_update_uses_question_marks() {
    let sql = build_update(Dialect::Mysql, "contacts", &["name", "age"], "id").unwrap();
    assert_eq!(
        sql,
        "UPDATE `contacts` SET `name` = ?, `age` = ? WHERE `id` = ?"
    );
}

// SQL Server

#[test]
fn sqlserver_uses_brackets_and_at_p_placeholders() {
    let sql = build_select_by_id(Dialect::Sqlserver, "contacts", "id").unwrap();
    assert_eq!(sql, "SELECT * FROM [contacts] WHERE [id] = @p1");
}

#[test]
fn sqlserver_insert_outputs_inserted() {
    let sql = build_insert(Dialect::Sqlserver, "contacts", &["name", "age"], "id").unwrap();
    assert_eq!(
        sql,
        "INSERT INTO [contacts] ([name], [age]) OUTPUT INSERTED.* VALUES (@p1, @p2)"
    );
}

#[test]
fn sqlserver_update_outputs_inserted() {
    let sql = build_update(Dialect::Sqlserver, "contacts", &["name"], "id").unwrap();
    assert_eq!(
        sql,
        "UPDATE [contacts] SET [name] = @p1 OUTPUT INSERTED.* WHERE [id] = @p2"
    );
}

#[test]
fn sqlserver_delete_outputs_deleted_id() {
    let sql = build_delete(Dialect::Sqlserver, "contacts", "id").unwrap();
    assert_eq!(
        sql,
        "DELETE FROM [contacts] OUTPUT DELETED.[id] WHERE [id] = @p1"
    );
}

// Oracle

#[test]
fn oracle_uses_colon_placeholders() {
    let sql = build_select_by_id(Dialect::Oracle, "contacts", "id").unwrap();
    assert_eq!(sql, "SELECT * FROM \"contacts\" WHERE \"id\" = :1");
}

#[test]
fn oracle_insert_returns_id_into_extra_bind() {
    let sql = build_insert(Dialect::Oracle, "contacts", &["name", "age"], "id").unwrap();
    assert_eq!(
        sql,
        "INSERT INTO \"contacts\" (\"name\", \"age\") VALUES (:1, :2) RETURNING \"id\" INTO :3"
    );
}

#[test]
fn oracle_update_uses_indexed_colons() {
    let sql = build_update(Dialect::Oracle, "contacts", &["name"], "id").unwrap();
    assert_eq!(
        sql,
        "UPDATE \"contacts\" SET \"name\" = :1 WHERE \"id\" = :2"
    );
}

// Validation

#[test]
fn accepts_pascal_identifiers_and_rejects_injection() {
    assert!(build_select_by_id(Dialect::Sqlite, "Contacts", "id").is_ok());
    assert!(build_select_by_column(Dialect::Sqlite, "contacts", "Bad", "id").is_ok());
    assert!(build_select_all(Dialect::Postgres, "contacts;DROP", "id").is_err());
    assert!(build_insert(Dialect::Sqlite, "contacts", &["good", "bad-name"], "id").is_err());
    assert!(build_update(Dialect::Sqlite, "contacts", &["1bad"], "id").is_err());
}

#[test]
fn custom_pk_propagates_through_select_update_delete() {
    // why: samples pass `key` (string PK) here — id_eq_placeholder once baked "id" into the format string, so custom-PK repos queried a nonexistent id column (0 rows / 500).
    assert_eq!(
        build_select_by_id(Dialect::Sqlite, "notifications", "key").unwrap(),
        "SELECT * FROM \"notifications\" WHERE \"key\" = ?"
    );
    assert_eq!(
        build_update(Dialect::Postgres, "notifications", &["text"], "key").unwrap(),
        "UPDATE \"notifications\" SET \"text\" = $1 WHERE \"key\" = $2 RETURNING *"
    );
    assert_eq!(
        build_delete(Dialect::Mysql, "notifications", "key").unwrap(),
        "DELETE FROM `notifications` WHERE `key` = ?"
    );
}
