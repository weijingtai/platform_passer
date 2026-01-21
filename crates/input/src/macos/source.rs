use crate::InputSource;
use anyhow::{Result, anyhow};
use core_graphics::event::{CGEvent, CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement, CGEventType};
use core_graphics::event::{CGEventField, EventField};
use platform_passer_core::InputEvent;
use std::sync::{Arc, Mutex};
use std::thread;

type HookCallback = Box<dyn Fn(InputEvent) + Send + Sync>;
static GLOBAL_CALLBACK: Mutex<Option<Arc<HookCallback>>> = Mutex::new(None);

pub struct MacosInputSource;

impl MacosInputSource {
    pub fn new() -> Self {
        Self
    }
}

extern "C" fn callback(
    _proxy: core_graphics::event::CGEventTapProxy,
    etype: core_graphics::event::CGEventType,
    event: &core_graphics::event::CGEvent,
    _user_info: *const libc::c_void,
) -> core_graphics::event::CGEvent {
    let mut input_event = None;

    match etype {
        CGEventType::MouseMoved => {
            let point = event.location();
            input_event = Some(InputEvent::MouseMove {
                x: point.x as f32,
                y: point.y as f32,
            });
        }
        CGEventType::KeyDown | CGEventType::KeyUp => {
            let key_code = event.get_integer_value_field(CGEventField::KeyboardEventKeycode);
            input_event = Some(InputEvent::Keyboard {
                key_code: key_code as u32,
                is_down: etype == CGEventType::KeyDown,
            });
        }
        _ => {}
    }

    if let Some(ev) = input_event {
        if let Ok(guard) = GLOBAL_CALLBACK.lock() {
            if let Some(cb) = &*guard {
                cb(ev);
            }
        }
    }

    // Return the event to continue propagation
    event.clone()
}

impl InputSource for MacosInputSource {
    fn start_capture(&self, callback_fn: Box<dyn Fn(InputEvent) + Send + Sync>) -> Result<()> {
        {
            let mut guard = GLOBAL_CALLBACK.lock().unwrap();
            *guard = Some(Arc::new(callback_fn));
        }

        thread::spawn(|| {
            let tap = CGEventTap::new(
                CGEventTapLocation::HID,
                CGEventTapPlacement::HeadInsertEventTap,
                CGEventTapOptions::Default,
                vec![
                    CGEventType::MouseMoved,
                    CGEventType::KeyDown,
                    CGEventType::KeyUp,
                ],
                callback,
            ).map_err(|_| anyhow!("Failed to create event tap"))?;

            let loop_source = tap.mach_port().create_runloop_source(0).map_err(|_| anyhow!("Failed to create runloop source"))?;
            
            unsafe {
                let current_loop = cocoa::appkit::NSApp(); // This is a bit of a hack, but fine for now
                // Actually we need the CFRunLoop
                let run_loop = core_foundation::runloop::CFRunLoop::get_current();
                run_loop.add_source(&loop_source, core_foundation::runloop::kCFRunLoopCommonModes);
                tap.enable();
                core_foundation::runloop::CFRunLoop::run_current();
            }

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    fn stop_capture(&self) -> Result<()> {
        // Implementation for stopping the runloop and disabling the tap would go here.
        // For MVP, we can just clear the callback.
        if let Ok(mut guard) = GLOBAL_CALLBACK.lock() {
            *guard = None;
        }
        Ok(())
    }
}
