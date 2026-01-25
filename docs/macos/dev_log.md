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

### [2026-01-24] Bidirectional Clipboard Synchronization
- **Server Clipboard Monitoring**:
    - Identified that `server.rs` lacked a clipboard listener, preventing macOS (as server) from sending its clipboard to Windows (as client).
    - Planned refactor of the server's outbound channel to broadcast `Frame` instead of just `InputEvent`.
    - Implementing loop protection using `last_remote_clip` to prevent synchronization loops.
- **Protocol Refinement**:
    - Moving towards a unified event distribution system where all local events (input, clipboard) are wrapped in `Frame` before being broadcasted to connected sessions.

### [2026-01-23] Core Stability: Focus Protection & Permission UX
- **Advanced Focus Protection (Anti-Leakage)**:
    - **Landing Zone Cooling**: Implemented a 300ms "protected period" upon returning to Local mode. During this time, mouse moves are permitted for positioning, but all other inputs (clicks, keys) are swallowed to prevent accidental focus-grabbing in macOS.
    - **Button Latching**: Implemented a physical button state mask. A transition to Local mode is not finalized until all mouse buttons that were pressed during Remote mode are physically released, preventing "ghost clicks" from leaking to macOS windows.
- **Proactive Permission Guidance**:
    - **Active Dialogs**: If Accessibility permissions are missing, the app now uses `AXIsProcessTrustedWithOptions` to programmatically trigger the system Privacy dialog.
    - **Deep Linking**: Enhanced `permissions.rs` with logic to open System Settings directly to the Accessibility and Input Monitoring panes (`x-apple.systempreferences`).
- **Protocol Observability & File Recovery**:
    - **Granular Tracing**: Standardized `log_debug!/log_error!` across both `server.rs` and `client.rs`. Every frame type (Heartbeat, Clipboard, FileTransfer) is now explicitly traced during transmission and reception to improve diagnostic capabilities.
    - **File Receiver Restoration**: Restored the server-side uni-directional stream handler in `server.rs`, which was unintentionally removed, enabling cross-platform file reception into the `downloads/` directory.
- **Session Reliability (EOF Race Fix)**:
    - Added explicit focus reset (`set_remote(false)`) in the server's session loop upon connection termination (including `Unexpected EOF`). This prevents the server from becoming stuck in a "swallowing" state if the client disconnects abruptly.
- **Multi-Monitor Coordinate Normalization**:
    - Implemented a workspace-wide bounding box calculation (`get_display_bounds`) that covers all active monitors. Coordinates are now normalized against the entire workspace rather than just the main display.
- **Performance**:
    - Integrated a `DISPLAY_CACHE` with a refresh strategy to avoid redundant FFI calls during every mouse movement tick.
- **Hysteresis & Conflict Resolution (Ping-Pong Fix)**:
    - **Directional Logic**: Fixed a loop where entering Windows (Left) from macOS (Right) at `x=1.0` would immediately trigger a return logic at `x >= 0.995`. Now uses `x=0.99` for entry and `x=0.998` for return.
    - **State Synchronization**: Modified `handle_event` to use a mutable local state that updates mid-function, ensuring coordinate routing (`abs` vs `virtual`) is consistent with the latest switch result.
- **Critical Stability**:
    - **Notification Disablement**: Permanently disabled `osascript` spawning from within event callbacks. Spawning processes from high-frequency FFI hooks causes process aborts on macOS, explaining the "error 0" disconnects.

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
