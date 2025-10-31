#!/bin/bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
export RUST_LOG=info
"$SCRIPT_DIR/../dist/windows-x86_64/bin/mcp-router" --config "$SCRIPT_DIR/../config/router.toml"
