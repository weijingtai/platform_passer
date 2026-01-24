# macOS Developer Task List

This document outlines the tasks for the macOS developer to ensure parity with the Windows implementation.

## 🎯 Current Iteration Goal: "Smooth Sync & Stability"
**Objective**: Finalize input synchronization, mapping, and connection stability between macOS and Windows.

---

## 📋 Tasks

### 1. Networking & Compilation <!-- id: mac_comp -->
- [x] **Fix Session Compilation**: 
    - [x] Update `crates/session` to use abstract traits instead of hardcoded `WindowsInputSource`.
    - [x] Ensure `cargo build` passes on macOS.

### 2. Input Refinement <!-- id: mac_input -->
- [x] **Coordinate Normalization**:
    - [x] Transform `CGEvent` coordinates to normalized `(0.0 - 1.0)` values before sending.
- [x] **CGEventTap Reliability**:
    - [x] Implement auto-reenable logic if the OS disables the tap.
- [x] **MouseButton Support**:
    - [x] Capture `Left/Right/Middle` clicks and forward them.
- [ ] **Multi-Monitor Support**:
    - [ ] Handle coordinates across multiple displays (currently only main display).

### 3. Synchronization Logic (Magic Edge) <!-- id: mac_sync -->
- [/] **Screen Focus Switching**:
    - [x] Implement **Right Edge** detection to switch to Windows (Remote).
    - [x] Implement **Left Edge** detection (via `ScreenSwitch::Local`) to return from Windows.
    - [x] Implement **Escape** hotkey as a fail-safe to return focus to macOS locally.
    - [ ] **Hardware Verification**: Verify edge transitions feel "smooth" and don't trigger accidentally.
- [x] **Event Swallowing (Mirroring Fix)**:
    - [x] Ensure `CGEventTap` returns `NULL` when `IS_REMOTE` is true (Remote Mode).
    - [x] **Strict Isolation**: Ensure `CGEventTap` returns `NULL` for outbound events when `!IS_REMOTE` (Local Mode), preventing leakage to Windows.
    - [ ] **Verification**: Move mouse on macOS (Local) and ensure Windows cursor does NOT move.

### 4. Input Mappings <!-- id: mac_map -->
- [/] **Key Mapping Implementation**:
    - [x] Expand `keymap.rs` for standard alphabetic, numeric, and function keys.
    - [x] **Keypad Support**: Map keypad codes (65-92) to Windows equivalents.
    - [x] **Modifier Support**: Capture `FlagsChanged` to support Shift, Ctrl, Alt (Option).
- [ ] **Special Key Handling**:
    - [x] Initial mapping for macOS `Command` to Windows `Win` key.
    - [x] Initial mapping for macOS `Option` to Windows `Alt` key.
    - [ ] **Hardware Verification**: Test "Shift + A", "Cmd + C", etc.

### 5. Performance & Stability <!-- id: mac_perf -->
- [x] **Connection Stability**:
    - [x] Increased QUIC idle timeout to 300s.
    - [x] Tightened keep-alive to 5s.
- [x] **Logging**:
    - [x] Detailed session events forwarded to GUI console.

### 6. Permissions & UX <!-- id: mac_perms -->
- [x] **Accessibility Check**:
    - [x] Create a utility to check if AXIsProcessTrusted() is true.
- [ ] **Visual Feedback**:
    - [ ] Show a native notification or menu bar state change when focus is remote.

### 7. Clipboard <!-- id: mac_clip -->
- [x] **NSPasteboard Implementation**:
    - [x] Create `MacosClipboard` implementing the `ClipboardProvider` trait.
    - [x] Support basic text `NSPasteboardTypeString`.

---

## 🤝 Synchronization Points
- **Next Sync**: Feedback on modifier key reliability and edge detection sensitivity.
- **Protocol Version**: 0.1.1
