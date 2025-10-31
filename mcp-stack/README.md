# MCP Stack

This repository contains a simplified MCP router and a collection of MCP-compatible servers implemented in Rust. The router exposes an HTTP API for JSON-RPC MCP methods, a static GUI, and Prometheus metrics. Companion binaries implement filesystem browsing, HTTP fetching, and AI provider proxies.

## Workspace Layout

- `crates/router`: Axum-based HTTP router.
- `crates/mcp-fs`: Filesystem MCP server (stdio).
- `crates/mcp-webfetch`: HTTP fetch MCP server (stdio).
- `crates/mcp-ollama`: Ollama proxy MCP server.
- `crates/mcp-openai`: OpenAI proxy MCP server.
- `crates/mcp-claude`: Claude proxy MCP server.
- `gui/`: Static dashboard assets.
- `config/`: Example router configuration.
- `migrations/`: SQLite migrations.
- `scripts/`: Build and runtime scripts.
- `packaging/`: Dockerfile and systemd unit.

## Building

```bash
cargo build --release
```

On Windows, run `scripts\build_and_pack.bat` to create a distribution archive in `dist/`.

## Running the Router

```bash
cargo run -p mcp-router -- config/router.toml
```

The router listens on `127.0.0.1:8848` by default and serves the GUI at `http://127.0.0.1:8848/`.

### Configuration highlights

- `config/router.toml` defines HTTP binding, optional bearer token, allowed origins, database location, and upstream MCP servers.
- The router persists users, subscriptions, providers, and usage in SQLite. Additional migrations can be placed in `migrations/` and will be applied automatically on startup.
- Provider API keys are encrypted at rest using AES-256-GCM. Set the environment variable `MCP_STACK_MASTER_KEY` to a base64-encoded 32-byte secret before launching the router to ensure deterministic encryption.

### Admin API

Authenticated HTTP endpoints under `/api/*` allow the GUI (or automation) to manage runtime state:

| Method | Path | Description |
| ------ | ---- | ----------- |
| `GET` | `/api/upstreams` | List configured upstream MCP servers |
| `POST` | `/api/upstreams` | Register a new HTTP or stdio upstream |
| `GET` | `/api/providers` | List registered AI providers |
| `POST` | `/api/providers` | Upsert provider metadata and encrypted API keys |
| `GET` | `/api/users` | List known users |
| `POST` | `/api/users` | Create or ensure a user exists |
| `GET` | `/api/subscriptions` | List subscription entitlements |
| `POST` | `/api/subscriptions` | Assign or update a subscription tier |

All admin endpoints require the bearer token configured under `[server].auth_bearer` (if empty, authentication is disabled).

### Streaming

Providers that support Server-Sent Events can be proxied through `GET /mcp/stream?server=<name>&...`. The router connects to the upstream stream endpoint and re-emits events to the browser or client in SSE format with keep-alive heartbeats.

## MCP Requests

Example `initialize` request:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "initialize"
}
```

Example `tools/list` request:

```json
{
  "jsonrpc": "2.0",
  "id": 2,
  "method": "tools/list"
}
```

Example `resources/read` request:

```json
{
  "jsonrpc": "2.0",
  "id": 3,
  "method": "resources/read",
  "params": { "uri": "mcp+router://stub/L3NhbXBsZQ==" }
}
```

## Scripts

- `scripts/build_and_pack.bat`: build and package Windows distribution.
- `scripts/start_windows.bat`: launch router on Windows.
- `scripts/start_macos.command`: launch on macOS.
- `scripts/start_linux.sh`: launch on Linux.

## Docker

```
docker build -t mcp-router -f packaging/Dockerfile .
```
Example `tools/call` request for OpenAI chat completion:

```json
{
  "jsonrpc": "2.0",
  "id": 4,
  "method": "tools/call",
  "params": {
    "name": "openai/chat_complete",
    "arguments": {
      "model": "gpt-4o-mini",
      "messages": [
        {"role": "system", "content": "You are a helpful assistant."},
        {"role": "user", "content": "Say hello."}
      ]
    },
    "user_id": "user-123"
  }
}
```

The router validates subscriptions before forwarding the call to the upstream provider, records quota usage, and exposes the result via JSON-RPC.
