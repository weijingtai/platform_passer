# Windows File Clipboard Sync Todo List

This document outlines the tasks required to implement Windows-side support for the file clipboard synchronization feature, matching the current macOS implementation.

## 1. Clipboard Provider Implementation (`crates/clipboard/src/windows.rs`)

- [ ] **Implement `get_files`**:
    - Use Win32 API (`OpenClipboard`, `GetClipboardData` with `CF_HDROP`).
    - Parse the `HDROP` structure using `DragQueryFileW` to get file paths.
    - Return `Result<Option<Vec<String>>>`.
- [ ] **Implement `set_files`**:
    - Use Win32 API (`OpenClipboard`, `EmptyClipboard`).
    - Construct a `DROPFILES` structure in global memory.
    - Append the null-terminated file paths (double-null at the end).
    - Use `SetClipboardData` with `CF_HDROP`.

## 2. Path Handling & Normalization

- [ ] **Verify Path Formats**: Ensure that paths received from macOS (posix-style) are handled correctly if the Windows machine is the receiver, and vice-versa. (The current implementation uses `std::path::PathBuf` which should handle basic normalization, but Win32 API specifically needs `Wide` strings).
- [ ] **Batch Storage**: Ensure the temporary directory logic for `ClipboardSync` purpose works correctly on Windows (`std::env::temp_dir()`).

## 3. Integration Testing

- [ ] **Manual Test**: macOS (Client/Server) <-> Windows (Client/Server).
- [ ] **Edge Cases**:
    - Unicode filenames.
    - Path length limitations (Long paths on Windows).
    - Locked files.

---
*Note: The network protocol and session logic are already updated to support `TransferPurpose::ClipboardSync`.*
