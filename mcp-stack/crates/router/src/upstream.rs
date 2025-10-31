use std::{collections::HashMap, sync::Arc};

use anyhow::{anyhow, Context};
use async_trait::async_trait;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter},
    process::{Child, Command},
    sync::Mutex,
};

use crate::{
    config::{Config, UpstreamKind},
    jsonrpc::{Request, Response},
};

pub type DynUpstream = Arc<dyn Upstream>;

#[async_trait]
pub trait Upstream: Send + Sync {
    async fn call(&self, request: Request) -> anyhow::Result<Response>;
}

pub struct UpstreamRegistry {
    map: Mutex<HashMap<String, DynUpstream>>,
}

impl UpstreamRegistry {
    pub fn new() -> Self {
        Self {
            map: Mutex::new(HashMap::new()),
        }
    }

    pub async fn insert(&self, name: impl Into<String>, upstream: DynUpstream) {
        self.map.lock().await.insert(name.into(), upstream);
    }

    pub async fn call(&self, name: &str, request: Request) -> anyhow::Result<Response> {
        let map = self.map.lock().await;
        let upstream = map
            .get(name)
            .ok_or_else(|| anyhow!("unknown upstream: {}", name))?
            .clone();
        drop(map);
        upstream.call(request).await
    }

    pub async fn list_names(&self) -> Vec<String> {
        self.map.lock().await.keys().cloned().collect()
    }

    pub async fn load_from_config(&self, config: &Config) -> anyhow::Result<()> {
        for (name, upstream) in &config.upstreams {
            let dyn_upstream: DynUpstream = match upstream.kind {
                UpstreamKind::Http => {
                    let url = upstream.url.clone().context("http upstream missing url")?;
                    Arc::new(HttpUpstream::new(url, upstream.bearer.clone())?)
                }
                UpstreamKind::Stdio => {
                    let command = upstream
                        .command
                        .clone()
                        .context("stdio upstream missing command")?;
                    Arc::new(
                        StdioUpstream::spawn(name.clone(), command, upstream.args.clone()).await?,
                    )
                }
            };
            self.insert(name.clone(), dyn_upstream).await;
        }
        Ok(())
    }
}

pub struct HttpUpstream {
    client: reqwest::Client,
    url: String,
    bearer: Option<String>,
}

impl HttpUpstream {
    pub fn new(url: String, bearer: Option<String>) -> anyhow::Result<Self> {
        let client = reqwest::Client::builder()
            .user_agent("mcp-router/0.1")
            .build()
            .context("build reqwest client")?;
        Ok(Self {
            client,
            url,
            bearer,
        })
    }
}

#[async_trait]
impl Upstream for HttpUpstream {
    async fn call(&self, request: Request) -> anyhow::Result<Response> {
        let mut req = self.client.post(&self.url).json(&request);
        if let Some(token) = &self.bearer {
            req = req.bearer_auth(token);
        }
        let resp = req.send().await?.error_for_status()?;
        let value = resp.json::<Response>().await?;
        Ok(value)
    }
}

pub struct StubUpstream {
    pub name: String,
}

#[async_trait]
impl Upstream for StubUpstream {
    async fn call(&self, request: Request) -> anyhow::Result<Response> {
        let payload = serde_json::json!({
            "upstream": self.name,
            "echo": request.params,
        });
        Ok(Response::result(request.id, payload))
    }
}

struct StdioProcess {
    #[allow(dead_code)]
    child: Child,
    stdin: BufWriter<tokio::process::ChildStdin>,
    stdout: tokio::io::Lines<BufReader<tokio::process::ChildStdout>>,
}

pub struct StdioUpstream {
    name: String,
    process: Mutex<StdioProcess>,
}

impl StdioUpstream {
    pub async fn spawn(
        name: impl Into<String>,
        command: String,
        args: Vec<String>,
    ) -> anyhow::Result<Self> {
        let mut cmd = Command::new(&command);
        cmd.args(&args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit());
        let mut child = cmd
            .spawn()
            .with_context(|| format!("spawn stdio upstream {}", command))?;
        let stdin = child.stdin.take().context("stdio upstream missing stdin")?;
        let stdout = child
            .stdout
            .take()
            .context("stdio upstream missing stdout")?;
        let process = StdioProcess {
            child,
            stdin: BufWriter::new(stdin),
            stdout: BufReader::new(stdout).lines(),
        };
        Ok(Self {
            name: name.into(),
            process: Mutex::new(process),
        })
    }
}

#[async_trait]
impl Upstream for StdioUpstream {
    async fn call(&self, request: Request) -> anyhow::Result<Response> {
        let mut process = self.process.lock().await;
        if let Some(status) = process.child.try_wait().context("poll stdio upstream")? {
            return Err(anyhow!(
                "stdio upstream {} exited with status {}",
                self.name,
                status
            ));
        }
        let payload = serde_json::to_string(&request)?;
        process.stdin.write_all(payload.as_bytes()).await?;
        process.stdin.write_all(b"\n").await?;
        process.stdin.flush().await?;
        let line = process
            .stdout
            .next_line()
            .await
            .context("read response from stdio upstream")?;
        let Some(line) = line else {
            return Err(anyhow!("stdio upstream {} closed stdout", self.name));
        };
        let response = serde_json::from_str::<Response>(&line)
            .context("deserialize stdio upstream response")?;
        Ok(response)
    }
}
