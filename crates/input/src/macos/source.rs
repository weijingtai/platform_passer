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

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Mutex;
use std::time::Instant;

static IS_REMOTE: AtomicBool = AtomicBool::new(false);
static PRESSED_BUTTONS: AtomicU8 = AtomicU8::new(0); // Bitmask: 1=Left, 2=Right, 4=Middle
static LAST_SWITCH_TIME: Mutex<Option<Instant>> = Mutex::new(None);
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
        let old = IS_REMOTE.swap(remote, Ordering::SeqCst);
        if old && !remote {
            // Transition from Remote to Local: Start cooling period
            if let Ok(mut lock) = LAST_SWITCH_TIME.lock() {
                *lock = Some(Instant::now());
            }
        }
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

fn show_notification(_text: &str) {
    // CRITICAL: std::process::Command spawning osascript from inside/near 
    // a high-frequency FFI callback or multi-threaded context is causing 
    // "error 0" (process aborts). Leaving this as a no-op for stability.
}

fn handle_event(etype: CGEventType, event: &CGEvent) -> Option<InputEvent> {
    let mut is_remote = IS_REMOTE.load(Ordering::SeqCst);
    let (max_width, max_height) = get_display_bounds();

    match etype {
        CGEventType::MouseMoved | CGEventType::LeftMouseDragged | CGEventType::RightMouseDragged => {
            let point = event.location();
            
            // Normalize absolute position
            let abs_x = point.x as f32 / max_width;
            let abs_y = point.y as f32 / max_height;

            // Decision variable: By default follow physical cursor
            let mut check_x = abs_x;

            if is_remote {
                let delta_x = event.get_double_value_field(4) as f32; // kCGMouseEventDeltaX = 4
                let delta_y = event.get_double_value_field(5) as f32; // kCGMouseEventDeltaY = 5
                
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
            }

            // --- LIGIC: Windows (Left) - macOS (Right) ---

            // Switch to Remote: Triggered when at macOS LEFT edge
            if !is_remote && abs_x <= 0.002 {
                IS_REMOTE.store(true, Ordering::SeqCst);
                is_remote = true; // Update local state for subsequent logic in this call
                
                // Initialize Virtual Cursor at the RIGHT edge of Windows
                if let Ok(mut vc) = VIRTUAL_CURSOR.lock() {
                    *vc = (0.990, abs_y); // Slightly away from edge for hysteresis
                    check_x = vc.0;
                }
                
                tracing::info!("InputSource: Layout [W][M]. Entered Windows (Left) from macOS (Right). Start at x=0.99");
                return Some(InputEvent::ScreenSwitch(platform_passer_core::ScreenSide::Remote));
            }
            
            // Return to Local: Triggered when Virtual Cursor hits RIGHT edge of Windows
            if is_remote && check_x >= 0.998 {
                MacosInputSource::set_remote(false);
                is_remote = false;
                
                tracing::info!("InputSource: Layout [W][M]. Returned to macOS.");
                return Some(InputEvent::ScreenSwitch(platform_passer_core::ScreenSide::Local));
            }

            // If not remote, don't send moves
            if !is_remote { return None; }

            // Route correct coordinate
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
            let button_bit = match etype {
                CGEventType::LeftMouseDown | CGEventType::LeftMouseUp => 1,
                CGEventType::RightMouseDown | CGEventType::RightMouseUp => 2,
                _ => 4,
            };
            let is_down = matches!(etype, CGEventType::LeftMouseDown | CGEventType::RightMouseDown | CGEventType::OtherMouseDown);
            
            if is_down {
                PRESSED_BUTTONS.fetch_or(button_bit, Ordering::SeqCst);
            } else {
                PRESSED_BUTTONS.fetch_and(!button_bit, Ordering::SeqCst);
            }

            if !is_remote { return None; }
            let button = match button_bit {
                1 => platform_passer_core::MouseButton::Left,
                2 => platform_passer_core::MouseButton::Right,
                _ => platform_passer_core::MouseButton::Middle,
            };
            tracing::info!("InputSource: Mouse Button {:?} {}", button, if is_down { "Down" } else { "Up" });
            Some(InputEvent::MouseButton { button, is_down })
        }
        CGEventType::KeyDown | CGEventType::KeyUp | CGEventType::FlagsChanged => {
            let key_code = event.get_integer_value_field(9); // kCGKeyboardEventKeycode = 9
            
            // Check for hotkey to return to local (e.g., Command + Escape)
            // Allow this even if remote (it's the escape hatch)
            if is_remote && key_code == 53 { // Escape
                 MacosInputSource::set_remote(false);
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
        // Check permissions before starting
        if !crate::macos::permissions::check_accessibility_trusted() {
            tracing::warn!("InputSource: Accessibility permissions missing. Triggering system dialog...");
            crate::macos::permissions::ensure_accessibility_permissions();
            return Err(anyhow!("Accessibility permissions required. Please enable in System Settings."));
        }

        if !crate::macos::permissions::check_input_monitoring_enabled() {
             tracing::warn!("InputSource: Input Monitoring permissions likely missing.");
             // Note: There is no easy way to trigger Input Monitoring dialog programmatically 
             // like Accessibility, so we just log the warning and instructions.
             crate::macos::permissions::open_system_settings_input_monitoring();
        }

        // Log monitor dimensions for DPI verification
        let (w, h) = get_display_bounds();
        tracing::info!("InputSource: Starting capture. Primary display/workspace bounds: {}x{}", w, h);

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
                move |_proxy, etype, event| {
                    match etype {
                        CGEventType::TapDisabledByTimeout | CGEventType::TapDisabledByUserInput => {
                            println!("WARNING: CGEventTap disabled. Re-enabling...");
                            
                            // Use raw port re-enable using the stored tap pointer
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
                            let was_remote_initially = IS_REMOTE.load(Ordering::SeqCst);

                            // Process event logic (extraction, sending to client)
                            if let Some(ev) = handle_event(etype, event) {
                                callback_arc(ev);
                            }
                            
                            let is_remote_now = IS_REMOTE.load(Ordering::SeqCst);
                            
                            // Multi-layer Protection Logic:
                            // 1. We are currently remote (is_remote_now).
                            // 2. We WERE remote but just switched (was_remote_initially && !is_remote_now).
                            // 3. We are local, but buttons are still physically pressed down from remote mode (button latching).
                            // 4. We are local, but within the 300ms "Landing Zone" cooling period.
                            
                            let buttons_pressed = PRESSED_BUTTONS.load(Ordering::SeqCst) != 0;
                            let in_cooling = if let Ok(lock) = LAST_SWITCH_TIME.lock() {
                                lock.map_or(false, |t| t.elapsed().as_millis() < 300)
                            } else { false };

                            if is_remote_now {
                                // Steady Remote: Swallow everything
                                None
                            } else if was_remote_initially || buttons_pressed || in_cooling {
                                // Transitioning or Protected period
                                match etype {
                                    CGEventType::MouseMoved | CGEventType::LeftMouseDragged | CGEventType::RightMouseDragged => {
                                        // Allow moves so cursor can "land" and user can see where it is
                                        Some(event.to_owned())
                                    }
                                    _ => {
                                        // Swallow everything else (Clicks, Keys, Scroll)
                                        // This prevents "MouseDown on Remote" -> "EOF/Switch" -> "MouseUp on Local" leakage.
                                        None
                                    }
                                }
                            } else {
                                // Steady Local
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
