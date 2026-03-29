use anyhow::Result;
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    pub general: GeneralConfig,
    #[serde(default)]
    pub theme: ThemeConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GeneralConfig {
    #[serde(default = "default_env")]
    pub default_environment: Option<String>,
    #[serde(default = "default_http_dirs")]
    pub http_file_dirs: Vec<PathBuf>,
    #[serde(default = "default_history_limit")]
    pub history_limit: usize,
    #[serde(default = "default_true")]
    pub follow_redirects: bool,
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    #[serde(default = "default_true")]
    pub verify_ssl: bool,
    #[serde(default = "default_chain_cache_ttl")]
    pub chain_cache_ttl: u64,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            default_environment: None,
            http_file_dirs: default_http_dirs(),
            history_limit: default_history_limit(),
            follow_redirects: true,
            timeout_secs: default_timeout(),
            verify_ssl: true,
            chain_cache_ttl: default_chain_cache_ttl(),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ThemeConfig {
    pub name: Option<String>,
    pub border_focused: Option<String>,
    pub border_unfocused: Option<String>,
    pub status_ok: Option<String>,
    pub status_client_error: Option<String>,
    pub status_server_error: Option<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            general: GeneralConfig::default(),
            theme: ThemeConfig::default(),
        }
    }
}

impl AppConfig {
    pub fn load() -> Result<Self> {
        let config_path = config_path();
        if config_path.exists() {
            let content = std::fs::read_to_string(&config_path)?;
            let config: AppConfig = toml::from_str(&content)?;
            Ok(config)
        } else {
            Ok(AppConfig::default())
        }
    }
}

pub fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("restui")
}

pub fn config_path() -> PathBuf {
    config_dir().join("config.toml")
}

pub fn data_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("restui")
}

fn default_env() -> Option<String> {
    None
}

fn default_http_dirs() -> Vec<PathBuf> {
    vec![PathBuf::from(".")]
}

fn default_history_limit() -> usize {
    100
}

fn default_timeout() -> u64 {
    30
}

fn default_true() -> bool {
    true
}

fn default_chain_cache_ttl() -> u64 {
    10
}
