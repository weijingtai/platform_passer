# Windows Implementation Development Log

This document tracks Windows-side progress and architectural decisions to sync with the macOS team.

## Current Progress (M1 Iteration)

- [x] **Input Handling**: 
    - `WindowsInputSource`: Global hook for mouse/keyboard capture using `SetWindowsHookEx`.
    - `WindowsInputSink`: Event injection using `SendInput`.
- [x] **Clipboard**:
    - `WindowsClipboard`: Basic text synchronization using a hidden window for notification.
- [x] **Networking**:
    - QUIC implementation via `quinn` is verified on Windows.

## Architectural Notes (For macOS Sync)

1. **Coordinate System**:
   - Windows uses pixel coordinates. I'm planning to move towards **Normalized Coordinates (0.0 to 1.0)** in `crates/core` to avoid DPI and resolution issues during the next sync.
2. **Session Logic**:
   - Currently, `session/client.rs` and `session/server.rs` have hardcoded Windows types. I will refactor these to use the traits defined in `input/traits.rs` and `clipboard/traits.rs` to allow cross-platform compilation.

## Contact/Lead
- **Lead**: Antigravity (Windows)
