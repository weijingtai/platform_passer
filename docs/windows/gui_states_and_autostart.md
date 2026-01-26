# GUI Enhancement: Connection States, Single Instance & Auto-Start

## New Features

### 1. Detailed Connection States
The GUI now provides granular feedback on the connection status using a state machine:
- **Server**: 
    - `Waiting`: Server is listening for incoming connections (Yellow indicator).
    - `Connecting`: A client has initiated a handshake (Orange indicator).
    - `Connected`: Session is active (Green indicator).
- **Client**: 
    - `Connecting`: Attempting to reach the server (Orange indicator).
    - `Connected`: Session is active (Green indicator).
    - `Reconnecting`: Automatically trying to recover from a dropped connection (Pulsing Red/Orange indicator).

### 2. Single Instance Protection
- Added `tauri-plugin-single-instance` to ensure only one instance of **Platform Passer** runs at a time.
- If a second instance is launched, it will automatically focus the existing window and exit.

### 3. Auto-Start Logic
- The application now remembers its last state and attempts to resume it on launch:
    - If the last mode was **Server**, it starts the server automatically.
    - If the last mode was **Client** and the saved IP was **not** a local address (localhost/127.0.0.1), it attempts to connect automatically.

## Technical Implementation
- **Backend**: Updated `SessionEvent` enum in `crates/session` and forwarded new events to Tauri.
- **Frontend**: Updated `index.html` and `style.css` with state-specific logic and animations.
- **Tauri**: Registered the single-instance plugin in `main.rs`.
