# macOS Developer Task List

This document outlines the tasks for the macOS developer to ensure parity with the Windows implementation.

## 🎯 Current Iteration Goal: "First Handshake"
**Objective**: Successfully control a Windows machine from a Mac (and vice versa) with correct mouse movement.

---

## 📋 Tasks

### 1. Networking & Compilation <!-- id: mac_comp -->
- [x] **Fix Session Compilation**: 
    - Update `crates/session` to use abstract traits instead of hardcoded `WindowsInputSource`.
    - Ensure `cargo build` passes on macOS.

### 2. Input Refinement <!-- id: mac_input -->
- [x] **Coordinate Normalization**:
    - Transform `CGEvent` coordinates to normalized `(0.0 - 1.0)` values before sending.
- [x] **CGEventTap Reliability**:
    - Implement auto-reenable logic if the OS disables the tap.
- [ ] **MouseButton Support**:
    - Capture `Left/Right/Middle` clicks and forward them.
- [ ] **Multi-Monitor Support**:
    - Handle coordinates across multiple displays (currently only main display).

### 3. Synchronization Logic <!-- id: mac_sync -->
- [/] **Screen Focus Switching**:
    - [x] Implement **Right Edge** detection to switch to Windows (Remote).
    - [ ] Verify **Left Edge** detection (Client -> Server) works via `InputEvent::ScreenSwitch`.
    - [x] Implement **Escape** hotkey to return focus to macOS locally.
- [/] **Event Swallowing**:
    - [x] Verify `CGEventTap` returns `NULL` when `IS_REMOTE` is true.

### 4. Input Mappings <!-- id: mac_map -->
- [/] **Key Mapping Implementation**:
    - [x] Initial mapping table in `crates/input/src/keymap.rs`.
    - [ ] Expand mapping for all standard keys and modifiers.
- [ ] **Special Key Handling**:
    - [ ] Map macOS `Command` to Windows `Win` key.
    - [ ] Map macOS `Option` to Windows `Alt` key.

### 5. Permissions & UX <!-- id: mac_perms -->
- [x] **Accessibility Check**:
    - Create a utility to check if `AXIsProcessTrusted()` is true.
- [ ] **Visual Feedback**:
    - Show a native notification or menu bar state change when focus is remote.

### 6. Clipboard <!-- id: mac_clip -->
- [x] **NSPasteboard Implementation**:
    - Create `MacosClipboard` implementing the `ClipboardProvider` trait.
    - Support basic text `NSPasteboardTypeString`.

---

## 🤝 Synchronization Points
- **Next Sync**: Coordinate mapping logic review.
- **Protocol Version**: 0.1.0
