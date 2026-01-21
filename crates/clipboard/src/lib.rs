pub mod traits;

#[cfg(target_os = "windows")]
pub mod windows;

#[cfg(target_os = "macos")]
pub mod macos;

pub use traits::*;

#[cfg(target_os = "windows")]
pub type DefaultClipboard = windows::WindowsClipboard;

#[cfg(target_os = "macos")]
pub type DefaultClipboard = macos::MacosClipboard;
#[cfg(target_os = "windows")]
pub use windows::*;
