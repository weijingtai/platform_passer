# macOS Handover: Graceful Disconnect & UX Sync

This document summarizes the changes made to the Windows/Core implementation that require synchronization or verification on the macOS side.

## 1. Graceful Disconnect
### Changes Made:
- **Core Logic**: `SessionCommand::Disconnect` now triggers an elegant shutdown in `crates/session`.
- **Frontend**: The "Connect" button in `index.html` now toggles to a red "Disconnect" button when a session is active. It calls a new Tauri command `stop_session`.
- **Backend (Windows)**: Added `stop_session` command and a "Disconnect" item to the system tray.

### macOS Sync Requirements:
- **Main Entry (`main.rs`)**:
  - Add `disconnect` menu item to the `TrayIconBuilder`.
  - Handle the `disconnect` event in `on_menu_event` by sending `SessionCommand::Disconnect` to the active session.
  - Register the `stop_session` command in the `invoke_handler`.
- **Verification**:
  - Ensure the macOS build correctly picks up the `.disconnect` CSS class and the button text/onclick toggle in `index.html`.

## 2. GUI Status Indicator
- **Fix**: The GUI now receives a final `Disconnected` event when the backend task finishes (even if it wasn't triggered by a user click).
- **macOS Task**: Ensure the session loop in macOS `main.rs` also emits this final state reset to prevent the GUI from being "stuck" in a connected state.

## 3. Accessibility Permissions
- **Hook Added**: The GUI now has an `accessibility-warning` block and calls `check_accessibility`.
- **macOS Task**: Ensure `platform_passer_input::macos::utils::is_accessibility_trusted()` is correctly wired up to the `check_accessibility` Tauri command.

## 4. Performance
- **Optimization**: We are now using 8ms coalescing for input events and `TCP_NODELAY`.
- **macOS Task**: Verify that the macOS `transport` still builds correctly after the `Cargo.toml` feature cleanup (moved to plain `tokio-tungstenite` for now as TLS wasn't active).
