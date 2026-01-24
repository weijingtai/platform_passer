
use core_graphics::event::{CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement, CGEventType};

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    pub fn AXIsProcessTrusted() -> bool;
}

pub fn check_accessibility_trusted() -> bool {
    unsafe {
        AXIsProcessTrusted()
    }
}

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
