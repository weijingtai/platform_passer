/// Checks if the current process has accessibility permissions.
pub fn is_accessibility_trusted() -> bool {
    unsafe {
        extern "C" {
            fn AXIsProcessTrusted() -> bool;
        }
        AXIsProcessTrusted()
    }
}

/// Provides a hint to the user on how to enable permissions.
pub fn get_accessibility_guidance() -> &'static str {
    "Please enable Accessibility permissions for this app in 'System Settings > Privacy & Security > Accessibility'."
}
