pub mod env;
pub mod http;
pub mod yaml;

use std::path::{Path, PathBuf};

use crate::model::collection::{Collection, FileFormat};
use crate::model::environment::EnvironmentStore;

pub fn scan_directories(dirs: &[PathBuf]) -> Vec<Collection> {
    let mut collections = Vec::new();

    for dir in dirs {
        if !dir.exists() {
            continue;
        }

        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => continue,
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                if let Some(collection) = load_collection(&path) {
                    collections.push(collection);
                }
            }
        }
    }

    collections.sort_by(|a, b| a.name.cmp(&b.name));
    collections
}

fn load_collection(path: &Path) -> Option<Collection> {
    let ext = path.extension()?.to_str()?;
    let name = path.file_stem()?.to_str()?.to_string();

    match ext {
        "http" | "rest" => {
            let content = std::fs::read_to_string(path).ok()?;
            let requests = http::parse(&content).ok()?;
            Some(Collection {
                name,
                path: path.to_path_buf(),
                requests,
                format: FileFormat::Http,
            })
        }
        "yaml" | "yml" => {
            let content = std::fs::read_to_string(path).ok()?;
            // Only parse as request collection if it has the right structure
            let requests = yaml::parse(&content).ok()?;
            if requests.is_empty() {
                return None;
            }
            Some(Collection {
                name,
                path: path.to_path_buf(),
                requests,
                format: FileFormat::Yaml,
            })
        }
        _ => None,
    }
}

pub fn load_environments(env_file: Option<&str>) -> EnvironmentStore {
    // Try explicit file first
    if let Some(file) = env_file {
        let path = PathBuf::from(file);
        if path.exists() {
            if let Ok(store) = env::parse_file(&path) {
                return store;
            }
        }
    }

    // Auto-discover env files in current directory
    let candidates = [
        "env.json",
        "env.yaml",
        "env.yml",
        ".env.json",
        "environments.json",
        "environments.yaml",
    ];

    for candidate in &candidates {
        let path = PathBuf::from(candidate);
        if path.exists() {
            if let Ok(store) = env::parse_file(&path) {
                return store;
            }
        }
    }

    EnvironmentStore::default()
}
