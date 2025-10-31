@echo off
setlocal enabledelayedexpansion
where cargo >nul 2>&1
if errorlevel 1 (
    echo Cargo is required but was not found in PATH.
    exit /b 1
)

cargo build --release
if errorlevel 1 exit /b 1

set DIST=dist\windows-x86_64
set BIN=%DIST%\bin
if exist %DIST% rmdir /s /q %DIST%
mkdir %BIN%

for %%F in (mcp-router mcp-fs mcp-webfetch mcp-ollama mcp-openai mcp-claude) do (
    copy target\release\%%F.exe %BIN%\ >nul
)

xcopy gui %DIST%\gui /E /I /Y >nul
xcopy config %DIST%\config /E /I /Y >nul
xcopy scripts\start_windows.bat %DIST%\ /Y >nul
copy README.md %DIST%\README.md >nul
xcopy migrations %DIST%\migrations /E /I /Y >nul

powershell -NoProfile -Command "Compress-Archive -Path %DIST%\* -DestinationPath dist\mcp-stack-windows.zip -Force"
if errorlevel 1 exit /b 1

echo Archive created at dist\mcp-stack-windows.zip
endlocal
