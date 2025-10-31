use std::{collections::HashMap, process::Stdio, sync::Arc};

use anyhow::{anyhow, Context};
use reqwest::Client;
use reqwest_eventsource::EventSource;
use serde::Serialize;
use serde_json::Value;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::Command,
    sync::RwLock,
};

use crate::{
    config::{UpstreamCommand, UpstreamKind},
    jsonrpc::{Request, Response},
};

#[derive(Clone)]
pub struct UpstreamRegistry {
    map: Arc<RwLock<HashMap<String, UpstreamHandle>>>,
}

impl UpstreamRegistry {
    pub fn new() -> Self {
        Self {
            map: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn register(&self, name: &str, command: UpstreamCommand) -> anyhow::Result<()> {
        let driver = UpstreamHandle::new(name, command.clone()).await?;
        self.map.write().await.insert(name.to_string(), driver);
        Ok(())
    }

    pub async fn list(&self) -> Vec<UpstreamSummary> {
        self.map
            .read()
            .await
            .values()
            .map(|handle| UpstreamSummary {
                name: handle.name.clone(),
                kind: handle.definition.kind.clone(),
                command: handle.definition.command.clone(),
                args: handle.definition.args.clone(),
                url: handle.definition.url.clone(),
                bearer: handle.definition.bearer.is_some(),
            })
            .collect()
    }

    pub async fn call(&self, name: &str, request: Request) -> anyhow::Result<Response> {
        let handle = self
            .map
            .read()
            .await
            .get(name)
            .cloned()
            .ok_or_else(|| anyhow!("unknown upstream: {name}"))?;
        handle.call(request).await
    }

    pub async fn broadcast(
        &self,
        method: &str,
        params: Value,
    ) -> Vec<(String, anyhow::Result<Response>)> {
        let handles: Vec<_> = self.map.read().await.values().cloned().collect();
        let mut responses = Vec::with_capacity(handles.len());
        for handle in handles {
            let request = Request::new(method, params.clone());
            let response = handle.call(request).await;
            responses.push((handle.name.clone(), response));
        }
        responses
    }

    pub async fn event_stream(
        &self,
        name: &str,
        query: &HashMap<String, String>,
    ) -> anyhow::Result<EventSource> {
        let handle = self
            .map
            .read()
            .await
            .get(name)
            .cloned()
            .ok_or_else(|| anyhow!("unknown upstream: {name}"))?;
        handle.event_stream(query)
    }
}

#[derive(Clone)]
pub struct UpstreamHandle {
    pub name: String,
    pub definition: UpstreamCommand,
    driver: UpstreamDriver,
}

impl UpstreamHandle {
    async fn new(name: &str, definition: UpstreamCommand) -> anyhow::Result<Self> {
        let driver = match definition.kind {
            UpstreamKind::Http => {
                let url = definition
                    .url
                    .clone()
                    .ok_or_else(|| anyhow!("http upstream requires url"))?;
                let http = HttpUpstream::new(name, url, definition.bearer.clone()).await?;
                UpstreamDriver::Http(Arc::new(http))
            }
            UpstreamKind::Stdio => {
                let command = definition
                    .command
                    .clone()
                    .ok_or_else(|| anyhow!("stdio upstream requires command"))?;
                let stdio = StdioUpstream::new(command, definition.args.clone());
                UpstreamDriver::Stdio(Arc::new(stdio))
            }
        };
        Ok(Self {
            name: name.to_string(),
            definition,
            driver,
        })
    }

    async fn call(&self, request: Request) -> anyhow::Result<Response> {
        match &self.driver {
            UpstreamDriver::Http(http) => http.call(request).await,
            UpstreamDriver::Stdio(stdio) => stdio.call(request).await,
        }
    }

    fn event_stream(&self, query: &HashMap<String, String>) -> anyhow::Result<EventSource> {
        match &self.driver {
            UpstreamDriver::Http(http) => http.stream(query),
            UpstreamDriver::Stdio(_) => Err(anyhow!("streaming not supported for stdio upstreams")),
        }
    }
}

#[derive(Clone)]
pub enum UpstreamDriver {
    Http(Arc<HttpUpstream>),
    Stdio(Arc<StdioUpstream>),
}

#[derive(Debug, Clone, Serialize)]
pub struct UpstreamSummary {
    pub name: String,
    pub kind: UpstreamKind,
    pub command: Option<String>,
    pub args: Vec<String>,
    pub url: Option<String>,
    pub bearer: bool,
}

pub struct HttpUpstream {
    client: Client,
    url: String,
    bearer: Option<String>,
}

impl HttpUpstream {
    pub async fn new(name: &str, url: String, bearer: Option<String>) -> anyhow::Result<Self> {
        let client = Client::builder()
            .user_agent(format!("mcp-router/{name}"))
            .build()
            .context("build http upstream client")?;
        Ok(Self {
            client,
            url,
            bearer,
        })
    }

    pub async fn call(&self, request: Request) -> anyhow::Result<Response> {
        let mut req = self.client.post(&self.url).json(&request);
        if let Some(token) = &self.bearer {
            req = req.bearer_auth(token);
        }
        let resp = req.send().await?.error_for_status()?;
        let value = resp.json::<Response>().await?;
        Ok(value)
    }

    pub fn stream(&self, query: &HashMap<String, String>) -> anyhow::Result<EventSource> {
        let mut req = self
            .client
            .get(format!("{}/stream", self.url.trim_end_matches('/')));
        if let Some(token) = &self.bearer {
            req = req.bearer_auth(token);
        }
        if !query.is_empty() {
            let pairs: Vec<(&str, &str)> = query
                .iter()
                .map(|(k, v)| (k.as_str(), v.as_str()))
                .collect();
            req = req.query(&pairs);
        }
        let source = EventSource::new(req).map_err(|err| anyhow!("start event stream: {err}"))?;
        Ok(source)
    }
}

pub struct StdioUpstream {
    command: String,
    args: Vec<String>,
}

impl StdioUpstream {
    pub fn new(command: String, args: Vec<String>) -> Self {
        Self { command, args }
    }

    pub async fn call(&self, request: Request) -> anyhow::Result<Response> {
        let mut child = Command::new(&self.command)
            .args(&self.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .with_context(|| format!("spawn stdio upstream {}", self.command))?;
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("failed to open stdin for stdio upstream"))?;
        let payload = serde_json::to_string(&request)?;
        stdin.write_all(payload.as_bytes()).await?;
        stdin.write_all(b"\n").await?;
        stdin.flush().await?;
        drop(stdin);
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("failed to read stdout from stdio upstream"))?;
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        reader.read_line(&mut line).await?;
        let response: Response = serde_json::from_str(line.trim())?;
        let status = child.wait().await?;
        if !status.success() {
            return Err(anyhow!("stdio upstream exited with status {status}"));
        }
        Ok(response)
    }
}
