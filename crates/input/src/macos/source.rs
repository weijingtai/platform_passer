use crate::InputSource;
use anyhow::{Result, anyhow};
use platform_passer_core::InputEvent;
use std::sync::Arc;
use std::thread;
use core_foundation::runloop::{CFRunLoop, kCFRunLoopCommonModes};
use core_graphics::event::{CGEvent, CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement, CGEventType};

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventTapEnable(tap: *mut std::ffi::c_void, enable: bool);
}

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

static IS_REMOTE: AtomicBool = AtomicBool::new(false);
static VIRTUAL_CURSOR: Mutex<(f32, f32)> = Mutex::new((0.0, 0.0));
static DISPLAY_CACHE: Mutex<Option<(f32, f32)>> = Mutex::new(None);

pub struct MacosInputSource {
    run_loop: Arc<Mutex<Option<CFRunLoop>>>,
}

impl MacosInputSource {
    pub fn new() -> Self {
        Self {
            run_loop: Arc::new(Mutex::new(None)),
        }
    }

    pub fn set_remote(remote: bool) {
        IS_REMOTE.store(remote, Ordering::SeqCst);
    }
}

// Optimization: Refresh display bounds only when needed, not on every mouse tick.
fn get_display_bounds() -> (f32, f32) {
    if let Ok(guard) = DISPLAY_CACHE.lock() {
        if let Some(bounds) = *guard {
            return bounds;
        }
    }

    unsafe {
        let mut max_width = 0.0;
        let mut max_height = 0.0;
        let mut display_count: u32 = 0;
        let mut displays = [0u32; 16];
        if core_graphics::display::CGGetActiveDisplayList(16, displays.as_mut_ptr(), &mut display_count) == 0 {
            for i in 0..display_count {
                let bounds = core_graphics::display::CGDisplayBounds(displays[i as usize]);
                let right = bounds.origin.x + bounds.size.width;
                let bottom = bounds.origin.y + bounds.size.height;
                if right > max_width { max_width = right; }
                if bottom > max_height { max_height = bottom; }
            }
        }
        
        if max_width > 1.0 && max_height > 1.0 {
            if let Ok(mut guard) = DISPLAY_CACHE.lock() {
                *guard = Some((max_width as f32, max_height as f32));
            }
            (max_width as f32, max_height as f32)
        } else {
            (1920.0, 1080.0) // Fallback
        }
    }
}

fn show_notification(text: &str) {
    let t = text.to_string();
    thread::spawn(move || {
        let _ = std::process::Command::new("osascript")
            .arg("-e")
            .arg(format!("display notification \"{}\" with title \"Platform Passer\"", t))
            .output();
    });
}

