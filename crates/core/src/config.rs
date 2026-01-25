use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AppConfig {
    pub topology: Topology,
    pub input: InputConfig,
    pub clipboard: ClipboardConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            topology: Topology::default(),
            input: InputConfig::default(),
            clipboard: ClipboardConfig::default(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Topology {
    /// Information about the machine running this instance
    pub local: ScreenInfo,
    /// List of known remote peers and their layout
    pub remotes: Vec<RemoteScreen>,
}

impl Default for Topology {
    fn default() -> Self {
        Self {
            local: ScreenInfo::default(),
            remotes: Vec::new(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ScreenInfo {
    pub width: u32,
    pub height: u32,
    pub dpi_scale: f32, // Default 1.0. Retina/HighDPI > 1.0
}

impl Default for ScreenInfo {
    fn default() -> Self {
        Self { width: 1920, height: 1080, dpi_scale: 1.0 }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RemoteScreen {
    pub id: String, // Hostname or IP
    pub position: ScreenPosition,
    pub info: ScreenInfo,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum ScreenPosition {
    Left,
    Right,
    Top,
    Bottom,
    // Absolute { x: i32, y: i32 } could be added later
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct InputConfig {
    pub cursor_speed_multiplier: f32,
    pub scroll_speed_multiplier: f32,
    pub maintain_aspect_ratio: bool,
}

impl Default for InputConfig {
    fn default() -> Self {
        Self {
            cursor_speed_multiplier: 1.0,
            scroll_speed_multiplier: 1.0,
            maintain_aspect_ratio: true,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ClipboardConfig {
    pub sync_enabled: bool,
    pub sync_images: bool, 
}

impl Default for ClipboardConfig {
    fn default() -> Self {
        Self {
            sync_enabled: true,
            sync_images: false, // Default off to save bandwidth/latency
        }
    }
}
