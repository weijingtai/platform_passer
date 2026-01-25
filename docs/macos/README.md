# macOS Implementation & Tauri v2 Migration Guide

This document outlines the technical details of the macOS support and the migration to Tauri v2.

## Tauri v2 Migration

The project has been successfully migrated to **Tauri v2**. Key changes include:
- **API Realignment**: The frontend now uses `window.__TAURI__.core.invoke` for command calls.
- **Global API**: `withGlobalTauri: true` is enabled in `tauri.conf.json` to support the vanilla JS frontend.
- **Plugins**: Integrated `tauri-plugin-dialog` for file system interactions.
- **Project Structure**: Updated `tauri.conf.json` to the v2 schema and fixed relative crate paths in `Cargo.toml`.

## macOS Support (Iteration 1)

### 1. Input Injection (Sink)
- Uses `CGEventPost` for mouse and keyboard event injection.
- **Coordinate Normalization**: All mouse coordinates are normalized to `0.0 - 1.0` range to support cross-platform scaling.
- **Raw FFI for Scrolling**: Implemented scroll wheel support via direct `CoreGraphics` FFI to overcome library limitations.

### 2. Input Capture (Source)
- Uses `CGEventTap` to capture system-wide events.n
- **Auto-Reenabling**: If the event tap is disabled by system timeout, the application automatically re-enables it.
- **Coordinate Scaling**: Captures local display coordinates and normalizes them for transmission.

### 3. Clipboard Synchronization
- Implemented using `NSPasteboard`.
- Supports text get/set operations with a polling mechanism for change detection.

## Accessibility Permissions

macOS requires **Accessibility Permissions** for both input capture and injection.

### Proactive Check
The application includes a `check_accessibility` command that calls `AXIsProcessTrusted()`.

### User Guidance
The UI displays a persistent warning if permissions are missing:
- Location: `System Settings > Privacy & Security > Accessibility`
- Action: Toggle the switch for `platform-passer-desktop`.

## Troubleshooting

- **Icon Panic**: The Tauri build requires a valid RGBA PNG. If `cargo tauri dev` panics on icon loading, ensure `apps/desktop/src-tauri/icons/icon.png` is a true PNG (not a renamed JPEG).
- **Temporal Dead Zone**: The frontend script is wrapped in a `DOMContentLoaded` listener to ensure the Tauri global object is injected before usage.
