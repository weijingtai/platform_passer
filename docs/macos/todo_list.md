# macOS Developer Task List

This document outlines the tasks for the macOS developer to ensure parity with the Windows implementation.

## 🎯 Current Iteration Goal: "First Handshake"
**Objective**: Successfully control a Windows machine from a Mac (and vice versa) with correct mouse movement.

---

## 📋 Tasks

### 1. Networking & Compilation <!-- id: mac_comp -->
- [ ] **Fix Session Compilation**: 
    - Update `crates/session` to use abstract traits instead of hardcoded `WindowsInputSource`.
    - Ensure `cargo build` passes on macOS.

### 2. Input Refinement <!-- id: mac_input -->
- [ ] **Coordinate Normalization**:
    - Transform `CGEvent` coordinates to normalized `(0.0 - 1.0)` values before sending.
    - Implement the inverse for `MacosInputSink` (Normalized -> Screen pixels).
- [ ] **CGEventTap Reliability**:
    - Implement auto-reenable logic if the OS disables the tap.

### 3. Permissions & UX <!-- id: mac_perms -->
- [ ] **Accessibility Check**:
    - Create a utility to check if `AXIsProcessTrusted()` is true.
    - Provide a CLI or UI prompt to guide users to System Settings.

### 4. Clipboard <!-- id: mac_clip -->
- [ ] **NSPasteboard Implementation**:
    - Create `MacosClipboard` implementing the `ClipboardProvider` trait.
    - Support basic text `NSPasteboardTypeString`.

---

## 🤝 Synchronization Points
- **Next Sync**: Coordinate mapping logic review.
- **Protocol Version**: 0.1.0
