use crate::InputSource;
use anyhow::{Result, anyhow};
use platform_passer_core::InputEvent;
use std::sync::Arc;
use std::thread;
use core_foundation::runloop::{CFRunLoop, kCFRunLoopCommonModes};
use core_graphics::event::{CGEvent, CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement, CGEventType};

use std::sync::atomic::{AtomicBool, Ordering};

use std::sync::Mutex;

static IS_REMOTE: AtomicBool = AtomicBool::new(false);
static VIRTUAL_CURSOR: Mutex<(f32, f32)> = Mutex::new((0.0, 0.0));

pub struct MacosInputSource;

impl MacosInputSource {
    pub fn new() -> Self {
        Self
    }

    pub fn set_remote(remote: bool) {
        IS_REMOTE.store(remote, Ordering::SeqCst);
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

    match etype {
        CGEventType::MouseMoved | CGEventType::LeftMouseDragged | CGEventType::RightMouseDragged => {
            let point = event.location();
            unsafe {
                // Multi-monitor coordinate calculation
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

                if max_width > 0.0 && max_height > 0.0 {
                    // Normalize absolute position
                    let abs_x = (point.x / max_width) as f32;
                    let abs_y = (point.y / max_height) as f32;

                    // Decision variables
                    let mut check_x = abs_x;
                    // let mut check_y = abs_y; // Unused for now

                    if is_remote {
                        // In Remote mode, the OS cursor is frozen. We must use deltas to update our virtual cursor.
                        let delta_x = event.get_double_value_field(kCGMouseEventDeltaX) as f32;
                        // let delta_y = event.get_double_value_field(kCGMouseEventDeltaY) as f32; 
                        
                        let mut vc = VIRTUAL_CURSOR.lock().unwrap();
                        
                        // Update virtual X (normalized)
                        // We scale delta by max_width to get normalized delta
                        vc.0 += delta_x / max_width; 
                        
                        // Clamp
                        if vc.0 < 0.0 { vc.0 = 0.0; }
                        if vc.0 > 1.0 { vc.0 = 1.0; }
                        
                        check_x = vc.0;
                    } else {
                        // Update virtual cursor to match physical when local, so it's ready for the switch
                        if let Ok(mut vc) = VIRTUAL_CURSOR.lock() {
                            *vc = (abs_x, abs_y);
                        }
                    }

                    // Edge detection for Server -> Client switch
                    if check_x >= 0.995 && !is_remote {
                        IS_REMOTE.store(true, Ordering::SeqCst);
                        show_notification("Switched to Remote Control"); // Keep notification
                        return Some(InputEvent::ScreenSwitch(platform_passer_core::ScreenSide::Remote));
                    }
                    
                    // Edge detection for Client -> Server switch
                    // Use Check_X which is Virtual Cursor X when remote
                    if check_x <= 0.005 && is_remote {
                        IS_REMOTE.store(false, Ordering::SeqCst);
                        show_notification("Returned to Local Control");
                        return Some(InputEvent::ScreenSwitch(platform_passer_core::ScreenSide::Local));
                    }

                    if !is_remote { return None; }

                    // Send the absolute (virtual) position if remote, or physical if not?
                    // Actually protocol expects normalized coordinates.
                    // If is_remote is true, we should probably send the virtual coordinates?
                    // Or keep sending the physical ones? 
                    // No, physical ones are stuck. We MUST send virtual coordinates if we want smooth movement on client
                    // BUT the client might be expecting relative deltas? 
                    // Let's stick to sending the MouseMove event. 
                    // Wait, if we send 'x' and 'y' derived from 'point' (physical), they are static.
                    // We should send 'vc.0' and 'vc.1'.
                    
                    let mut final_x = abs_x;
                    let mut final_y = abs_y;
                    
                    if is_remote {
                         if let Ok(vc) = VIRTUAL_CURSOR.lock() {
                             final_x = vc.0;
                             final_y = vc.1;
                         }
                    }

                    Some(InputEvent::MouseMove { x: final_x, y: final_y })
                } else {
                    None
                }
            }
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
            Some(InputEvent::MouseButton { button, is_down })
        }
        CGEventType::KeyDown | CGEventType::KeyUp | CGEventType::FlagsChanged => {
            let key_code = event.get_integer_value_field(9); // kCGKeyboardEventKeycode = 9
            
            // Check for hotkey to return to local (e.g., Command + Escape)
            // Allow this even if remote (it's the escape hatch)
            if is_remote && key_code == 53 { // Escape
                 IS_REMOTE.store(false, Ordering::SeqCst);
                 show_notification("Returned to Local Control (Escape)");
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
                move |_proxy, etype, event| {
                    match etype {
                        CGEventType::TapDisabledByTimeout | CGEventType::TapDisabledByUserInput => {
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
