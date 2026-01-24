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

### [2026-01-23] Stability, Modifiers & Keypad Fixes
- **Connection Stability (QUIC)**:
    - Increased `max_idle_timeout` to 300 seconds and decreased `keep_alive_interval` to 5 seconds to prevent unexpected session drops.
- **Input Sync Expansion**:
    - **Modifiers**: Implemented `FlagsChanged` event capture in `MacosInputSource`. Shift, Option, Control, and Command states are now synchronized in real-time, enabling combination keys (e.g., `Shift + A`).
    - **Keypad**: Fully mapped macOS keyboard keypad sector (codes 65-92) to Windows Virtual Keys.
- **Magic Edge & Isolation Refinement**:
    - **Isolation**: prevents "Ghost Inputs". Modified `handle_event` to return `None` when `!IS_REMOTE` (Local Mode), ensuring macOS inputs are NOT sent to Windows unless explicitly switched.
    - **Tap Location**: Switched from `HID` to `Session`. This allows the application to suppress events (swallow inputs) without requiring Root privileges, strictly adhering to user session boundaries.
    - **Virtual Cursor**: Implemented a Delta-based tracking system in `source.rs`. When in Remote mode (where the OS cursor is frozen/swallowed), `VIRTUAL_CURSOR` accumulates `kCGMouseEventDeltaX/Y` to maintain a logical position for edge detection, ensuring the user can return to the Local screen.
    - **Swallowing**: Confirmed `CGEventTap` swallows local events when `IS_REMOTE` is true, fixing the "Cursor Mirroring" issue where the macOS cursor would move while controlling Windows.
    - **Return**: Pressing `Escape` (or detected Left Edge return) sets `IS_REMOTE` to false.
- **Observability**:
    - Implemented high-priority event forwarding from session loops to the desktop GUI console. Handshakes, pulses, and errors are now visible to the end user.
- **Verification**:
    - Confirmed successful handshake and bi-directional control between macOS (Server) and Windows (Client) using the latest `origin/main`.

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
