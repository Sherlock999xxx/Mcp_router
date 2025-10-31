# MCP Stack (Preview)

This repository hosts a simplified MCP router and supporting services written in Rust. The project exposes an Axum HTTP API, serves a minimal dark-themed GUI, and persists configuration data in SQLite.

## Building

```bash
cargo build --release
```

## Running

```bash
cargo run --release --bin mcp-router -- --config config/router.toml
```

## Packaging (Windows)

Run `scripts/build_and_pack.bat` from a developer command prompt. The script builds the workspace, gathers binaries and assets into `dist/windows-x86_64`, and produces `dist/mcp-stack-windows.zip`.

## HTTP Endpoints

- `POST /mcp` – JSON-RPC interface supporting `initialize`, `tools/list`, `tools/call`, `prompts/list`, `prompts/get`, `resources/list`, and `resources/read`.
- `GET /mcp/stream` – Server-sent events channel emitting heartbeats.
- `GET /healthz` – Basic health probe.
- `GET /metrics` – Prometheus metrics.
- `GET/POST /api/upstreams` – List and add upstream definitions.
- `GET/POST /api/providers` – List and register providers.
- `GET/POST /api/subscriptions` – List and manage subscriptions.
- `GET/POST /api/users` – List and create users.

## Database

SQLite migrations are located under `migrations/`. The router automatically runs migrations on startup.

## GUI

Static assets live under `gui/`. The SPA fetches lists of upstreams, providers, and users.
