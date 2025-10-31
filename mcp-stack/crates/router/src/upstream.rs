use std::{collections::HashMap, sync::Arc};

use tokio::sync::RwLock;

use crate::{
    config::{RouterConfig, UpstreamConfig},
    metrics::MetricsHandle,
};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum UpstreamDescriptor {
    Stdio { command: String, args: Vec<String> },
    Http { url: String },
}

#[derive(Clone)]
pub struct UpstreamRegistry {
    config: Arc<RwLock<HashMap<String, UpstreamConfig>>>,
    #[allow(dead_code)]
    metrics: MetricsHandle,
}

impl UpstreamRegistry {
    pub async fn from_config(
        config: &RouterConfig,
        metrics: MetricsHandle,
    ) -> anyhow::Result<Self> {
        let map = config.upstreams.clone();
        Ok(Self {
            config: Arc::new(RwLock::new(map)),
            metrics,
        })
    }

    pub async fn list(&self) -> Vec<(String, UpstreamDescriptor)> {
        let cfg = self.config.read().await;
        cfg.iter()
            .map(|(name, entry)| {
                let descriptor = match entry {
                    UpstreamConfig::Stdio { command, args, .. } => UpstreamDescriptor::Stdio {
                        command: command.clone(),
                        args: args.clone(),
                    },
                    UpstreamConfig::Http { url, .. } => {
                        UpstreamDescriptor::Http { url: url.clone() }
                    }
                };
                (name.clone(), descriptor)
            })
            .collect()
    }

    pub async fn add(&self, name: String, config: UpstreamConfig) {
        let mut map = self.config.write().await;
        map.insert(name, config);
    }
}
