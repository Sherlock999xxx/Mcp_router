@echo off
setlocal
echo Starting MCP Router...
set RUST_LOG=info
mcp-router.exe --config config\router.toml
endlocal
