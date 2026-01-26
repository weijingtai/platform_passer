use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum Frame {
    Handshake(Handshake),
    Heartbeat(Heartbeat),
    Input(InputEvent),
    Clipboard(ClipboardEvent),
    FileTransferRequest(FileTransferRequest),
    FileTransferResponse(FileTransferResponse),
    FileData { id: u32, chunk: Vec<u8> },
    FileEnd { id: u32 },
    ScreenSwitch(ScreenSide),
    Notification { title: String, message: String },
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
    pub purpose: TransferPurpose,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub enum TransferPurpose {
    Manual,
    ClipboardSync { batch_id: u64 },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FileTransferResponse {
    pub id: u32,
    pub accepted: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum ClipboardEvent {
    Text(String),
    Image { data: Vec<u8> }, // PNG encoded
    Files { manifest: FileManifest },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FileManifest {
    pub files: Vec<FileMeta>,
    pub total_size: u64,
    pub batch_id: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FileMeta {
    pub name: String,
    pub size: u64,
}

use crate::config::ScreenInfo;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Handshake {
    pub version: u32,
    pub client_id: String,
    pub capabilities: Vec<String>,
    pub screen_info: Option<ScreenInfo>,
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
