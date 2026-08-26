use crate::error::RepositoryError;
use crate::sql_identifier::{
    quote_identifier_checked, quote_mysql_identifier, quote_sqlserver_identifier,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    Sqlite,
    Postgres,
    Mysql,
    Sqlserver,
    Oracle,
}

impl Dialect {
    pub fn quote(&self, name: &str) -> Result<String, RepositoryError> {
        match self {
            Dialect::Mysql => quote_mysql_identifier(name),
            Dialect::Sqlserver => quote_sqlserver_identifier(name),
            _ => quote_identifier_checked(name),
        }
    }

    pub fn placeholder(&self, index: usize) -> String {
        match self {
            Dialect::Sqlite | Dialect::Mysql => "?".to_string(),
            Dialect::Postgres => format!("${index}"),
            Dialect::Sqlserver => format!("@p{index}"),
            Dialect::Oracle => format!(":{index}"),
        }
    }

    pub fn pk_eq_placeholder(
        &self,
        pk_column: &str,
        index: usize,
    ) -> Result<String, RepositoryError> {
        let quoted = self.quote(pk_column)?;
        Ok(match self {
            Dialect::Sqlite | Dialect::Mysql => format!("{quoted} = ?"),
            Dialect::Postgres => format!("{quoted} = ${index}"),
            Dialect::Sqlserver => format!("{quoted} = @p{index}"),
            Dialect::Oracle => format!("{quoted} = :{index}"),
        })
    }

    pub fn order_by_pk(&self, pk_column: &str) -> Result<String, RepositoryError> {
        self.order_by_pks(&[pk_column.to_string()])
    }

    pub fn order_by_pks(&self, pk_columns: &[String]) -> Result<String, RepositoryError> {
        let mut parts = Vec::with_capacity(pk_columns.len());
        for column in pk_columns {
            parts.push(format!("{} ASC", self.quote(column)?));
        }
        Ok(format!("ORDER BY {}", parts.join(", ")))
    }

    pub fn identity_eq(
        &self,
        pk_columns: &[String],
        start_index: usize,
    ) -> Result<String, RepositoryError> {
        let mut parts = Vec::with_capacity(pk_columns.len());
        for (i, column) in pk_columns.iter().enumerate() {
            parts.push(self.pk_eq_placeholder(column, start_index + i)?);
        }
        Ok(parts.join(" AND "))
    }
}

pub fn build_select_by_id(
    dialect: Dialect,
    table: &str,
    pk_column: &str,
) -> Result<String, RepositoryError> {
    build_select_by_identity(dialect, table, &[pk_column.to_string()])
}

pub fn build_select_by_identity(
    dialect: Dialect,
    table: &str,
    pk_columns: &[String],
) -> Result<String, RepositoryError> {
    let quoted = dialect.quote(table)?;
    Ok(format!(
        "SELECT * FROM {quoted} WHERE {}",
        dialect.identity_eq(pk_columns, 1)?
    ))
}

pub fn build_select_all(
    dialect: Dialect,
    table: &str,
    pk_column: &str,
) -> Result<String, RepositoryError> {
    let quoted = dialect.quote(table)?;
    Ok(format!(
        "SELECT * FROM {quoted} {}",
        dialect.order_by_pk(pk_column)?
    ))
}

pub fn build_select_by_column(
    dialect: Dialect,
    table: &str,
    column: &str,
    pk_column: &str,
) -> Result<String, RepositoryError> {
    let table_quoted = dialect.quote(table)?;
    let column_quoted = dialect.quote(column)?;
    let placeholder = dialect.placeholder(1);
    Ok(format!(
        "SELECT * FROM {table_quoted} WHERE {column_quoted} = {placeholder} {}",
        dialect.order_by_pk(pk_column)?
    ))
}

pub fn build_insert(
    dialect: Dialect,
    table: &str,
    columns: &[&str],
    pk_column: &str,
) -> Result<String, RepositoryError> {
    let table_quoted = dialect.quote(table)?;
    let pk_quoted = dialect.quote(pk_column)?;
    let mut quoted_cols = Vec::with_capacity(columns.len());
    for c in columns {
        quoted_cols.push(dialect.quote(c)?);
    }
    let placeholders: Vec<String> = (1..=columns.len())
        .map(|i| dialect.placeholder(i))
        .collect();
    let cols_joined = quoted_cols.join(", ");
    let phs_joined = placeholders.join(", ");
    let sql = match dialect {
        Dialect::Postgres => {
            format!("INSERT INTO {table_quoted} ({cols_joined}) VALUES ({phs_joined}) RETURNING *")
        }
        Dialect::Sqlserver => format!(
            "INSERT INTO {table_quoted} ({cols_joined}) OUTPUT INSERTED.* VALUES ({phs_joined})"
        ),
        Dialect::Oracle => {
            let return_index = columns.len() + 1;
            format!(
                "INSERT INTO {table_quoted} ({cols_joined}) VALUES ({phs_joined}) RETURNING {pk_quoted} INTO :{return_index}"
            )
        }
        Dialect::Mysql => {
            format!("INSERT INTO {table_quoted} ({cols_joined}) VALUES ({phs_joined})")
        }
        Dialect::Sqlite => {
            // why RETURNING, not last_insert_rowid(): pooled rowid is per-connection and races under concurrent inserts
            format!("INSERT INTO {table_quoted} ({cols_joined}) VALUES ({phs_joined}) RETURNING *")
        }
    };
    Ok(sql)
}

