use std::{collections::HashMap, fs, path::Path};

use serde::{Deserialize, Serialize};

use crate::subs::Tier;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ServerConfig {
    pub bind: String,
    #[serde(default)]
    pub stdio_server: bool,
    #[serde(default)]
    pub auth_bearer: Option<String>,
    #[serde(default)]
    pub allow_origins: Vec<String>,
    #[serde(default)]
    pub database_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UpstreamCommand {
    pub kind: UpstreamKind,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub bearer: Option<String>,
    #[serde(default)]
    pub provider_slug: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum UpstreamKind {
    Stdio,
    Http,
}

impl std::fmt::Display for UpstreamKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UpstreamKind::Stdio => write!(f, "stdio"),
            UpstreamKind::Http => write!(f, "http"),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    pub server: ServerConfig,
    #[serde(default)]
    pub upstreams: HashMap<String, UpstreamCommand>,
    #[serde(default)]
    pub providers: Vec<ProviderSeed>,
}

impl Config {
    pub fn load_from(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let path = path.as_ref();
        let data = fs::read_to_string(path)?;
        let mut cfg: Self = toml::from_str(&data)?;
        if cfg.server.allow_origins.is_empty() {
            cfg.server.allow_origins.push("http://localhost".into());
        }
        if cfg.server.database_url.is_none() {
            cfg.server.database_url = Some("sqlite://mcp-router.db".into());
        }
        Ok(cfg)
    }

    pub fn example() -> Self {
        let server = ServerConfig {
            bind: "127.0.0.1:8848".into(),
            stdio_server: false,
            auth_bearer: None,
            allow_origins: vec!["http://localhost".into()],
            database_url: Some("sqlite://mcp-router.db".into()),
        };
        let mut upstreams = HashMap::new();
        upstreams.insert(
            "fs".into(),
            UpstreamCommand {
                kind: UpstreamKind::Stdio,
                command: Some("./target/release/mcp-fs".into()),
                args: vec!["--root".into(), "./".into()],
                url: None,
                bearer: None,
                provider_slug: None,
            },
        );
        upstreams.insert(
            "web".into(),
            UpstreamCommand {
                kind: UpstreamKind::Stdio,
                command: Some("./target/release/mcp-webfetch".into()),
                args: vec![],
                url: None,
                bearer: None,
                provider_slug: None,
            },
        );
        Self {
            server,
            upstreams,
            providers: vec![ProviderSeed {
                slug: "openai".into(),
                display_name: "OpenAI".into(),
                description: Some("OpenAI API".into()),
            }],
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self::example()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionPreset {
    pub name: Tier,
    pub max_tokens: i64,
    pub max_requests: i64,
    pub max_concurrent: i32,
}

impl SubscriptionPreset {
    pub fn defaults() -> Vec<Self> {
        vec![
            Self {
                name: Tier::Basic,
                max_tokens: 100_000,
                max_requests: 1_000,
                max_concurrent: 1,
            },
            Self {
                name: Tier::Pro,
                max_tokens: 1_000_000,
                max_requests: 10_000,
                max_concurrent: 3,
            },
            Self {
                name: Tier::Enterprise,
                max_tokens: 10_000_000,
                max_requests: 100_000,
                max_concurrent: 10,
            },
        ]
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProviderSeed {
    pub slug: String,
    pub display_name: String,
    pub description: Option<String>,
}
