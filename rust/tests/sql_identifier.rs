use deterministic::sql_identifier::{
    quote_identifier, quote_identifier_checked, quote_mysql_identifier, quote_sqlserver_identifier,
    validate_identifier,
};

#[test]
fn accepts_lowercase_snake_case() {
    assert!(validate_identifier("contacts").is_ok());
    assert!(validate_identifier("contact_emails").is_ok());
    assert!(validate_identifier("a1_b2_c3").is_ok());
    assert!(validate_identifier("_internal_v2").is_ok());
}

#[test]
fn accepts_mixed_case_legacy_identifiers() {
    // TS parity (typescript/src/repositories/sqlIdentifier.ts): PascalCase /
    // camelCase tables from datasource_mappings.source must validate.
    assert!(validate_identifier("OldContactsTbl").is_ok());
    assert!(validate_identifier("UserAccount").is_ok());
    assert!(validate_identifier("legacyTbl").is_ok());
    assert!(validate_identifier("CntID").is_ok());
}

#[test]
fn rejects_empty_string() {
    assert!(validate_identifier("").is_err());
}

#[test]
fn rejects_leading_digit() {
    assert!(validate_identifier("1contacts").is_err());
}

#[test]
fn rejects_special_characters() {
    assert!(validate_identifier("contacts;DROP").is_err());
    assert!(validate_identifier("contacts-emails").is_err());
    assert!(validate_identifier("contacts.emails").is_err());
    assert!(validate_identifier("contacts emails").is_err());
}

#[test]
fn quote_identifier_uses_double_quotes() {
    assert_eq!(quote_identifier("contacts"), "\"contacts\"");
}

#[test]
fn quote_identifier_checked_returns_error_for_invalid() {
    assert!(quote_identifier_checked("bad column").is_err());
    assert!(quote_identifier_checked("bad;DROP").is_err());
}

#[test]
fn mysql_quoting_uses_backticks() {
    assert_eq!(quote_mysql_identifier("contacts").unwrap(), "`contacts`");
}

#[test]
fn sqlserver_quoting_uses_brackets() {
    assert_eq!(
        quote_sqlserver_identifier("contacts").unwrap(),
        "[contacts]"
    );
}
