@echo off
rem Bench Buddy checker: watches the golem logs, pages on http://127.0.0.1:8771/
rem Needs Rust (https://rustup.rs). A change to the code builds and starts it again here (exit code 3).
cd /d "%~dp0"
cargo run --release -q -p checker -- %*
:again
if "%errorlevel%"=="3" goto build
if "%errorlevel%"=="101" goto broken
goto :eof
:build
cargo run --release -q -p checker -- --no-browser %*
goto again
:broken
echo The build failed (see above). Fix it, then press a key to try again.
pause >nul
goto build
