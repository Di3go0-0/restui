use crate::model::request::Request;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileFormat {
    Http,
    Yaml,
}

#[derive(Debug, Clone)]
pub struct Collection {
    pub name: String,
    pub path: PathBuf,
    pub requests: Vec<Request>,
    pub format: FileFormat,
}

impl Collection {
    pub fn display_name(&self) -> &str {
        &self.name
    }
}
