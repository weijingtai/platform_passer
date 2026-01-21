# macOS Implementation Development Log

This document tracks the progress of the macOS platform support for the `platform_passer` project.

## Progress Overview

- **Input Capture**: Completed (Basic)
- **Input Injection**: Completed (Basic)
- **Coordinate Mapping**: Pending
- **Permissions Management**: Pending

---

## Completed Tasks

### [2026-01-20] Core Input Support
- **Infrastructure**:
    - Added macOS-specific dependencies to `crates/input/Cargo.toml` (`core-graphics`, `core-foundation`, `cocoa`).
    - Integrated `macos` module with conditional compilation in `lib.rs`.
- **Capture (`MacosInputSource`)**:
    - Implemented global event capture using `CGEventTap`.
    - Supported events: `MouseMoved`, `KeyDown`, `KeyUp`.
    - Integrated with a background `CFRunLoop` for asynchronous event handling.
- **Injection (`MacosInputSink`)**:
    - Implemented event injection using `CGEventPost`.
    - Supported events: Mouse movement, Mouse buttons (Left/Right/Center), Keyboard keys, and Scroll wheel.

---

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
