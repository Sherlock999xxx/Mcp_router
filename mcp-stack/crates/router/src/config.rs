use std::{collections::HashMap, fs, path::Path};

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct RouterConfig {
    pub server: ServerConfig,
    pub database: DatabaseConfig,
    #[serde(default)]
    pub upstreams: HashMap<String, UpstreamConfig>,
}

impl RouterConfig {
    pub fn load_from_path<P: AsRef<Path>>(path: P) -> anyhow::Result<Self> {
        let raw = fs::read_to_string(path)?;
        let cfg = config::Config::builder()
            .add_source(config::File::from_str(&raw, config::FileFormat::Toml))
            .build()?;
        Ok(cfg.try_deserialize()?)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_bind")]
    pub bind: String,
    #[serde(default)]
    #[allow(dead_code)]
    pub stdio_server: bool,
    #[serde(default)]
    pub auth_bearer: Option<String>,
    #[serde(default)]
    pub allow_origins: Vec<String>,
}

fn default_bind() -> String {
    "127.0.0.1:8848".to_string()
}

#[derive(Debug, Clone, Deserialize)]
pub struct DatabaseConfig {
    #[serde(default = "default_db_path")]
    pub path: String,
}

fn default_db_path() -> String {
    "data/router.db".to_string()
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum UpstreamConfig {
    Stdio {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        #[allow(dead_code)]
        env: HashMap<String, String>,
    },
    Http {
        url: String,
        #[serde(default)]
        #[allow(dead_code)]
        bearer: Option<String>,
    },
}
