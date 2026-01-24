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
    let is_remote = IS_REMOTE.load(Ordering::SeqCst);

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
                    if x >= 0.999 && !is_remote {
                        IS_REMOTE.store(true, Ordering::SeqCst);
                        return Some(InputEvent::ScreenSwitch(platform_passer_core::ScreenSide::Remote));
                    }
                    
                    // Edge detection for Client -> Server switch
                    // (Should usually come from client, but if cursor drifts back...)
                    if x <= 0.001 && is_remote {
                        IS_REMOTE.store(false, Ordering::SeqCst);
                        return Some(InputEvent::ScreenSwitch(platform_passer_core::ScreenSide::Local));
                    }

                    if !is_remote { return None; }

                    Some(InputEvent::MouseMove { x, y })
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
                 return Some(InputEvent::ScreenSwitch(platform_passer_core::ScreenSide::Local));
            }

            if !is_remote { return None; }

            let is_down = if etype == CGEventType::FlagsChanged {
                let flags = event.get_flags();
                // Check if any of the modifier bits are set
                // (Very simplified: if the keycode is a modifier, we just check the mask)
                let mask = match key_code {
                    56 | 60 => 0x00020000, // Shift (Left/Right)
                    59 | 62 => 0x00040000, // Control
                    58 | 61 => 0x00080000, // Option
                    55 | 54 => 0x00100000, // Command
                    57 => 0x00010000,      // Caps Lock
                    _ => 0,
                };
                (flags.bits() & mask) != 0
            } else {
                etype == CGEventType::KeyDown
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
                    CGEventType::FlagsChanged,
                    CGEventType::ScrollWheel,
                ],
                move |_proxy, etype, event| {
                    match etype {
                        CGEventType::TapDisabledByTimeout | CGEventType::TapDisabledByUserInput => {
                            // Re-enable if disabled? For now just log or ignore.
                            // In a real app we should re-enable the tap.
                            let tap_ptr = _proxy as *mut core_graphics::sys::CGEventTapProxy; // Incorrect but we don't have the tap ref here easily
                            // CGEventTapEnable(tap, true); 
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
                                // IMPORTANT: When remote, we must swallow LOCAL events entirely 
                                // to prevent "Ghost inputs" on the macOS side.
                                // Returning None captures the event and stops propagation.
                                
                                // Exception: Allow the specific edge-triggering MouseMove to pass? 
                                // No, capturing it is safer to stop the cursor exactly at the edge.
                                // However, if we capture ALL MouseMoves, the user might feel "stuck".
                                // But since we are "Remote", the cursor SHOULD be stuck or hidden.
                                
                                // Exception 2: FlagsChanged might need to be local for OS awareness?
                                // Usually no, we want full redirection.
                                
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
