use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::model::request::HttpMethod;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub method: HttpMethod,
    pub url: String,
    pub name: Option<String>,
    pub status: u16,
    pub status_text: String,
    pub elapsed_ms: u64,
    pub size_bytes: usize,
    pub timestamp: String,
    pub body_preview: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct History {
    pub entries: Vec<HistoryEntry>,
}

impl History {
    pub fn add(&mut self, entry: HistoryEntry, limit: usize) {
        self.entries.insert(0, entry);
        self.entries.truncate(limit);
    }

    pub fn load(path: &PathBuf) -> Self {
        if path.exists() {
            std::fs::read_to_string(path)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default()
        } else {
            Self::default()
        }
    }

    pub fn save(&self, path: &PathBuf) {
        let _ = std::fs::create_dir_all(path.parent().unwrap_or(&PathBuf::from(".")));
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(path, json);
        }
    }
}
