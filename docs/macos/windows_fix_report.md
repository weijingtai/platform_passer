# Sync Report: Critical Windows Fixes (Input & UI)

This report details recent critical fixes implemented on the Windows side that are relevant for cross-platform consistency and "lessons learned" for the macOS implementation.

## 1. Invisible Window Interference (UI Z-Order)
- **Problem**: Mouse clicks (but not hovers or right-clicks) were failing on the Windows Desktop.
- **Cause**: An invisible 0x0 window used for clipboard listening was created as a top-level `WS_OVERLAPPEDWINDOW`. Windows hit-testing incorrectly attributed desktop clicks to this "ghost" window.
- **Solution**: Switch to **Message-Only Windows** (`HWND_MESSAGE`).
- **Lesson for macOS**: Ensure any "listener" windows or background observers used by the macOS sink/source are not participating in the window hierarchy or hit-testing (e.g., set activation policy to accessory or use an agent-style process).

## 2. Keyboard "Sticky Keys" on Forceful Termination
- **Problem**: Pressing Ctrl+C in the terminal to kill the app while a session was active left the "Ctrl" key logically pressed in the OS.
- **Cause**: The process died before it could send `KeyUp` events or unhook properly.
- **Solution**: Added a global `ctrlc` signal handler in `main.rs` that explicitly releases all modifier keys (Ctrl, Alt, Shift, Win) using `SendInput`/`keybd_event` before exiting.
- **Lesson for macOS**: macOS should also have a signal handler (SIGINT/SIGTERM) to ensure the `CGEventTap` is disabled and any stuck keys are cleared via `CGEventCreateKeyboardEvent(..., false)`.

## 3. UIPI and Administrator Privileges
- **Observation**: Low-level mouse hooks (`WH_MOUSE_LL`) cannot interact with the Windows Desktop (Explorer) unless the app has equivalent or higher integrity (Administrator).
- **Current Status**: Running as Administrator is now a requirement for the Windows Client.
- **Parallel for macOS**: Remind users that **Accessibility Permissions** are the macOS equivalent and are mandatory for both capture and injection.

## 4. Hook Performance (The "Hot Path")
- **Refinement**: To prevent double-click lag, we now bypass the entire hook logic for non-movement events when in local mode.
```rust
if !is_remote && msg != WM_MOUSEMOVE {
    return CallNextHookEx(MOUSE_HOOK, code, wparam, lparam);
}
```
- **Sync**: Both sides should ensure that "local mode" has zero overhead to avoid interfering with native OS experience.