pub fn build_update_identity(
    dialect: Dialect,
    table: &str,
    columns: &[&str],
    pk_columns: &[String],
) -> Result<String, RepositoryError> {
    let table_quoted = dialect.quote(table)?;
    let mut set_clauses = Vec::with_capacity(columns.len());
    for (i, c) in columns.iter().enumerate() {
        let q = dialect.quote(c)?;
        let ph = dialect.placeholder(i + 1);
        set_clauses.push(format!("{q} = {ph}"));
    }
    let id_clause = dialect.identity_eq(pk_columns, columns.len() + 1)?;
    let set_joined = set_clauses.join(", ");
    let sql = match dialect {
        Dialect::Postgres => {
            format!("UPDATE {table_quoted} SET {set_joined} WHERE {id_clause} RETURNING *")
        }
        Dialect::Sqlserver => {
            format!("UPDATE {table_quoted} SET {set_joined} OUTPUT INSERTED.* WHERE {id_clause}")
        }
        _ => format!("UPDATE {table_quoted} SET {set_joined} WHERE {id_clause}"),
    };
    Ok(sql)
}

pub fn build_update(
    dialect: Dialect,
    table: &str,
    columns: &[&str],
    pk_column: &str,
) -> Result<String, RepositoryError> {
    build_update_identity(dialect, table, columns, &[pk_column.to_string()])
}

pub fn build_select_in(
    dialect: Dialect,
    table: &str,
    column: &str,
    value_count: usize,
    pk_column: &str,
) -> Result<String, RepositoryError> {
    let table_quoted = dialect.quote(table)?;
    let column_quoted = dialect.quote(column)?;
    let placeholders: Vec<String> = (1..=value_count).map(|i| dialect.placeholder(i)).collect();
    let phs_joined = placeholders.join(", ");
    Ok(format!(
        "SELECT * FROM {table_quoted} WHERE {column_quoted} IN ({phs_joined}) {}",
        dialect.order_by_pk(pk_column)?
    ))
}

pub fn build_update_by(
    dialect: Dialect,
    table: &str,
    columns: &[&str],
    match_column: &str,
) -> Result<String, RepositoryError> {
    let table_quoted = dialect.quote(table)?;
    let match_quoted = dialect.quote(match_column)?;
    let mut set_clauses = Vec::with_capacity(columns.len());
    for (i, c) in columns.iter().enumerate() {
        let q = dialect.quote(c)?;
        let ph = dialect.placeholder(i + 1);
        set_clauses.push(format!("{q} = {ph}"));
    }
    let match_index = columns.len() + 1;
    let match_placeholder = dialect.placeholder(match_index);
    let set_joined = set_clauses.join(", ");
    let sql = match dialect {
        Dialect::Postgres => format!(
            "UPDATE {table_quoted} SET {set_joined} WHERE {match_quoted} = {match_placeholder} RETURNING *"
        ),
        Dialect::Sqlserver => format!(
            "UPDATE {table_quoted} SET {set_joined} OUTPUT INSERTED.* WHERE {match_quoted} = {match_placeholder}"
        ),
        _ => format!(
            "UPDATE {table_quoted} SET {set_joined} WHERE {match_quoted} = {match_placeholder}"
        ),
    };
    Ok(sql)
}

pub fn build_delete_by(
    dialect: Dialect,
    table: &str,
    column: &str,
) -> Result<String, RepositoryError> {
    let table_quoted = dialect.quote(table)?;
    let column_quoted = dialect.quote(column)?;
    let placeholder = dialect.placeholder(1);
    Ok(format!(
        "DELETE FROM {table_quoted} WHERE {column_quoted} = {placeholder}"
    ))
}

pub fn build_delete_identity(
    dialect: Dialect,
    table: &str,
    pk_columns: &[String],
) -> Result<String, RepositoryError> {
    let table_quoted = dialect.quote(table)?;
    let pk_quoted = dialect.quote(pk_columns.first().map(String::as_str).unwrap_or("id"))?;
    let id_clause = dialect.identity_eq(pk_columns, 1)?;
    let sql = match dialect {
        Dialect::Postgres => {
            format!("DELETE FROM {table_quoted} WHERE {id_clause} RETURNING {pk_quoted}")
        }
        Dialect::Sqlserver => {
            format!("DELETE FROM {table_quoted} OUTPUT DELETED.{pk_quoted} WHERE {id_clause}")
        }
        Dialect::Sqlite => {
            format!("DELETE FROM {table_quoted} WHERE {id_clause} RETURNING {pk_quoted}")
        }
        _ => format!("DELETE FROM {table_quoted} WHERE {id_clause}"),
    };
    Ok(sql)
}

pub fn build_delete(
    dialect: Dialect,
    table: &str,
    pk_column: &str,
) -> Result<String, RepositoryError> {
    build_delete_identity(dialect, table, &[pk_column.to_string()])
}
