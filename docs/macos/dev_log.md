# macOS Implementation Development Log

This document tracks the progress of the macOS platform support for the `platform_passer` project.

## Progress Overview

- **Input Capture**: Completed (Normalized 0.0-1.0, Auto-reenable)
- **Input Injection**: Completed (Normalized restoration)
- **Coordinate Mapping**: Completed (Normalization logic)
- **Permissions Management**: Completed (AXIsProcessTrusted utility)
- **Clipboard**: Completed (NSPasteboard polling implementation)

---

## Synchronization Log

### [2026-01-23] Screen Focus & Input Synchronization
- **Screen Focus Switching (Magic Edge)**:
    - Implemented edge detection in `MacosInputSource` (Server).
    - **Trigger**: Moving mouse to the `Right` edge of the main screen now sets `IS_REMOTE` to true and sends a `ScreenSwitch` event.
    - **Return**: Pressing `Escape` (or detected Left Edge return) sets `IS_REMOTE` to false.
- **Input Swallowing**:
    - `CGEventTap` updated to return `NULL` when `IS_REMOTE` is active, effectively hiding the cursor and preventing unintended actions on the macOS machine.
- **Input Reliability & Mapping**:
    - **Keyboard**: Implemented `macos_to_windows_vk` mapping in `keymap.rs`. macOS keys now translate correctly to Windows Virtual-Key codes (e.g., Backspace).
    - **Mouse Buttons**: Added support for `Left/Right/Middle` mouse button capture in `handle_event`.
- **UI Enhancements**:
    - Integrated a real-time connection status indicator in the desktop GUI.
- **Verification**:
    - Verified connectivity between macOS (Server) and Windows (Client).
    - Confirmed mouse buttons and basic keyboard input are working as expected.

### [2026-01-21] Robust Logging & ALPN Fix
- **Networking Stability**:
    - Resolved `no_application_protocol` (Error 120) by implementing explicit ALPN configuration (`pp/1`) on both client and server transport layers.
    - Fixed compilation errors in `crates/session` related to duplicate dependencies and platform-specific types.
- **Enhanced Observability**:
    - Implemented a structured logging system using `tracing-subscriber` with `EnvFilter`.
    - Configured build-sensitive log levels (Debug for dev, Info for prod).
    - Added deep tracing logs in `transport` and `session` crates to monitor handshakes and stream lifecycle.
- **Verification**:
    - Successfully performed a local loopback test using the CLI tool.
    - Confirmed handshake success, bi-directional stream opening, and clipboard data exchange.

### [2026-01-20] Merged Remote Session Refactor
- **Conflict Resolution**:
    - Merged changes from `origin/main` which introduced session-level abstraction.
    - Resolved conflicts in `crates/session/src/client.rs` and `server.rs` by adopting a unified `Default...` alias approach instead of inline `#[cfg]` blocks.
    - This ensures the session layer remains perfectly platform-agnostic while utilizing macOS native implementations provided by `crates/input` and `crates/clipboard`.
- **Status**: macOS implementation is now fully integrated with the latest shared session logic.

## Pending / To-Do Tasks

### [High Priority]
- [ ] **Manual Verification**: Verify input capture and injection on actual macOS hardware with proper Accessibility permissions.
- [ ] **Permission Detection**: Implement logic to check if Accessibility and Input Monitoring permissions are granted, and provide user guidance if missing.
- [ ] **DPI & Coordinate Mapping**: Handle multiple displays and ensure coordinates are correctly mapped across different resolutions and scaling factors.

### [Medium Priority]
- [ ] **Robustness**: Implement `CGEventTap` auto-reenable logic (macOS can disable taps if the callback is too slow).
- [ ] **Enhanced Clipboard**: Implement sync for rich text, images, and file references (macOS `NSPasteboard`).

### [Low Priority]
- [ ] **File Drag & Drop**: Implement the macOS side of the file dragging protocol.
- [ ] **Performance Profile**: Measure CPU impact of global event tapping and optimize callback processing.
