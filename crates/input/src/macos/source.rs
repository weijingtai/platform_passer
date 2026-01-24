use crate::InputSource;
use anyhow::{Result, anyhow};
use platform_passer_core::InputEvent;
use std::sync::Arc;
use std::thread;
use core_foundation::runloop::{CFRunLoop, kCFRunLoopCommonModes};
use core_graphics::display::CGMainDisplayID;
use core_graphics::event::{CGEvent, CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement, CGEventType};

use std::sync::atomic::{AtomicBool, Ordering};

static IS_REMOTE: AtomicBool = AtomicBool::new(false);

pub struct MacosInputSource;

impl MacosInputSource {
    pub fn new() -> Self {
        Self
    }

    pub fn set_remote(remote: bool) {
        IS_REMOTE.store(remote, Ordering::SeqCst);
    }
}


fn handle_event(etype: CGEventType, event: &CGEvent) -> Option<InputEvent> {
    match etype {
        CGEventType::MouseMoved | CGEventType::LeftMouseDragged | CGEventType::RightMouseDragged => {
            let point = event.location();
            unsafe {
                let display_id = CGMainDisplayID();
                let bounds = core_graphics::display::CGDisplayBounds(display_id);
                if bounds.size.width > 0.0 && bounds.size.height > 0.0 {
                    let x = (point.x / bounds.size.width) as f32;
                    let y = (point.y / bounds.size.height) as f32;

                    // Edge detection for Server -> Client switch
                    if x >= 0.999 && !IS_REMOTE.load(Ordering::SeqCst) {
                        IS_REMOTE.store(true, Ordering::SeqCst);
                        return Some(InputEvent::ScreenSwitch(platform_passer_core::ScreenSide::Remote));
                    }
                    
                    // Edge detection for Client -> Server switch
                    if x <= 0.001 && IS_REMOTE.load(Ordering::SeqCst) {
                        IS_REMOTE.store(false, Ordering::SeqCst);
                        return Some(InputEvent::ScreenSwitch(platform_passer_core::ScreenSide::Local));
                    }

                    Some(InputEvent::MouseMove { x, y })
                } else {
                    None
                }
            }
        }
        CGEventType::LeftMouseDown | CGEventType::LeftMouseUp |
        CGEventType::RightMouseDown | CGEventType::RightMouseUp |
        CGEventType::OtherMouseDown | CGEventType::OtherMouseUp => {
            let button = match etype {
                CGEventType::LeftMouseDown | CGEventType::LeftMouseUp => platform_passer_core::MouseButton::Left,
                CGEventType::RightMouseDown | CGEventType::RightMouseUp => platform_passer_core::MouseButton::Right,
                _ => platform_passer_core::MouseButton::Middle,
            };
            let is_down = matches!(etype, CGEventType::LeftMouseDown | CGEventType::RightMouseDown | CGEventType::OtherMouseDown);
            Some(InputEvent::MouseButton { button, is_down })
        }
        CGEventType::KeyDown | CGEventType::KeyUp => {
            let key_code = event.get_integer_value_field(9); // kCGKeyboardEventKeycode = 9
            
            // Check for hotkey to return to local (e.g., Command + Escape)
            // macOS Command key is usually 55
            if IS_REMOTE.load(Ordering::SeqCst) && key_code == 53 { // Escape
                 // We could also check modifiers here, but Escape is a good simple out for now
                 IS_REMOTE.store(false, Ordering::SeqCst);
                 return Some(InputEvent::ScreenSwitch(platform_passer_core::ScreenSide::Local));
            }

            let win_vk = crate::keymap::macos_to_windows_vk(key_code as u32);
            Some(InputEvent::Keyboard {
                key_code: win_vk,
                is_down: matches!(etype, CGEventType::KeyDown),
            })
        }
        CGEventType::ScrollWheel => {
            let dx = event.get_double_value_field(97) as f32; // kCGScrollWheelEventDeltaAxis2
            let dy = event.get_double_value_field(96) as f32; // kCGScrollWheelEventDeltaAxis1
            Some(InputEvent::Scroll { dx, dy })
        }
        _ => None,
    }
}

impl InputSource for MacosInputSource {
    fn start_capture(&self, callback_fn: Box<dyn Fn(InputEvent) + Send + Sync>) -> Result<()> {
        let callback_arc = Arc::new(callback_fn);
        
        thread::spawn(move || {
            let tap = CGEventTap::new(
                CGEventTapLocation::HID,
                CGEventTapPlacement::HeadInsertEventTap,
                CGEventTapOptions::Default,
                vec![
                    CGEventType::MouseMoved,
                    CGEventType::LeftMouseDragged,
                    CGEventType::RightMouseDragged,
                    CGEventType::LeftMouseDown,
                    CGEventType::LeftMouseUp,
                    CGEventType::RightMouseDown,
                    CGEventType::RightMouseUp,
                    CGEventType::OtherMouseDown,
                    CGEventType::OtherMouseUp,
                    CGEventType::KeyDown,
                    CGEventType::KeyUp,
                    CGEventType::ScrollWheel,
                ],
                move |_proxy, etype, event| {
                    match etype {
                        CGEventType::TapDisabledByTimeout | CGEventType::TapDisabledByUserInput => {
                            None
                        }
                        _ => {
                            let is_remote = IS_REMOTE.load(Ordering::SeqCst);
                            if let Some(ev) = handle_event(etype, event) {
                                callback_arc(ev);
                            }
                            
                            if is_remote {
                                // Swallow event locally
                                None
                            } else {
                                // Execute locally
                                Some(event.to_owned())
                            }
                        }
                    }
                },
            ).map_err(|_| anyhow!("Failed to create event tap. Check Accessibility permissions."))?;

            let loop_source = tap.mach_port.create_runloop_source(0).map_err(|_| anyhow!("Failed to create runloop source"))?;
            
            unsafe {
                let run_loop = CFRunLoop::get_current();
                run_loop.add_source(&loop_source, kCFRunLoopCommonModes);
                tap.enable();
                CFRunLoop::run_current();
            }

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    fn stop_capture(&self) -> Result<()> {
        Ok(())
    }
}
