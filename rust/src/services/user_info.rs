use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserInfoResult {
    pub user_id: i64,
    pub username: String,
    pub roles: Vec<String>,
    pub scopes: Vec<String>,
    pub iat: i64,
    pub exp: i64,
}
