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
