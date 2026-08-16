use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StandardTable {
    pub id: i64,
    pub uuid: String,
    pub created: String,
    pub updated: String,
}

pub trait HasStandardFields {
    fn id(&self) -> i64;
    fn uuid(&self) -> &str;
    fn created(&self) -> &str;
    fn updated(&self) -> &str;
}

impl HasStandardFields for StandardTable {
    fn id(&self) -> i64 {
        self.id
    }
    fn uuid(&self) -> &str {
        &self.uuid
    }
    fn created(&self) -> &str {
        &self.created
    }
    fn updated(&self) -> &str {
        &self.updated
    }
}
