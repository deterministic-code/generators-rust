use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::Value;

use crate::mappings::field_mapping_translator::FieldMappingTranslator;
use crate::mappings::type_field_converter::TypeFieldConverter;
use crate::repositories::postgres::converter_bind_casts::apply_converter_bind_casts;
use crate::repositories::row_map::RowMap;

// Shared converter + field-mapping plumbing for PostgresStandardRepository and PostgresCrudRepository: both expose fields via accessors and inherit the write-row/bind-cast behavior without copy-paste.
pub trait PgConvertingRepo {
    fn column_datasource_types(&self) -> &BTreeMap<String, String>;
    fn converters_by_type(&self) -> &BTreeMap<String, Arc<dyn TypeFieldConverter>>;
    fn field_mapping_translator(&self) -> &Arc<FieldMappingTranslator>;

    fn apply_to(&self, logical_column: &str, value: Value) -> Value {
        let types = self.column_datasource_types();
        if types.is_empty() {
            return value;
        }
        let Some(ds_type) = types.get(logical_column) else {
            return value;
        };
        let Some(conv) = self.converters_by_type().get(ds_type) else {
            return value;
        };
        conv.to(&value)
    }

    fn apply_from_and_translate(&self, row: RowMap) -> RowMap {
        let logical_row = self.field_mapping_translator().to_logical_row(row);
        let types = self.column_datasource_types();
        let converters = self.converters_by_type();
        if types.is_empty() || converters.is_empty() {
            return logical_row;
        }
        let mut out = RowMap::new();
        for (logical_key, val) in logical_row {
            let converted = match types.get(&logical_key).and_then(|t| converters.get(t)) {
                Some(conv) => conv.from(&val),
                None => val,
            };
            out.insert(logical_key, converted);
        }
        out
    }

    fn prepare_write_row(&self, row: RowMap) -> RowMap {
        let converted: RowMap = row
            .into_iter()
            .map(|(k, v)| {
                let new_value = self.apply_to(&k, v);
                (k, new_value)
            })
            .collect();
        self.field_mapping_translator().to_physical_row(converted)
    }

    // why translate: `columns` are physical names post-prepare_write_row but column_datasource_types is keyed by logical names — without translating, no ::timestamptz cast is appended and datetime binds 500.
    fn apply_converter_bind_casts(&self, sql: String, columns: &[&str]) -> String {
        let translator = self.field_mapping_translator();
        let logical_owned: Vec<String> = columns.iter().map(|c| translator.to_logical(c)).collect();
        let logical_refs: Vec<&str> = logical_owned.iter().map(String::as_str).collect();
        apply_converter_bind_casts(
            sql,
            &logical_refs,
            self.column_datasource_types(),
            self.converters_by_type(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mappings::postgres_date_time_converter::PostgresDateTimeConverter;

    struct FakeRepo {
        types: BTreeMap<String, String>,
        converters: BTreeMap<String, Arc<dyn TypeFieldConverter>>,
        translator: Arc<FieldMappingTranslator>,
    }

    impl PgConvertingRepo for FakeRepo {
        fn column_datasource_types(&self) -> &BTreeMap<String, String> {
            &self.types
        }
        fn converters_by_type(&self) -> &BTreeMap<String, Arc<dyn TypeFieldConverter>> {
            &self.converters
        }
        fn field_mapping_translator(&self) -> &Arc<FieldMappingTranslator> {
            &self.translator
        }
    }

    fn datetime_repo() -> FakeRepo {
        let mut types = BTreeMap::new();
        types.insert("imported_at".to_string(), "datetime".to_string());
        let mut converters: BTreeMap<String, Arc<dyn TypeFieldConverter>> = BTreeMap::new();
        converters.insert(
            "datetime".to_string(),
            Arc::new(PostgresDateTimeConverter) as Arc<dyn TypeFieldConverter>,
        );
        FakeRepo {
            types,
            converters,
            translator: Arc::new(FieldMappingTranslator::new(None).unwrap()),
        }
    }

    #[test]
    fn default_apply_converter_bind_casts_adds_timestamptz_for_datetime_column() {
        let r = datetime_repo();
        let out = r.apply_converter_bind_casts(
            "INSERT INTO t (\"imported_at\") VALUES ($1)".to_string(),
            &["imported_at"],
        );
        assert!(out.contains("$1::timestamptz"));
    }

    #[test]
    fn no_types_registered_means_passthrough() {
        let r = FakeRepo {
            types: BTreeMap::new(),
            converters: BTreeMap::new(),
            translator: Arc::new(FieldMappingTranslator::new(None).unwrap()),
        };
        let sql = "INSERT INTO t (\"x\") VALUES ($1)".to_string();
        let out = r.apply_converter_bind_casts(sql.clone(), &["x"]);
        assert_eq!(out, sql);
    }
}
