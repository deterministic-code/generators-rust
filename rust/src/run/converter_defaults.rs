use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use crate::mappings::{
    MysqlBinaryConverter, MysqlBooleanConverter, MysqlDateTimeConverter, MysqlUuidConverter,
    PostgresBinaryConverter, PostgresBooleanConverter, PostgresDateTimeConverter,
    PostgresUuidConverter, SqliteBinaryConverter, SqliteBooleanConverter, SqliteDateTimeConverter,
    SqliteUuidConverter, TypeFieldConverter,
};
use crate::run::datasource::DialectKind;

// why this helper exists: each entity's column→datasource_type map is built at boot from YAML. When wiring a CRUD repo we need the matching set of dialect-specific TypeFieldConverter instances for the *types in use* — keyed on the dialect so callers don't have to know which Rust struct fits MySQL vs Postgres vs SQLite.
pub fn default_field_converters_for_dialect(
    column_datasource_types: &BTreeMap<String, String>,
    dialect: &DialectKind,
) -> Vec<Arc<dyn TypeFieldConverter>> {
    let mut needed: BTreeSet<&str> = BTreeSet::new();
    for ds_type in column_datasource_types.values() {
        needed.insert(ds_type.as_str());
    }
    let mut out: Vec<Arc<dyn TypeFieldConverter>> = Vec::new();
    if needed.contains("boolean") {
        out.push(boolean_converter_for(dialect));
    }
    if needed.contains("datetime") {
        out.push(datetime_converter_for(dialect));
    }
    if needed.contains("uuid") {
        out.push(uuid_converter_for(dialect));
    }
    if needed.contains("binary") {
        out.push(binary_converter_for(dialect));
    }
    out
}

fn boolean_converter_for(dialect: &DialectKind) -> Arc<dyn TypeFieldConverter> {
    match dialect {
        DialectKind::Mysql => Arc::new(MysqlBooleanConverter),
        DialectKind::Postgres => Arc::new(PostgresBooleanConverter),
        DialectKind::Sqlite => Arc::new(SqliteBooleanConverter),
    }
}

fn datetime_converter_for(dialect: &DialectKind) -> Arc<dyn TypeFieldConverter> {
    match dialect {
        DialectKind::Mysql => Arc::new(MysqlDateTimeConverter),
        DialectKind::Postgres => Arc::new(PostgresDateTimeConverter),
        DialectKind::Sqlite => Arc::new(SqliteDateTimeConverter),
    }
}

fn uuid_converter_for(dialect: &DialectKind) -> Arc<dyn TypeFieldConverter> {
    match dialect {
        DialectKind::Mysql => Arc::new(MysqlUuidConverter),
        DialectKind::Postgres => Arc::new(PostgresUuidConverter),
        DialectKind::Sqlite => Arc::new(SqliteUuidConverter),
    }
}

