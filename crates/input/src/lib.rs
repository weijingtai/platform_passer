pub mod traits;
pub mod keymap;

#[cfg(target_os = "windows")]
pub mod windows;

#[cfg(target_os = "macos")]
pub mod macos;

pub use traits::*;

#[cfg(target_os = "windows")]
pub use windows::*;

#[cfg(target_os = "macos")]
pub use macos::*;

#[cfg(target_os = "windows")]
pub type DefaultInputSource = WindowsInputSource;
#[cfg(target_os = "windows")]
pub type DefaultInputSink = WindowsInputSink;

#[cfg(target_os = "macos")]
pub type DefaultInputSource = MacosInputSource;
#[cfg(target_os = "macos")]
pub type DefaultInputSink = MacosInputSink;
