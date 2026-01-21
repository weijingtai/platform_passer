# Windows Implementation Development Log

This document tracks Windows-side progress and architectural decisions to sync with the macOS team.

## Current Progress (M1 Iteration)

- [x] **Input Handling**: 
    - `WindowsInputSource`: Global hook with **Normalized Coordinate (0.0-1.0)** capture.
    - `WindowsInputSink`: Event injection with **Normalized Coordinate** support.
- [x] **Clipboard**:
    - `WindowsClipboard`: Basic text synchronization.
- [x] **Session Logic Refactor**:
    - `session/client.rs` and `session/server.rs` now use platform-agnostic traits and conditional compilation. 
    - This allows the codebase to **compile on macOS**.

## Architectural Notes (For macOS Sync)

1. **Coordinate System**:
   - Windows uses pixel coordinates. I'm planning to move towards **Normalized Coordinates (0.0 to 1.0)** in `crates/core` to avoid DPI and resolution issues during the next sync.
2. **Session Logic**:
   - Currently, `session/client.rs` and `session/server.rs` have hardcoded Windows types. I will refactor these to use the traits defined in `input/traits.rs` and `clipboard/traits.rs` to allow cross-platform compilation.

## Contact/Lead
- **Lead**: Antigravity (Windows)