fn binary_converter_for(dialect: &DialectKind) -> Arc<dyn TypeFieldConverter> {
    match dialect {
        DialectKind::Mysql => Arc::new(MysqlBinaryConverter),
        DialectKind::Postgres => Arc::new(PostgresBinaryConverter),
        DialectKind::Sqlite => Arc::new(SqliteBinaryConverter),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repositories::datasource_middleware::DatabaseBackend;

    fn map_with(entries: &[(&str, &str)]) -> BTreeMap<String, String> {
        entries
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    fn types_of(converters: &[Arc<dyn TypeFieldConverter>]) -> BTreeSet<&str> {
        converters.iter().map(|c| c.datasource_type()).collect()
    }

    #[test]
    fn empty_column_map_yields_no_converters() {
        let out = default_field_converters_for_dialect(&BTreeMap::new(), &DialectKind::Mysql);
        assert!(out.is_empty());
    }

    #[test]
    fn boolean_column_picks_mysql_boolean_converter_for_mysql_dialect() {
        let cols = map_with(&[("is_active", "boolean")]);
        let out = default_field_converters_for_dialect(&cols, &DialectKind::Mysql);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].datasource_type(), "boolean");
        assert_eq!(out[0].from_datasource(), DatabaseBackend::Mysql);
    }

    #[test]
    fn boolean_column_picks_sqlite_boolean_converter_for_sqlite_dialect() {
        let cols = map_with(&[("is_active", "boolean")]);
        let out = default_field_converters_for_dialect(&cols, &DialectKind::Sqlite);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].from_datasource(), DatabaseBackend::Sqlite);
    }

    #[test]
    fn boolean_column_picks_postgres_boolean_converter_for_postgres_dialect() {
        let cols = map_with(&[("is_active", "boolean")]);
        let out = default_field_converters_for_dialect(&cols, &DialectKind::Postgres);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].from_datasource(), DatabaseBackend::Postgres);
    }

    #[test]
    fn multiple_boolean_columns_dedupe_to_one_converter() {
        let cols = map_with(&[("is_active", "boolean"), ("is_archived", "boolean")]);
        let out = default_field_converters_for_dialect(&cols, &DialectKind::Mysql);
        assert_eq!(out.len(), 1, "one converter per type, not per column");
    }

    #[test]
    fn datetime_column_dispatches_per_dialect() {
        let cols = map_with(&[("created", "datetime")]);
        for (dialect, backend) in [
            (DialectKind::Mysql, DatabaseBackend::Mysql),
            (DialectKind::Postgres, DatabaseBackend::Postgres),
            (DialectKind::Sqlite, DatabaseBackend::Sqlite),
        ] {
            let out = default_field_converters_for_dialect(&cols, &dialect);
            assert_eq!(out.len(), 1);
            assert_eq!(out[0].datasource_type(), "datetime");
            assert_eq!(out[0].from_datasource(), backend);
        }
    }

    #[test]
    fn uuid_column_dispatches_per_dialect() {
        let cols = map_with(&[("uuid", "uuid")]);
        for (dialect, backend) in [
            (DialectKind::Mysql, DatabaseBackend::Mysql),
            (DialectKind::Postgres, DatabaseBackend::Postgres),
            (DialectKind::Sqlite, DatabaseBackend::Sqlite),
        ] {
            let out = default_field_converters_for_dialect(&cols, &dialect);
            assert_eq!(out.len(), 1);
            assert_eq!(out[0].datasource_type(), "uuid");
            assert_eq!(out[0].from_datasource(), backend);
        }
    }

    #[test]
    fn binary_column_dispatches_per_dialect() {
        let cols = map_with(&[("payload", "binary")]);
        for (dialect, backend) in [
            (DialectKind::Mysql, DatabaseBackend::Mysql),
            (DialectKind::Postgres, DatabaseBackend::Postgres),
            (DialectKind::Sqlite, DatabaseBackend::Sqlite),
        ] {
            let out = default_field_converters_for_dialect(&cols, &dialect);
            assert_eq!(out.len(), 1);
            assert_eq!(out[0].datasource_type(), "binary");
            assert_eq!(out[0].from_datasource(), backend);
        }
    }

    #[test]
    fn postgres_uuid_converter_carries_uuid_bind_cast() {
        let cols = map_with(&[("uuid", "uuid")]);
        let out = default_field_converters_for_dialect(&cols, &DialectKind::Postgres);
        assert_eq!(out[0].bind_sql_cast(), Some("::uuid"));
    }

    #[test]
    fn postgres_datetime_converter_carries_timestamptz_bind_cast() {
        let cols = map_with(&[("created", "datetime")]);
        let out = default_field_converters_for_dialect(&cols, &DialectKind::Postgres);
        assert_eq!(out[0].bind_sql_cast(), Some("::timestamptz"));
    }

    #[test]
    fn mixed_columns_emit_one_converter_per_distinct_type() {
        let cols = map_with(&[
            ("is_active", "boolean"),
            ("created", "datetime"),
            ("uuid", "uuid"),
            ("payload", "binary"),
        ]);
        let out = default_field_converters_for_dialect(&cols, &DialectKind::Postgres);
        let types = types_of(&out);
        assert_eq!(out.len(), 4);
        assert!(types.contains("boolean"));
        assert!(types.contains("datetime"));
        assert!(types.contains("uuid"));
        assert!(types.contains("binary"));
    }
}
