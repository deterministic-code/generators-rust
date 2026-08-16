use crate::error::RepositoryError;

// Parity with typescript/src/repositories/sqlIdentifier.ts: `^[A-Za-z_][A-Za-z0-9_]*$`.
// Legacy physical tables like `OldContactsTbl` declared in datasource_mappings.source
// are PascalCase by design — Rust must accept them to match the TS validator.
pub fn validate_identifier(name: &str) -> Result<(), RepositoryError> {
    if name.is_empty() {
        return Err(RepositoryError::InvalidIdentifier(name.to_string()));
    }
    let mut chars = name.chars();
    let first = chars.next().unwrap();
    if !(first.is_ascii_alphabetic() || first == '_') {
        return Err(RepositoryError::InvalidIdentifier(name.to_string()));
    }
    for c in chars {
        if !(c.is_ascii_alphanumeric() || c == '_') {
            return Err(RepositoryError::InvalidIdentifier(name.to_string()));
        }
    }
    Ok(())
}

pub fn quote_identifier(name: &str) -> String {
    validate_identifier(name).expect("invalid SQL identifier");
    format!("\"{name}\"")
}

pub fn quote_identifier_checked(name: &str) -> Result<String, RepositoryError> {
    validate_identifier(name)?;
    Ok(format!("\"{name}\""))
}

pub fn quote_mysql_identifier(name: &str) -> Result<String, RepositoryError> {
    validate_identifier(name)?;
    Ok(format!("`{name}`"))
}

pub fn quote_sqlserver_identifier(name: &str) -> Result<String, RepositoryError> {
    validate_identifier(name)?;
    Ok(format!("[{name}]"))
}
