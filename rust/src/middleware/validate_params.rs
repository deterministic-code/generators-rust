use std::marker::PhantomData;

use crate::middleware::error_handler::AppError;
use crate::middleware::validate_body::zod_style_error;
use serde::de::DeserializeOwned;

pub struct ValidateParams<T> {
    _phantom: PhantomData<fn() -> T>,
}

impl<T: DeserializeOwned> ValidateParams<T> {
    pub fn new() -> Self {
        Self {
            _phantom: PhantomData,
        }
    }

    pub fn validate_query(query: &str) -> Result<T, AppError> {
        serde_urlencoded::from_str::<T>(query).map_err(|err| zod_style_error(err.to_string()))
    }

    pub fn validate_path_map<I, K, V>(pairs: I) -> Result<T, AppError>
    where
        I: IntoIterator<Item = (K, V)>,
        K: AsRef<str>,
        V: AsRef<str>,
    {
        let mut buf = String::new();
        let mut first = true;
        for (k, v) in pairs {
            if !first {
                buf.push('&');
            }
            buf.push_str(&urlencoding::encode(k.as_ref()));
            buf.push('=');
            buf.push_str(&urlencoding::encode(v.as_ref()));
            first = false;
        }
        Self::validate_query(&buf)
    }
}

impl<T: DeserializeOwned> Default for ValidateParams<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Deserialize, Debug, PartialEq)]
    struct PathParams {
        id: u64,
    }

    #[test]
    fn validate_query_accepts_coercible_string_to_int() {
        let parsed = ValidateParams::<PathParams>::validate_query("id=42").expect("ok");
        assert_eq!(parsed, PathParams { id: 42 });
    }

    #[test]
    fn validate_query_rejects_non_numeric_id_with_400_envelope() {
        let err = ValidateParams::<PathParams>::validate_query("id=abc").unwrap_err();
        let app_err = match err {
            AppError::ValidationMany(_) => true,
            _ => false,
        };
        assert!(app_err);
    }

    #[test]
    fn validate_path_map_consumes_key_value_pairs() {
        let parsed = ValidateParams::<PathParams>::validate_path_map([("id", "7")]).expect("ok");
        assert_eq!(parsed, PathParams { id: 7 });
    }
}
