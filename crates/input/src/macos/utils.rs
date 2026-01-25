use platform_passer_core::config::ScreenInfo;
use core_graphics::display::{CGMainDisplayID, CGDisplayBounds, CGDisplayPixelsWide};

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

/// Fetches the main display's information (resolution and DPI).
pub fn get_screen_info() -> Option<ScreenInfo> {
    unsafe {
        let display_id = CGMainDisplayID();
        let bounds = CGDisplayBounds(display_id);
        
        let width_points = bounds.size.width as u32;
        let height_points = bounds.size.height as u32;
        
        // Physical pixels (Raw/Native resolution)
        let width_pixels = CGDisplayPixelsWide(display_id) as u32;
        
        // Calculate backing scale factor (DPI)
        let dpi_scale = if width_points > 0 {
            width_pixels as f32 / width_points as f32
        } else {
            1.0
        };
        
        // Use 1.0 minimum to avoid div by zero issues downstream
        let final_scale = if dpi_scale < 1.0 { 1.0 } else { dpi_scale };

        Some(ScreenInfo {
            width: width_points, // We report logical points for cursor mapping
            height: height_points,
            dpi_scale: final_scale,
        })
    }
}
