use crate::InputSink;
use anyhow::{Result, anyhow};
use core_graphics::event::{CGEvent, CGEventSource, CGEventTapLocation, CGEventType, CGMouseButton};
use platform_passer_core::InputEvent;

pub struct MacosInputSink;

impl MacosInputSink {
    pub fn new() -> Self {
        Self
    }
}

impl InputSink for MacosInputSink {
    fn inject_event(&self, event: InputEvent) -> Result<()> {
        let source = CGEventSource::new(core_graphics::event::CGEventSourceStateID::Private).map_err(|_| anyhow!("Failed to create event source"))?;

        match event {
            InputEvent::MouseMove { x, y } => {
                let display_id = unsafe { core_graphics::display::CGMainDisplayID() };
                let bounds = unsafe { core_graphics::display::CGDisplayBounds(display_id) };
                
                let cg_event = CGEvent::new_mouse_event(
                    source,
                    CGEventType::MouseMoved,
                    core_graphics::geometry::CGPoint::new(
                        (x as f64) * bounds.size.width,
                        (y as f64) * bounds.size.height,
                    ),
                    CGMouseButton::Left,
                ).map_err(|_| anyhow!("Failed to create mouse move event"))?;
                cg_event.post(CGEventTapLocation::HID);
            }
            InputEvent::Keyboard { key_code, is_down } => {
                let cg_event = CGEvent::new_keyboard_event(
                    source,
                    key_code as u16,
                    is_down,
                ).map_err(|_| anyhow!("Failed to create keyboard event"))?;
                cg_event.post(CGEventTapLocation::HID);
            }
            InputEvent::MouseButton { button_mask, is_down } => {
                // Simplified button mapping
                let button = if button_mask & 1 != 0 {
                    CGMouseButton::Left
                } else if button_mask & 2 != 0 {
                    CGMouseButton::Right
                } else {
                    CGMouseButton::Center
                };

                let etype = if is_down {
                    match button {
                        CGMouseButton::Left => CGEventType::LeftMouseDown,
                        CGMouseButton::Right => CGEventType::RightMouseDown,
                        _ => CGEventType::OtherMouseDown,
                    }
                } else {
                    match button {
                        CGMouseButton::Left => CGEventType::LeftMouseUp,
                        CGMouseButton::Right => CGEventType::RightMouseUp,
                        _ => CGEventType::OtherMouseUp,
                    }
                };

                // We need the current mouse position for button events on macOS
                // For now, we'll use (0,0) or better, get current position if possible.
                // In a real app, we'd track the last known mouse position.
                let cg_event = CGEvent::new_mouse_event(
                    source,
                    etype,
                    core_graphics::geometry::CGPoint::new(0.0, 0.0), // Placeholder
                    button,
                ).map_err(|_| anyhow!("Failed to create mouse button event"))?;
                cg_event.post(CGEventTapLocation::HID);
            }
            InputEvent::Scroll { dx, dy } => {
                let cg_event = CGEvent::new_scroll_event(
                    source,
                    core_graphics::event::CGScrollEventUnit::Line,
                    2, // wheel count
                    dy as i32,
                    dx as i32,
                    0,
                ).map_err(|_| anyhow!("Failed to create scroll event"))?;
                cg_event.post(CGEventTapLocation::HID);
            }
        }

        Ok(())
    }
}
