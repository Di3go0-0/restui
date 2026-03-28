use serde::{Deserialize, Serialize};

use crate::model::request::HttpMethod;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub method: HttpMethod,
    pub url: String,
    pub status: Option<u16>,
    pub elapsed_ms: Option<u64>,
    pub timestamp: String,
}
