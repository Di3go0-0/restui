use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::Duration;
use chrono::{DateTime, Local};
use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub status: u16,
    pub status_text: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
    pub content_type: Option<String>,
    #[serde(with = "duration_millis")]
    pub elapsed: Duration,
    pub size_bytes: usize,
    /// Raw bytes for binary responses — skipped in serialization
    #[serde(skip)]
    pub body_bytes: Option<Vec<u8>>,
}

mod duration_millis {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::Duration;

    pub fn serialize<S: Serializer>(d: &Duration, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_u64(d.as_millis() as u64)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Duration, D::Error> {
        let ms = u64::deserialize(d)?;
        Ok(Duration::from_millis(ms))
    }
}

impl Response {
    pub fn status_category(&self) -> StatusCategory {
        match self.status {
            200..=299 => StatusCategory::Success,
            300..=399 => StatusCategory::Redirect,
            400..=499 => StatusCategory::ClientError,
            500..=599 => StatusCategory::ServerError,
            _ => StatusCategory::Unknown,
        }
    }

    pub fn formatted_body(&self) -> String {
        // Try to pretty-print JSON
        if self
            .content_type
            .as_deref()
            .is_some_and(|ct| ct.contains("json"))
        {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&self.body) {
                if let Ok(pretty) = serde_json::to_string_pretty(&val) {
                    return pretty;
                }
            }
        }
        self.body.clone()
    }

    pub fn elapsed_display(&self) -> String {
        let ms = self.elapsed.as_millis();
        if ms < 1000 {
            format!("{}ms", ms)
        } else {
            format!("{:.2}s", self.elapsed.as_secs_f64())
        }
    }

    pub fn size_display(&self) -> String {
        if self.size_bytes < 1024 {
            format!("{}B", self.size_bytes)
        } else if self.size_bytes < 1024 * 1024 {
            format!("{:.1}KB", self.size_bytes as f64 / 1024.0)
        } else {
            format!("{:.1}MB", self.size_bytes as f64 / (1024.0 * 1024.0))
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseHistoryEntry {
    pub response: Response,
    pub timestamp: DateTime<Local>,
    #[serde(default)]
    pub request_fingerprint: String,
}

/// Wrapper for persistence
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ResponseHistories {
    pub data: std::collections::HashMap<String, VecDeque<ResponseHistoryEntry>>,
}

impl ResponseHistories {
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
        if let Ok(json) = serde_json::to_string(&self) {
            let _ = std::fs::write(path, json);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusCategory {
    Success,
    Redirect,
    ClientError,
    ServerError,
    Unknown,
}
