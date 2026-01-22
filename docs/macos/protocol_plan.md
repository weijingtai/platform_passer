# Protocol Design & Plan (v1.0)

This document defines the communication protocol between the **Windows** and **macOS** nodes for the Platform Passer application.

## 1. Transport Layer (QUIC)

### ALPN (Application-Layer Protocol Negotiation)
Both client and server **MUST** use the following identifier:
- **ID**: `pp/1`
- **Hex**: `70 70 2f 31`

### Connection Stability
To prevent idle disconnections, the following QUIC parameters are enforced:
- **Keep-Alive Interval**: 10 seconds.
- **Max Idle Timeout**: 60 seconds.
- **Handshake Timeout**: 20 seconds.

---

## 2. Framing & Serialization

### Serialization
All frames are serialized using **Bincode** (standard configuration).

### Frame Structure
The protocol uses a TLV-like structure (Tag-Length-Value) wrapped by Bincode. The top-level container is the `Frame` enum.

| Variant | Description | Data Type |
| :--- | :--- | :--- |
| `Handshake` | Initial connection metadata | `HandshakeInfo` |
| `Heartbeat` | Periodic connectivity check | `()` |
| `Input` | Mouse/Keyboard events | `InputEvent` |
| `Clipboard` | Text/Image data updates | `ClipboardEvent` |
| `FileTransferRequest` | Metadata for file upload | `FileTransferRequest` |
| `FileTransferResponse` | Acceptance/Rejection of file | `FileTransferResponse` |

---

## 3. Control Flow (Input Injection)

The application uses a **Server-Controlled** architecture:
- **Server (Source)**: Captures local user input (mouse move, clicks, keys) and broadcasts them.
- **Client (Sink)**: Receives input frames and injects them into the local OS.

### Input Coordinates
- **Normalized Coordinates**: Mouse positions are sent as normalized values (`0.0` to `1.0`) to account for different screen resolutions between macOS and Windows.

---

## 4. Stream Management

### Main Protocol Stream (Bi-directional)
- Opened by the **Client** immediately after handshake.
- Used for: Heartbeats, Input Events, Clipboard synchronization, and File Transfer negotiation.

### Bulk Data Stream (Uni-directional)
- Opened dynamically for large payloads (e.g., file chunks).
- **Strategy**: One stream per file delivery to maximize QUIC's parallel delivery and avoid Head-of-Line blocking.

---

## 5. Security
- **TLS**: QUIC mandatory TLS 1.3.
- **Certificates**: Currently using self-signed certificates with **Server Verification Skip** on the client side for development ease. Production will require a more robust PKI.
