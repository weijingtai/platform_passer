use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum Frame {
    Handshake(Handshake),
    Heartbeat(Heartbeat),
    Input(InputEvent),
    Clipboard(ClipboardEvent),
    FileTransferRequest(FileTransferRequest),
    FileTransferResponse(FileTransferResponse),
    ScreenSwitch(ScreenSide),
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq)]
pub enum ScreenSide {
    Local,
    Remote,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FileTransferRequest {
    pub id: u32,
    pub filename: String,
    pub file_size: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FileTransferResponse {
    pub id: u32,
    pub accepted: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum ClipboardEvent {
    Text(String),
    // Future: Image, Files
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Handshake {
    pub version: u32,
    pub client_id: String,
    pub capabilities: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Heartbeat {
    pub timestamp: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum InputEvent {
    /// Normalized coordinates from 0.0 to 1.0
    MouseMove { x: f32, y: f32 },
    MouseButton { button: MouseButton, is_down: bool },
    Keyboard { key_code: u32, is_down: bool },
    Scroll { dx: f32, dy: f32 },
    ScreenSwitch(ScreenSide),
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}
