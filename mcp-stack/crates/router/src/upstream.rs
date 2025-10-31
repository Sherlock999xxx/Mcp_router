use std::{collections::HashMap, process::Stdio, sync::Arc};

use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter},
    process::{Child, ChildStdin, ChildStdout, Command},
    sync::{Mutex, MutexGuard},
};
use tracing::warn;

use crate::{
    config::UpstreamKind,
    jsonrpc::{Id, Request, Response},
};

#[async_trait]
pub trait Upstream: Send + Sync {
    async fn call(&self, request: Request) -> Result<Response>;
}

pub type DynUpstream = Arc<dyn Upstream>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpstreamRegistration {
    pub name: String,
    pub kind: String,
    pub command: Option<String>,
    pub args: Vec<String>,
    pub url: Option<String>,
    pub bearer: Option<String>,
    pub provider_slug: Option<String>,
}

impl UpstreamRegistration {
    pub fn kind(&self) -> UpstreamKind {
        match self.kind.to_lowercase().as_str() {
            "http" => UpstreamKind::Http,
            _ => UpstreamKind::Stdio,
        }
    }
}

#[derive(Clone)]
pub struct UpstreamHandle {
    name: String,
    provider_slug: Option<String>,
    inner: DynUpstream,
    info: Arc<RwLock<Option<Value>>>,
}

impl UpstreamHandle {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn provider_slug(&self) -> Option<&str> {
        self.provider_slug.as_deref()
    }

    pub async fn call(&self, request: Request) -> Result<Response> {
        self.inner.call(request).await
    }

    pub async fn initialize(&self) -> Result<Value> {
        if let Some(info) = self.info.read().clone() {
            return Ok(info);
        }
        let request = Request {
            jsonrpc: "2.0".into(),
            id: Id::None,
            method: "initialize".into(),
            params: Value::default(),
        };
        let response = self.call(request).await?;
        let info = response
            .result
            .ok_or_else(|| anyhow!("initialize missing result for upstream {}", self.name))?;
        *self.info.write() = Some(info.clone());
        Ok(info)
    }
}

#[derive(Clone)]
pub struct UpstreamRegistry {
    entries: Arc<RwLock<HashMap<String, Arc<UpstreamHandle>>>>,
}

