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
pub fn ensure_accessibility_permissions() -> bool {
    unsafe {
        let key = CFString::from_static_string("kAXTrustedCheckOptionPrompt");
        let value = CFBoolean::true_value();
        let options = CFDictionary::from_CFType_pairs(&[(key.as_CFType(), value.as_CFType())]);
        
        AXIsProcessTrustedWithOptions(options.as_concrete_TypeRef() as *const _)
    }
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
