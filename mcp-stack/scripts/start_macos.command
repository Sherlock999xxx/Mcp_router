#!/bin/bash
set -euo pipefail
DIR="$(cd "$(dirname "$0")"/.. && pwd)"
export RUST_LOG=info
"$DIR/bin/mcp-router" --config "$DIR/config/router.toml"
