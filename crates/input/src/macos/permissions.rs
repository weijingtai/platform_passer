use core_graphics::event::{CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement, CGEventType};
use core_foundation::base::TCFType;
use core_foundation::dictionary::CFDictionary;
use core_foundation::string::CFString;
use core_foundation::boolean::CFBoolean;
use std::process::Command;

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn AXIsProcessTrustedWithOptions(options: *const std::ffi::c_void) -> bool;
}

/// Checks if accessibility permissions are granted.
pub fn check_accessibility_trusted() -> bool {
    unsafe {
        AXIsProcessTrusted()
    }
}

/// Checks if accessibility permissions are granted, and if not, attempts to trigger the system dialog.
/// 
/// Note: This function should ideally be called from the main thread to avoid potential issues
/// with Core Foundation objects, but we use a thread-safe approach here.
pub fn ensure_accessibility_permissions() -> bool {
    // First check without prompting
    if check_accessibility_trusted() {
        return true;
    }
    
    // If not trusted, we need to prompt. This requires main thread execution.
    // For now, just return the status without prompting to avoid crashes.
    // The GUI should handle prompting via the check_accessibility command.
    false
}

/// Checks if input monitoring is likely enabled by attempting to create a HID event tap (which requires it).
pub fn check_input_monitoring_enabled() -> bool {
    // Attempt to create a passive event tap. If it fails, we likely lack permissions.
    let tap = CGEventTap::new(
        CGEventTapLocation::HID, // HID location requires Input Monitoring
        CGEventTapPlacement::HeadInsertEventTap,
        CGEventTapOptions::ListenOnly,
        vec![CGEventType::MouseMoved],
        |_, _, _| None,
    );
    
    tap.is_ok()
}

/// Opens the macOS System Settings at the Accessibility Privacy pane.
pub fn open_system_settings_accessibility() {
    let _ = Command::new("open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
        .spawn();
}

/// Opens the macOS System Settings at the Input Monitoring Privacy pane.
pub fn open_system_settings_input_monitoring() {
    let _ = Command::new("open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent")
        .spawn();
}
