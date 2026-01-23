use crate::InputSink;
use anyhow::{Result, anyhow};
use core_graphics::event::{CGEvent, CGEventTapLocation, CGEventType, CGMouseButton};
use core_graphics::event_source::CGEventSource;
use core_graphics::geometry::CGPoint;
use foreign_types::ForeignType;
use platform_passer_core::InputEvent;
use std::sync::Mutex;

pub struct MacosInputSink {
    last_pos: Mutex<CGPoint>,
}

impl MacosInputSink {
    pub fn new() -> Self {
        Self {
            last_pos: Mutex::new(CGPoint::new(0.0, 0.0)),
        }
    }
}

impl InputSink for MacosInputSink {
    fn inject_event(&self, event: InputEvent) -> Result<()> {
        let source = CGEventSource::new(core_graphics::event_source::CGEventSourceStateID::Private).map_err(|_| anyhow!("Failed to create event source"))?;

        match event {
            InputEvent::MouseMove { x, y } => {
                let display_id = unsafe { core_graphics::display::CGMainDisplayID() };
                let bounds = unsafe { core_graphics::display::CGDisplayBounds(display_id) };
                
                let target_pos = CGPoint::new(
                    (x as f64) * bounds.size.width,
                    (y as f64) * bounds.size.height,
                );

                // Update last known position
                if let Ok(mut pos) = self.last_pos.lock() {
                    *pos = target_pos;
                }

                let cg_event = core_graphics::event::CGEvent::new_mouse_event(
                    source,
                    CGEventType::MouseMoved,
                    target_pos,
                    CGMouseButton::Left,
                ).map_err(|_| anyhow!("Failed to create mouse move event"))?;
                cg_event.post(CGEventTapLocation::HID);
            }
            InputEvent::Keyboard { key_code, is_down } => {
                let cg_event = core_graphics::event::CGEvent::new_keyboard_event(
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

                let pos = if let Ok(p) = self.last_pos.lock() {
                    *p
                } else {
                    CGPoint::new(0.0, 0.0)
                };

                let cg_event = core_graphics::event::CGEvent::new_mouse_event(
                    source,
                    etype,
                    pos,
                    button,
                ).map_err(|_| anyhow!("Failed to create mouse button event"))?;
                cg_event.post(CGEventTapLocation::HID);
            }
            InputEvent::Scroll { dx: _dx, dy } => {
                extern "C" {
                    fn CGEventCreateScrollWheelEvent2(
                        source: *mut std::ffi::c_void,
                        units: u32,
                        wheelCount: u32,
                        wheel1: i32,
                        wheel2: i32,
                        wheel3: i32,
                    ) -> *mut std::ffi::c_void;
                }

                unsafe {
                    let source_ptr: *mut std::ffi::c_void = std::mem::transmute(source);
                    let event_ptr = CGEventCreateScrollWheelEvent2(
                        source_ptr,
                        0, // Pixel units
                        1, // wheel count
                        dy as i32,
                        0,
                        0,
                    );
                    if !event_ptr.is_null() {
                        let cg_event = CGEvent::from_ptr(event_ptr as *mut _);
                        cg_event.post(CGEventTapLocation::HID);
                    }
                }
            }
        }

        Ok(())
    }
}
