# Update: Reconnection Logic & GUI Styles

## 1. Robust Reconnection Logic
Addressed an issue where the application would not reconnect if a peer disconnected abruptly (e.g., power loss, network drop) without sending a proper close frame.

### Technical Detail
-   **Heartbeat Watchdog**: Implemented a 15-second timeout on both Server and Client read loops.
-   **Server**: If no heartbeat is received from the Client for 15 seconds, the Server assumes the connection is dead, closes the socket, and returns to the `Waiting` state.
-   **Client**: If no heartbeat echo is received from the Server for 15 seconds, the Client assumes the connection is dead, closes the socket, and transitions to the `Reconnecting` state.

## 2. GUI Status Colors
Updated the visual indicators in the macOS/Windows GUI to be more consistent with user expectations.

-   **Connected**: Text and Dot are now explicit **Green** (`#10b981`).
-   **Waiting**: Text and Dot are **Yellow**.
-   **Connecting**: Text and Dot are **Orange**.
-   **Reconnecting**: Text and Dot are **Dark Orange**.
-   **Error**: Text and Dot are **Red**.
