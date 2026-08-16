use std::collections::BTreeMap;
use std::sync::Arc;

use crate::mappings::type_field_converter::TypeFieldConverter;

// sqlx binds Value::String as TEXT and postgres won't implicitly cast text to timestamptz/uuid, so each converter's bind_sql_cast() is appended to its placeholder for server-side coercion (shared by Standard + Crud repos).
pub fn apply_converter_bind_casts(
    sql: String,
    columns: &[&str],
    column_datasource_types: &BTreeMap<String, String>,
    converters_by_type: &BTreeMap<String, Arc<dyn TypeFieldConverter>>,
) -> String {
    let mut indices_with_cast: Vec<(usize, &str)> = columns
        .iter()
        .enumerate()
        .filter_map(|(i, c)| {
            let ds_type = column_datasource_types.get(*c)?;
            let conv = converters_by_type.get(ds_type)?;
            let cast = conv.bind_sql_cast()?;
            Some((i + 1, cast))
        })
        .collect();
    indices_with_cast.sort_by(|a, b| b.0.cmp(&a.0));
    let mut out = sql;
    for (idx, cast) in indices_with_cast {
        out = inject_placeholder_cast(&out, idx, cast);
    }
    out
}

pub fn inject_placeholder_cast(sql: &str, idx: usize, cast: &str) -> String {
    let needle = format!("${}", idx);
    let mut out = String::with_capacity(sql.len() + 14);
    let mut remaining = sql;
    while let Some(pos) = remaining.find(&needle) {
        out.push_str(&remaining[..pos]);
        let end = pos + needle.len();
        let next_is_digit = remaining[end..]
            .chars()
            .next()
            .map_or(false, |c| c.is_ascii_digit());
        if next_is_digit {
            out.push_str(&remaining[pos..end]);
        } else {
            out.push_str(&needle);
            out.push_str(cast);
        }
        remaining = &remaining[end..];
    }
    out.push_str(remaining);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mappings::postgres_date_time_converter::PostgresDateTimeConverter;
    use serde_json::Value;

    fn datetime_converters() -> BTreeMap<String, Arc<dyn TypeFieldConverter>> {
        let mut m: BTreeMap<String, Arc<dyn TypeFieldConverter>> = BTreeMap::new();
        m.insert(
            "datetime".to_string(),
            Arc::new(PostgresDateTimeConverter) as Arc<dyn TypeFieldConverter>,
        );
        m
    }

    #[test]
    fn appends_timestamptz_cast_for_datetime_column_only() {
        let mut types = BTreeMap::new();
        types.insert("imported_at".to_string(), "datetime".to_string());
        let sql = "INSERT INTO t (\"key\", \"imported_at\") VALUES ($1, $2)".to_string();
        let out = apply_converter_bind_casts(
            sql,
            &["key", "imported_at"],
            &types,
            &datetime_converters(),
        );
        assert_eq!(
            out,
            "INSERT INTO t (\"key\", \"imported_at\") VALUES ($1, $2::timestamptz)"
        );
    }

    #[test]
    fn injects_cast_at_every_occurrence_of_the_placeholder() {
        let mut types = BTreeMap::new();
        types.insert("imported_at".to_string(), "datetime".to_string());
        let sql = "UPDATE t SET \"imported_at\" = $1 WHERE \"imported_at\" >= $1".to_string();
        let out = apply_converter_bind_casts(sql, &["imported_at"], &types, &datetime_converters());
        assert_eq!(
            out,
            "UPDATE t SET \"imported_at\" = $1::timestamptz WHERE \"imported_at\" >= $1::timestamptz"
        );
    }

    #[test]
    fn does_not_touch_two_digit_placeholders_when_injecting_single_digit() {
        let mut types = BTreeMap::new();
        types.insert("imported_at".to_string(), "datetime".to_string());
        // 11 columns: $1 should be cast but $11 must NOT pick up the cast as a $1 prefix.
        let cols: Vec<&str> = [
            "imported_at",
            "c2",
            "c3",
            "c4",
            "c5",
            "c6",
            "c7",
            "c8",
            "c9",
            "c10",
            "c11",
        ]
        .to_vec();
        let sql = "INSERT INTO t VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)".to_string();
        let out = apply_converter_bind_casts(sql, &cols, &types, &datetime_converters());
        assert!(out.contains("$1::timestamptz"));
        assert!(!out.contains("$11::timestamptz"));
        assert!(out.contains("$11"));
    }

    #[test]
    fn passthrough_when_no_converter_matches() {
        let types = BTreeMap::new();
        let converters = BTreeMap::new();
        let sql = "INSERT INTO t (\"name\") VALUES ($1)".to_string();
        let out = apply_converter_bind_casts(sql.clone(), &["name"], &types, &converters);
        assert_eq!(out, sql);
    }

    #[test]
    fn passthrough_when_converter_lacks_bind_cast() {
        struct NoCast;
        impl TypeFieldConverter for NoCast {
            fn from_datasource(
                &self,
            ) -> crate::repositories::datasource_middleware::DatabaseBackend {
                crate::repositories::datasource_middleware::DatabaseBackend::Postgres
            }
            fn datasource_type(&self) -> &str {
                "string"
            }
            fn from(&self, v: &Value) -> Value {
                v.clone()
            }
            fn to(&self, v: &Value) -> Value {
                v.clone()
            }
        }
        let mut types = BTreeMap::new();
        types.insert("name".to_string(), "string".to_string());
        let mut converters: BTreeMap<String, Arc<dyn TypeFieldConverter>> = BTreeMap::new();
        converters.insert(
            "string".to_string(),
            Arc::new(NoCast) as Arc<dyn TypeFieldConverter>,
        );
        let sql = "INSERT INTO t (\"name\") VALUES ($1)".to_string();
        let out = apply_converter_bind_casts(sql.clone(), &["name"], &types, &converters);
        assert_eq!(out, sql);
    }
}
