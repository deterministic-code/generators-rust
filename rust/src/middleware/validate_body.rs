use std::marker::PhantomData;

use crate::middleware::error_handler::{ApiErrorEntry, AppError};
use serde::de::DeserializeOwned;

pub struct ValidateBody<T> {
    _phantom: PhantomData<fn() -> T>,
}

impl<T: DeserializeOwned> ValidateBody<T> {
    pub fn new() -> Self {
        Self {
            _phantom: PhantomData,
        }
    }

    pub fn validate_json(bytes: &[u8]) -> Result<T, AppError> {
        serde_json::from_slice::<T>(bytes).map_err(|err| zod_style_error(err.to_string()))
    }
}

impl<T: DeserializeOwned> Default for ValidateBody<T> {
    fn default() -> Self {
        Self::new()
    }
}

pub fn zod_style_error(message: String) -> AppError {
    AppError::ValidationMany(vec![ApiErrorEntry {
        code: "VALIDATION_ERROR".to_string(),
        message,
    }])
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::response::IntoResponse;
    use http_body_util::BodyExt;
    use serde::Deserialize;

    #[derive(Deserialize, Debug, PartialEq)]
    struct Payload {
        name: String,
        age: u32,
    }

    #[tokio::test]
    async fn validate_json_accepts_well_formed_payload() {
        let body = br#"{"name":"alice","age":30}"#;
        let parsed = ValidateBody::<Payload>::validate_json(body).expect("ok");
        assert_eq!(
            parsed,
            Payload {
                name: "alice".to_string(),
                age: 30
            }
        );
    }

    #[tokio::test]
    async fn validate_json_rejects_missing_field_with_400_envelope() {
        let body = br#"{"name":"alice"}"#;
        let err = ValidateBody::<Payload>::validate_json(body).unwrap_err();
        let res = err.into_response();
        assert_eq!(res.status(), 400);
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["errors"][0]["code"].as_str().unwrap(), "VALIDATION_ERROR");
    }

    #[tokio::test]
    async fn validate_json_rejects_wrong_type_with_400_envelope() {
        let body = br#"{"name":"alice","age":"thirty"}"#;
        let err = ValidateBody::<Payload>::validate_json(body).unwrap_err();
        let res = err.into_response();
        assert_eq!(res.status(), 400);
    }
}
