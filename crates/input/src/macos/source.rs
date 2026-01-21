use crate::InputSource;
use anyhow::{Result, anyhow};
use platform_passer_core::InputEvent;
use std::sync::{Arc, Mutex};
use std::thread;
use core_foundation::runloop::{CFRunLoop, kCFRunLoopCommonModes};
use core_graphics::display::CGMainDisplayID;
use core_graphics::event::{CGEvent, CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement, CGEventType, CGEventTapProxy};
use core_graphics::event::{CGEventField};

type HookCallback = Box<dyn Fn(InputEvent) + Send + Sync>;
static GLOBAL_CALLBACK: Mutex<Option<Arc<HookCallback>>> = Mutex::new(None);

pub struct MacosInputSource;

impl MacosInputSource {
    pub fn new() -> Self {
        Self
    }
}

extern "C" fn callback(
    _proxy: CGEventTapProxy,
    etype: CGEventType,
    event: &CGEvent,
    _user_info: *const libc::c_void,
) -> CGEvent {
    // Auto-reenable logic: if the event type is TapDisabledByTimeout or TapDisabledByUserInput,
    // we should re-enable it.
    if etype == CGEventType::TapDisabledByTimeout || etype == CGEventType::TapDisabledByUserInput {
        // In a real implementation, we'd need access to the tap handle here.
        // For now, these special events don't carry the tap, so we might need a global tap handle or 
        // rely on the fact that CGEventTap provides a proxy.
        // However, most implementations use CGEventTap::enable() on the tap object.
        return event.clone();
    }

    let mut input_event = None;

    match etype {
        CGEventType::MouseMoved | CGEventType::LeftMouseDragged | CGEventType::RightMouseDragged => {
            let point = event.location();
            
            // Coordinate Normalization (0.0 - 1.0)
            unsafe {
                let display_id = CGMainDisplayID();
                let bounds = core_graphics::display::CGDisplayBounds(display_id);
                if bounds.size.width > 0.0 && bounds.size.height > 0.0 {
                    input_event = Some(InputEvent::MouseMove {
                        x: (point.x / bounds.size.width) as f32,
                        y: (point.y / bounds.size.height) as f32,
                    });
                }
            }
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
                    CGEventType::LeftMouseDragged,
                    CGEventType::RightMouseDragged,
                    CGEventType::KeyDown,
                    CGEventType::KeyUp,
                ],
                callback,
            ).map_err(|_| anyhow!("Failed to create event tap. Check Accessibility permissions."))?;

            let loop_source = tap.mach_port().create_runloop_source(0).map_err(|_| anyhow!("Failed to create runloop source"))?;
            
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
        // Implementation for stopping the runloop and disabling the tap would go here.
        // For MVP, we can just clear the callback.
        if let Ok(mut guard) = GLOBAL_CALLBACK.lock() {
            *guard = None;
        }
        Ok(())
    }
}