fn handle_event(etype: CGEventType, event: &CGEvent) -> Option<InputEvent> {
    let is_remote = IS_REMOTE.load(Ordering::SeqCst);
    let (max_width, max_height) = get_display_bounds();

    match etype {
        CGEventType::MouseMoved | CGEventType::LeftMouseDragged | CGEventType::RightMouseDragged => {
            let point = event.location();
            
            // Normalize absolute position
            let abs_x = (point.x as f32 / max_width);
            let abs_y = (point.y as f32 / max_height);

            // Decision variables
            let mut check_x = abs_x;

            if is_remote {
                // In Remote mode, the OS cursor is frozen. We must use deltas.
                use core_graphics::event::{kCGMouseEventDeltaX, kCGMouseEventDeltaY};
                let delta_x = event.get_double_value_field(kCGMouseEventDeltaX) as f32;
                let delta_y = event.get_double_value_field(kCGMouseEventDeltaY) as f32;
                
                // Panic-safe lock acquisition
                if let Ok(mut vc) = VIRTUAL_CURSOR.lock() {
                    // Update virtual coords (normalized)
                    vc.0 += delta_x / max_width; 
                    vc.1 += delta_y / max_height;
                    
                    // Clamp
                    if vc.0 < 0.0 { vc.0 = 0.0; }
                    if vc.0 > 1.0 { vc.0 = 1.0; }
                    if vc.1 < 0.0 { vc.1 = 0.0; }
                    if vc.1 > 1.0 { vc.1 = 1.0; }
                    
                    check_x = vc.0;
                }
            } else {
                // Update virtual cursor to match physical when local
                if let Ok(mut vc) = VIRTUAL_CURSOR.lock() {
                    *vc = (abs_x, abs_y);
                }
            }

            // Edge detection for Server -> Client switch (Windows is on LEFT)
            if check_x <= 0.005 && !is_remote {
                IS_REMOTE.store(true, Ordering::SeqCst);
                
                // Initialize Virtual Cursor at the RIGHT edge of Windows (0.999)
                if let Ok(mut vc) = VIRTUAL_CURSOR.lock() {
                    *vc = (0.999, abs_y);
                }
                
                show_notification("Switched to Remote (Windows Left)");
                tracing::info!("InputSource: Switched to Remote Control (Left Edge -> Windows Right Edge)");
                return Some(InputEvent::ScreenSwitch(platform_passer_core::ScreenSide::Remote));
            }
            
            // Edge detection for Client -> Server switch
            // Return when virtual cursor hits the RIGHT edge of Windows
            if check_x >= 0.995 && is_remote {
                IS_REMOTE.store(false, Ordering::SeqCst);
                show_notification("Returned to Local Control");
                tracing::info!("InputSource: Returned to Local Control (Windows Right Edge -> Left Edge)");
                return Some(InputEvent::ScreenSwitch(platform_passer_core::ScreenSide::Local));
            }

            if !is_remote { return None; }

            let mut final_x = abs_x;
            let mut final_y = abs_y;
            
            if is_remote {
                 if let Ok(vc) = VIRTUAL_CURSOR.lock() {
                     final_x = vc.0;
                     final_y = vc.1;
                 }
            }

            Some(InputEvent::MouseMove { x: final_x, y: final_y })
        }
        CGEventType::LeftMouseDown | CGEventType::LeftMouseUp |
        CGEventType::RightMouseDown | CGEventType::RightMouseUp |
        CGEventType::OtherMouseDown | CGEventType::OtherMouseUp => {
            if !is_remote { return None; }
            let button = match etype {
                CGEventType::LeftMouseDown | CGEventType::LeftMouseUp => platform_passer_core::MouseButton::Left,
                CGEventType::RightMouseDown | CGEventType::RightMouseUp => platform_passer_core::MouseButton::Right,
                _ => platform_passer_core::MouseButton::Middle,
            };
            let is_down = matches!(etype, CGEventType::LeftMouseDown | CGEventType::RightMouseDown | CGEventType::OtherMouseDown);
            tracing::info!("InputSource: Mouse Button {:?} {}", button, if is_down { "Down" } else { "Up" });
            Some(InputEvent::MouseButton { button, is_down })
        }
        CGEventType::KeyDown | CGEventType::KeyUp | CGEventType::FlagsChanged => {
            let key_code = event.get_integer_value_field(9); // kCGKeyboardEventKeycode = 9
            
            // Check for hotkey to return to local (e.g., Command + Escape)
            // Allow this even if remote (it's the escape hatch)
            if is_remote && key_code == 53 { // Escape
                 IS_REMOTE.store(false, Ordering::SeqCst);
                 show_notification("Returned to Local Control (Escape)");
                 tracing::info!("InputSource: Returned to Local Control (Escape)");
                 return Some(InputEvent::ScreenSwitch(platform_passer_core::ScreenSide::Local));
            }

            if !is_remote { return None; }

            let is_down = if matches!(etype, CGEventType::FlagsChanged) {
                 // For FlagsChanged, we need to check the flags bitmask
                 let flags = event.get_flags();
                 let is_mod = match key_code {
                     54 | 55 => flags.contains(core_graphics::event::CGEventFlags::CGEventFlagCommand),
                     56 | 60 => flags.contains(core_graphics::event::CGEventFlags::CGEventFlagShift),
                     57 => flags.contains(core_graphics::event::CGEventFlags::CGEventFlagAlphaShift),
                     58 | 61 => flags.contains(core_graphics::event::CGEventFlags::CGEventFlagAlternate),
                     59 | 62 => flags.contains(core_graphics::event::CGEventFlags::CGEventFlagControl),
                     _ => false,
                 };
                 is_mod
            } else {
                 matches!(etype, CGEventType::KeyDown)
            };

            let win_vk = crate::keymap::macos_to_windows_vk(key_code as u32);
            Some(InputEvent::Keyboard {
                key_code: win_vk,
                is_down,
            })
        }
        CGEventType::ScrollWheel => {
            if !is_remote { return None; }
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
        
        // Store the raw pointer to the tap's MachPort so we can re-enable it from within the callback
        let tap_port_ptr: Arc<Mutex<Option<usize>>> = Arc::new(Mutex::new(None));
        let tap_port_ptr_clone = tap_port_ptr.clone();

        let run_loop_shared = self.run_loop.clone();

        thread::spawn(move || {
            let tap = CGEventTap::new(
                CGEventTapLocation::Session,
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
                    CGEventType::FlagsChanged,
                    CGEventType::ScrollWheel,
                ],
                move |proxy, etype, event| {
                    match etype {
                        CGEventType::TapDisabledByTimeout | CGEventType::TapDisabledByUserInput => {
                            println!("WARNING: CGEventTap disabled. Re-enabling...");
                            
                            // Try simpler proxy re-enable first
                            proxy.enable();
                            
                            // Fallback to raw port re-enable if proxy doesn't suffice (redundant but safe)
                            let ptr_opt = tap_port_ptr_clone.lock().unwrap();
                            if let Some(ptr) = *ptr_opt {
                                unsafe {
                                    let port_ref = ptr as *mut std::ffi::c_void;
                                    CGEventTapEnable(port_ref as _, true);
                                }
                            }
                            None
                        }
                        _ => {
                            // Process event logic (extraction, sending to client)
                            if let Some(ev) = handle_event(etype, event) {
                                callback_arc(ev);
                            }
                            
                            // Input Swallowing / Suppression Logic
                            let is_remote = IS_REMOTE.load(Ordering::SeqCst);
                            
                            if is_remote {
                                // Swallow event locally
                                None
                            } else {
                                // Local mode: Pass event through to OS
                                Some(event.to_owned())
                            }
                        }
                    }
                },
            ).map_err(|e| anyhow!("Failed to create event tap: {:?}", e))?;

            // Store the MachPort pointer
            {
                use core_foundation::base::TCFType;
                let mut lock = tap_port_ptr.lock().unwrap();
                *lock = Some(tap.mach_port.as_concrete_TypeRef() as usize);
            }

            let loop_source = tap.mach_port.create_runloop_source(0).map_err(|_| anyhow!("Failed to create runloop source"))?;
            
            unsafe {
                let run_loop = CFRunLoop::get_current();
                
                if let Ok(mut rl) = run_loop_shared.lock() {
                    *rl = Some(run_loop.clone());
                }

                run_loop.add_source(&loop_source, kCFRunLoopCommonModes);
                tap.enable();
                CFRunLoop::run_current();
            }

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    fn stop_capture(&self) -> Result<()> {
        if let Ok(mut rl_lock) = self.run_loop.lock() {
            if let Some(rl) = rl_lock.take() {
                rl.stop();
                tracing::info!("InputSource: Capture stopped, run loop terminated.");
            }
        }
        Ok(())
    }
}
