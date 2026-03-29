use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HttpMethod {
    GET,
    POST,
    PUT,
    PATCH,
    DELETE,
    HEAD,
    OPTIONS,
}

impl fmt::Display for HttpMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HttpMethod::GET => write!(f, "GET"),
            HttpMethod::POST => write!(f, "POST"),
            HttpMethod::PUT => write!(f, "PUT"),
            HttpMethod::PATCH => write!(f, "PATCH"),
            HttpMethod::DELETE => write!(f, "DELETE"),
            HttpMethod::HEAD => write!(f, "HEAD"),
            HttpMethod::OPTIONS => write!(f, "OPTIONS"),
        }
    }
}

impl HttpMethod {
    pub const ALL: &'static [HttpMethod] = &[
        HttpMethod::GET,
        HttpMethod::POST,
        HttpMethod::PUT,
        HttpMethod::PATCH,
        HttpMethod::DELETE,
        HttpMethod::HEAD,
        HttpMethod::OPTIONS,
    ];

    pub fn next(self) -> Self {
        let all = Self::ALL;
        let idx = all.iter().position(|&m| m == self).unwrap_or(0);
        all[(idx + 1) % all.len()]
    }

    pub fn prev(self) -> Self {
        let all = Self::ALL;
        let idx = all.iter().position(|&m| m == self).unwrap_or(0);
        all[(idx + all.len() - 1) % all.len()]
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_uppercase().as_str() {
            "GET" => Some(HttpMethod::GET),
            "POST" => Some(HttpMethod::POST),
            "PUT" => Some(HttpMethod::PUT),
            "PATCH" => Some(HttpMethod::PATCH),
            "DELETE" => Some(HttpMethod::DELETE),
            "HEAD" => Some(HttpMethod::HEAD),
            "OPTIONS" => Some(HttpMethod::OPTIONS),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Header {
    pub name: String,
    pub value: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryParam {
    pub key: String,
    pub value: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cookie {
    pub name: String,
    pub value: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathParam {
    pub key: String,
    pub value: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    pub name: Option<String>,
    pub method: HttpMethod,
    pub url: String,
    pub headers: Vec<Header>,
    pub query_params: Vec<QueryParam>,
    pub cookies: Vec<Cookie>,
    pub path_params: Vec<PathParam>,
    pub body: Option<String>,
    #[serde(skip)]
    pub source_file: Option<PathBuf>,
    #[serde(skip)]
    pub source_line: Option<usize>,
}

impl Default for Request {
    fn default() -> Self {
        Self {
            name: None,
            method: HttpMethod::GET,
            url: String::from("https://"),
            headers: Vec::new(),
            query_params: Vec::new(),
            cookies: Vec::new(),
            path_params: Vec::new(),
            body: None,
            source_file: None,
            source_line: None,
        }
    }
}

impl Request {
    pub fn display_name(&self) -> String {
        if let Some(ref name) = self.name {
            name.clone()
        } else {
            format!("{} {}", self.method, self.url)
        }
    }
}
