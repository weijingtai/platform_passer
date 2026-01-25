# macOS Development Plan - Phase 2: UX & Topology

This document outlines the specific implementation steps required on macOS to support the new UX, Topology, and Clipboard features.

## 1. Screen Resolution & DPI Handling

**Objective**: Accurately report macOS screen dimensions and DPI scaling to the peer to ensure smooth cursor mapping.

### Implementation Details
-   **Fetch Screen Info**:
    -   Use `NSScreen.screens` to iterate available displays.
    -   Key properties:
        -   `frame`: Global coordinates (note: macOS uses bottom-left origin, might need conversion if system expects top-left).
        -   `backingScaleFactor`: The DPI scale (Retina = 2.0).
    -   **Action**: Implement `platform_passer_input::macos::utils::get_screen_info() -> Vec<ScreenInfo>`.

-   **Coordinate Normalization (Update)**:
    -   Ensure `macos/source.rs` correctly normalizes coordinates based on the *current* screen the mouse is on, not just the main screen.
    -   Handle multiple monitors: Coordinate systems range from `(0.0, 0.0)` top-left of the *virtual desktop* to `(1.0, 1.0)`?
    -   *Correction*: The current protocol uses per-monitor normalized coords or global?
    -   **Plan**: Stick to **Global Normalized** if possible, or send `ScreenID`. For now, ensure normalization respects the union of all frames.

## 2. Cursor Speed & Input Transformation

**Objective**: Apply speed multipliers and sensitivity adjustments.

### Implementation Details
-   **Input Injection (`sink.rs`)**:
    -   Verify `CGEventCreateMouseEvent` behavior with fractional coordinates.
    -   If the shared `InputTransformer` logic is in `crates/session`, macOS just needs to ensure the `ScreenInfo` it provides is accurate.
-   **Input Capture (`source.rs`)**:
    -   No specific change needed *if* we send normalized coordinates.
    -   *Optimization*: If "Speed Multiplier" is implemented on the *sender* side, we might need to artificially scale deltas before normalizing?
    -   **Decision**: Implement speed scaling on the **Receiver** side (Sink) to allow user local preference.

## 3. Clipboard Optimization (Images)

**Objective**: Support Copy/Paste of images between devices.

### Implementation Details
-   **Read from Clipboard (`NSPasteboard`)**:
    -   Check for types: `NSPasteboardTypePNG`, `NSPasteboardTypeTIFF`.
    -   Prioritize PNG for web compatibility.
    -   **Action**: Update `MacosClipboard::get_image() -> Option<Vec<u8>>`.
        -   Use `NSPasteboard.dataForType`.
-   **Write to Clipboard**:
    -   **Action**: Update `MacosClipboard::set_image(data: Vec<u8>)`.
    -   Create `NSPasteboardItem`, set data for type PNG.
-   **Debouncing**:
    -   Implement the logic in `crates/session` (shared), generally requires no macOS-specific logic, but verify `NSPasteboard.changeCount` reliability.

## 4. Topology & GUI

**Objective**: Ensure the Tauri-based Settings UI works on macOS.

### Implementation Details
-   **Window Management**:
    -   Verify Tauri window transparency and specific styling on macOS (vibrancy).
-   **File Storage**:
    -   Ensure `save_config` writes to `~/Library/Application Support/com.platform-passer.dev/`.
-   **Edge Detection (Daemon)**:
    -   The topology config will determine *which* edge triggers the switch.
    -   **Action**: Update `source.rs` to respect the dynamic `Topology` config (e.g., "Switch to Remote is on TOP" instead of always Right).
    -   Listen for configuration updates (via channel or shared state).


## 5. Notifications & Background (Phase 3)

**Objective**: Provide feedback and background persistence as requested.

### Implementation Details
- **Notifications**:
    - Use `tauri-plugin-notification`.
    - **macOS Requirement**: `NSUserNotificationAlertStyle` is determined by system settings. 
    - Permission request logic handled by Tauri.
- **Background Mode (Tray)**:
    - Use `tauri-plugin-system-tray`.
    - **App Bundle**: Ensure `Info.plist` is correct if we want to support "LSUIElement" (Tray Only) mode in the future.
    - **Behavior**:
        - On `Cmd+W` or Close button: Hide window, do NOT quit.
        - Add Tray Menu: "Open Platform Passer", "Quit".

