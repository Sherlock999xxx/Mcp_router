@echo off
setlocal ENABLEDELAYEDEXPANSION

where cargo >nul 2>&1
if errorlevel 1 (
    echo [ERROR] cargo not found in PATH. Please install Rust toolchain.
    exit /b 1
)

echo Building workspace in release mode...
cargo build --release
if errorlevel 1 (
    echo [ERROR] cargo build failed.
    exit /b 1
)

set DIST=dist\windows-x86_64
set BINDIR=%DIST%\bin

if exist %DIST% (rmdir /s /q %DIST%)
mkdir %BINDIR%

copy target\release\mcp-router.exe %BINDIR%\
copy target\release\mcp-fs.exe %BINDIR%\
copy target\release\mcp-webfetch.exe %BINDIR%\
copy target\release\mcp-ollama.exe %BINDIR%\
copy target\release\mcp-openai.exe %BINDIR%\
copy target\release\mcp-claude.exe %BINDIR%\

xcopy gui %DIST%\gui /E /I /Y
xcopy config %DIST%\config /E /I /Y
xcopy migrations %DIST%\migrations /E /I /Y
copy scripts\start_windows.bat %DIST%\
copy scripts\start_macos.command %DIST%\
copy scripts\start_linux.sh %DIST%\
copy README.md %DIST%\

powershell -NoProfile -Command "Compress-Archive -Path %DIST%\* -DestinationPath dist\mcp-stack-windows.zip -Force"

echo Package ready: dist\mcp-stack-windows.zip

echo To start: unzip and run scripts\start_windows.bat

endlocal
