use std::time::Duration;

#[derive(Debug, Clone)]
pub struct Response {
    pub status: u16,
    pub status_text: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
    pub content_type: Option<String>,
    pub elapsed: Duration,
    pub size_bytes: usize,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusCategory {
    Success,
    Redirect,
    ClientError,
    ServerError,
    Unknown,
}