impl UpstreamRegistry {
    pub fn new() -> Self {
        Self {
            entries: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn register(&self, registration: UpstreamRegistration) -> Result<()> {
        let upstream: DynUpstream = match registration.kind() {
            UpstreamKind::Http => Arc::new(HttpUpstream::new(
                registration
                    .url
                    .clone()
                    .ok_or_else(|| anyhow!("http upstream requires url"))?,
                registration.bearer.clone(),
            )?),
            UpstreamKind::Stdio => Arc::new(StdioUpstream::new(
                registration
                    .command
                    .clone()
                    .ok_or_else(|| anyhow!("stdio upstream requires command"))?,
                registration.args.clone(),
            )?),
        };
        let handle = Arc::new(UpstreamHandle {
            name: registration.name.clone(),
            provider_slug: registration.provider_slug.clone(),
            inner: upstream,
            info: Arc::new(RwLock::new(None)),
        });
        self.entries
            .write()
            .insert(registration.name.clone(), handle);
        Ok(())
    }

    pub fn list(&self) -> Vec<Arc<UpstreamHandle>> {
        self.entries.read().values().cloned().collect::<Vec<_>>()
    }

    pub async fn call(&self, name: &str, request: Request) -> Result<Response> {
        let handle = self
            .entries
            .read()
            .get(name)
            .cloned()
            .ok_or_else(|| anyhow!("unknown upstream: {}", name))?;
        handle.call(request).await
    }

    pub async fn ensure_initialized(&self) -> Vec<(String, Value)> {
        let mut infos = Vec::new();
        for handle in self.list() {
            if let Ok(info) = handle.initialize().await {
                infos.push((handle.name.clone(), info));
            }
        }
        infos
    }
}

struct HttpUpstream {
    client: reqwest::Client,
    url: String,
    bearer: Option<String>,
    session: Arc<RwLock<Option<String>>>,
}

impl HttpUpstream {
    fn new(url: String, bearer: Option<String>) -> Result<Self> {
        let client = reqwest::Client::builder()
            .user_agent("mcp-router/0.1")
            .build()
            .context("build reqwest client")?;
        Ok(Self {
            client,
            url,
            bearer,
            session: Arc::new(RwLock::new(None)),
        })
    }
}

#[async_trait]
impl Upstream for HttpUpstream {
    async fn call(&self, request: Request) -> Result<Response> {
        let mut req = self
            .client
            .post(&self.url)
            .header("Accept", "application/json")
            .header("MCP-Protocol-Version", "2024-05-13")
            .json(&request);
        if let Some(token) = &self.bearer {
            req = req.bearer_auth(token);
        }
        if let Some(session) = self.session.read().clone() {
            req = req.header("Mcp-Session-Id", session);
        }
        let resp = req.send().await.context("send http request")?;
        if let Some(session) = resp
            .headers()
            .get("Mcp-Session-Id")
            .and_then(|value| value.to_str().ok())
        {
            *self.session.write() = Some(session.to_string());
        }
        let response = resp.error_for_status()?.json::<Response>().await?;
        Ok(response)
    }
}

struct StdioUpstream {
    command: String,
    args: Vec<String>,
    state: Mutex<Option<StdioState>>,
}

struct StdioState {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl StdioUpstream {
    fn new(command: String, args: Vec<String>) -> Result<Self> {
        Ok(Self {
            command,
            args,
            state: Mutex::new(None),
        })
    }

    async fn ensure_process(&self) -> Result<StdioGuard<'_>> {
        let mut guard = self.state.lock().await;
        let respawn = match guard.as_mut() {
            Some(state) => match state.child.try_wait() {
                Ok(Some(status)) => {
                    warn!(
                        command = %self.command,
                        ?status,
                        "stdio upstream exited unexpectedly; respawning"
                    );
                    true
                }
                Ok(None) => false,
                Err(err) => {
                    warn!(
                        command = %self.command,
                        ?err,
                        "failed to poll stdio upstream; respawning"
                    );
                    true
                }
            },
            None => true,
        };

        if respawn {
            *guard = None;
            let mut cmd = Command::new(&self.command);
            cmd.args(&self.args)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit());
            let mut child = cmd.spawn().context("spawn stdio upstream")?;
            let stdin = child
                .stdin
                .take()
                .ok_or_else(|| anyhow!("child missing stdin"))?;
            let stdout = child
                .stdout
                .take()
                .ok_or_else(|| anyhow!("child missing stdout"))?;
            *guard = Some(StdioState {
                child,
                stdin,
                stdout: BufReader::new(stdout),
            });
        }
        Ok(StdioGuard { guard })
    }
}

struct StdioGuard<'a> {
    guard: MutexGuard<'a, Option<StdioState>>,
}

impl<'a> StdioGuard<'a> {
    fn state_mut(&mut self) -> &mut StdioState {
        self.guard.as_mut().expect("state initialized")
    }

    fn reset(&mut self) {
        let _ = self.guard.take();
    }
}

#[async_trait]
impl Upstream for StdioUpstream {
    async fn call(&self, request: Request) -> Result<Response> {
        let mut guard = self.ensure_process().await?;
        let state = guard.state_mut();
        let mut stdin = BufWriter::new(&mut state.stdin);
        let payload = serde_json::to_vec(&request)?;
        stdin.write_all(&payload).await?;
        stdin.write_all(b"\n").await?;
        stdin.flush().await?;
        let mut line = String::new();
        let bytes = state.stdout.read_line(&mut line).await?;
        if bytes == 0 {
            guard.reset();
            return Err(anyhow!("upstream closed stream"));
        }
        let response: Response = serde_json::from_str(&line)?;
        Ok(response)
    }
}

#[cfg(test)]
impl UpstreamRegistry {
    pub fn register_test(&self, name: &str, upstream: DynUpstream, provider_slug: Option<String>) {
        let handle = Arc::new(UpstreamHandle {
            name: name.to_string(),
            provider_slug,
            inner: upstream,
            info: Arc::new(RwLock::new(None)),
        });
        self.entries.write().insert(name.to_string(), handle);
    }
}
