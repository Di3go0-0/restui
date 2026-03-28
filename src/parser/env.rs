use anyhow::Result;
use indexmap::IndexMap;
use std::collections::HashMap;
use std::path::Path;

use crate::model::environment::{Environment, EnvironmentStore};

pub fn parse_file(path: &Path) -> Result<EnvironmentStore> {
    let content = std::fs::read_to_string(path)?;
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");

    match ext {
        "json" => parse_json(&content),
        "yaml" | "yml" => parse_yaml(&content),
        _ => anyhow::bail!("Unsupported environment file format: {}", ext),
    }
}

fn parse_json(content: &str) -> Result<EnvironmentStore> {
    // Expected format: { "local": { "base_url": "...", "token": "..." }, "prod": { ... } }
    let map: HashMap<String, HashMap<String, String>> = serde_json::from_str(content)?;

    let environments = map
        .into_iter()
        .map(|(name, vars)| {
            let variables: IndexMap<String, String> = vars.into_iter().collect();
            Environment { name, variables }
        })
        .collect();

    Ok(EnvironmentStore {
        environments,
        active: Some(0),
    })
}

fn parse_yaml(content: &str) -> Result<EnvironmentStore> {
    let map: HashMap<String, HashMap<String, String>> = serde_yaml::from_str(content)?;

    let environments = map
        .into_iter()
        .map(|(name, vars)| {
            let variables: IndexMap<String, String> = vars.into_iter().collect();
            Environment { name, variables }
        })
        .collect();

    Ok(EnvironmentStore {
        environments,
        active: Some(0),
    })
}
