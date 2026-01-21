use crate::InputSource;
use anyhow::{Result, anyhow};
use platform_passer_core::InputEvent;
use std::sync::Arc;
use std::thread;
use core_foundation::runloop::{CFRunLoop, kCFRunLoopCommonModes};
use core_graphics::display::CGMainDisplayID;
use core_graphics::event::{CGEvent, CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement, CGEventType};

pub struct MacosInputSource;

impl MacosInputSource {
    pub fn new() -> Self {
        Self
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
                    Some(InputEvent::MouseMove {
                        x: (point.x / bounds.size.width) as f32,
                        y: (point.y / bounds.size.height) as f32,
                    })
                } else {
                    None
                }
            }
        }
        CGEventType::KeyDown | CGEventType::KeyUp => {
            let key_code = event.get_integer_value_field(9); // kCGKeyboardEventKeycode = 9
            Some(InputEvent::Keyboard {
                key_code: key_code as u32,
                is_down: matches!(etype, CGEventType::KeyDown),
            })
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
                    CGEventType::KeyDown,
                    CGEventType::KeyUp,
                ],
                move |_proxy, etype, event| {
                    match etype {
                        CGEventType::TapDisabledByTimeout | CGEventType::TapDisabledByUserInput => {
                            None
                        }
                        _ => {
                            if let Some(ev) = handle_event(etype, event) {
                                callback_arc(ev);
                            }
                            None
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
