@echo off
setlocal
set RUST_LOG=info
set CONFIG=config\router.toml

if not exist %CONFIG% (
    echo Configuration file %CONFIG% not found.
    exit /b 1
)

mcp-router.exe --config %CONFIG%
endlocal
