use crate::InputSink;
use anyhow::{Result, anyhow};
use core_graphics::event::{CGEvent, CGEventTapLocation, CGEventType, CGMouseButton};
use core_graphics::event_source::CGEventSource;
use core_graphics::geometry::CGPoint;
use foreign_types::ForeignType;
use platform_passer_core::InputEvent;
use std::sync::Mutex;
use std::collections::HashSet;
use platform_passer_core::config::AppConfig;

pub struct MacosInputSink {
    last_pos: Mutex<CGPoint>,
    scroll_multiplier: Mutex<f32>,
    scroll_reverse: Mutex<bool>,
    pressed_keys: Mutex<HashSet<u16>>,
    pressed_buttons: Mutex<HashSet<u32>>,
}

impl MacosInputSink {
    pub fn new() -> Self {
        Self {
            last_pos: Mutex::new(CGPoint::new(0.0, 0.0)),
            scroll_multiplier: Mutex::new(1.0),
            scroll_reverse: Mutex::new(false),
            pressed_keys: Mutex::new(HashSet::new()),
            pressed_buttons: Mutex::new(HashSet::new()),
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
                let mac_keycode = crate::keymap::windows_to_macos_keycode(key_code);
                
                if let Ok(mut keys) = self.pressed_keys.lock() {
                    if is_down {
                        keys.insert(mac_keycode);
                    } else {
                        keys.remove(&mac_keycode);
                    }
                }

                let cg_event = core_graphics::event::CGEvent::new_keyboard_event(
                    source,
                    mac_keycode,
                    is_down,
                ).map_err(|_| anyhow!("Failed to create keyboard event"))?;
                cg_event.post(CGEventTapLocation::HID);
            }
            InputEvent::MouseButton { button, is_down } => {
                let cg_button = match button {
                    platform_passer_core::MouseButton::Left => CGMouseButton::Left,
                    platform_passer_core::MouseButton::Right => CGMouseButton::Right,
                    platform_passer_core::MouseButton::Middle => CGMouseButton::Center,
                };
                
                // Using u32 representation
                let btn_u32 = cg_button as u32;

                if let Ok(mut btns) = self.pressed_buttons.lock() {
                    if is_down {
                        btns.insert(btn_u32);
                    } else {
                        btns.remove(&btn_u32);
                    }
                }

                let etype = if is_down {
                    match cg_button {
                        CGMouseButton::Left => CGEventType::LeftMouseDown,
                        CGMouseButton::Right => CGEventType::RightMouseDown,
                        _ => CGEventType::OtherMouseDown,
                    }
                } else {
                    match cg_button {
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
                    cg_button,
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
                        {
                            let mult = if let Ok(guard) = self.scroll_multiplier.lock() { *guard } else { 1.0 };
                            let reverse = if let Ok(guard) = self.scroll_reverse.lock() { *guard } else { false };
                            let dy_val = if reverse { -dy } else { dy };
                            (dy_val as f32 * mult) as i32
                        },
                        0,
                        0,
                    );
                    if !event_ptr.is_null() {
                        let cg_event = CGEvent::from_ptr(event_ptr as *mut _);
                        cg_event.post(CGEventTapLocation::HID);
                    }
                }
            }
            InputEvent::ScreenSwitch(_) => {
                // Sinks don't handle screen switches directly
            }
        }

        Ok(())
    }

    fn update_config(&self, config: AppConfig) -> Result<()> {
        if let Ok(mut guard) = self.scroll_multiplier.lock() {
            *guard = config.input.scroll_speed_multiplier;
        }
        if let Ok(mut guard) = self.scroll_reverse.lock() {
            *guard = config.input.scroll_reverse;
        }
        Ok(())
    }

    fn reset_input(&self) -> Result<()> {
        let source = CGEventSource::new(core_graphics::event_source::CGEventSourceStateID::Private).map_err(|_| anyhow!("Failed to create event source"))?;

        if let Ok(mut keys) = self.pressed_keys.lock() {
            for key in keys.drain() {
                if let Ok(cg_event) = core_graphics::event::CGEvent::new_keyboard_event(
                    source.clone(),
                    key,
                    false, // is_down = false -> key up
                ) {
                    cg_event.post(CGEventTapLocation::HID);
                }
            }
        }

        if let Ok(mut btns) = self.pressed_buttons.lock() {
            let pos = if let Ok(p) = self.last_pos.lock() { *p } else { CGPoint::new(0.0, 0.0) };
            
            for btn in btns.drain() {
                let btn_cg: CGMouseButton = unsafe { std::mem::transmute(btn) };
                
                let etype = match btn {
                    0 => CGEventType::LeftMouseUp, // Left
                    1 => CGEventType::RightMouseUp, // Right
                    _ => CGEventType::OtherMouseUp,
                };

                if let Ok(cg_event) = core_graphics::event::CGEvent::new_mouse_event(
                    source.clone(),
                    etype,
                    pos,
                    btn_cg,
                ) {
                    cg_event.post(CGEventTapLocation::HID);
                }
            }
        }

        Ok(())
    }
}


pub fn force_release_modifiers() {
    use core_graphics::event::{CGEvent, CGEventTapLocation};
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
    
    if let Ok(source) = CGEventSource::new(CGEventSourceStateID::Private) {
        // macOS Modifier Keycodes:
        // Command: 55, 54 | Shift: 56, 60 | Option: 58, 61 | Control: 59, 62
        let mod_keys = [55, 54, 56, 60, 58, 61, 59, 62];
        for key in mod_keys {
            if let Ok(event) = CGEvent::new_keyboard_event(source.clone(), key, false) {
                event.post(CGEventTapLocation::HID);
            }
        }
    }
}
