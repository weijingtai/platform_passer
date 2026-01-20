# Platform Passer

A high-performance, cross-platform keyboard and mouse sharing application written in Rust.

## Prerequisites

- **Rust**: stable toolchain.
- **Windows**: Build tools (MSVC).
- **Tauri**: `cargo install tauri-cli` (optional).

## Project Structure

- `crates/core`: Protocol definitions (`Frame`).
- `crates/transport`: QUIC networking layer (`quinn`).
- `crates/input`: Input capture and injection (`windows` API).
- `crates/clipboard`: Clipboard synchronization (`Win32` hidden window).
- `crates/session`: **Core Application Logic** (State machine, Events, Commands).
- `apps/cli`: CLI tool for headless testing.
- `apps/desktop`: Tauri GUI Application.

## Architecture

logic is centralized in `crates/session`.
- **Session Loops** (`run_client_session`/`run_server_session`) handle QUIC traffic, Input events, and SessionCommands simultaneously via `tokio::select!`.
- **Tauri Integration**: The desktop app injects commands (like `SendFile`) into the running session loop using async channels.

## Running the App

### Desktop GUI
```bash
cd apps/desktop/src-tauri
cargo tauri dev
```
1. **Server**: Start Server.
2. **Client**: Connect to Server IP.
3. **File Transfer**: While connected, click "Send File" button to pick a file. Transfer happens in background.

### CLI
```bash
cargo run -p platform-passer-cli -- server
cargo run -p platform-passer-cli -- client --server 127.0.0.1:4433
```
*(CLI also supports one-off file send via `--send-file`)*

## Status
- **Protocol**: Complete (Input, Clipboard, Files).
- **Core Logic**: Centralized in `session` crate.
- **GUI**: Functional with Input Sharing, Clipboard Sync, and File Transfer.
