use crate::InputSource;
use anyhow::{Result, anyhow};
use platform_passer_core::InputEvent;
use platform_passer_core::config::{AppConfig, Topology, ScreenPosition};
use std::sync::Arc;
use std::thread;
use core_foundation::runloop::{CFRunLoop, kCFRunLoopCommonModes};
use core_graphics::event::{CGEvent, CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement, CGEventType};

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventTapEnable(tap: *mut std::ffi::c_void, enable: bool);
    fn CGAssociateMouseAndMouseCursorPosition(connected: bool) -> u32;
    fn CGDisplayHideCursor(display: u32) -> u32;
    fn CGDisplayShowCursor(display: u32) -> u32;
    fn CGWarpMouseCursorPosition(new_pos: core_graphics::geometry::CGPoint) -> u32;
    fn CGMainDisplayID() -> u32;
}

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Mutex;
use std::time::Instant;

static IS_REMOTE: AtomicBool = AtomicBool::new(false);
static PRESSED_BUTTONS: AtomicU8 = AtomicU8::new(0); // Bitmask: 1=Left, 2=Right, 4=Middle
static LAST_SWITCH_TIME: Mutex<Option<Instant>> = Mutex::new(None);
static VIRTUAL_CURSOR: Mutex<(f32, f32)> = Mutex::new((0.0, 0.0));
static DISPLAY_CACHE: Mutex<Option<(f32, f32)>> = Mutex::new(None);
static TOPOLOGY: Mutex<Option<Topology>> = Mutex::new(None);
static ACTIVE_REMOTE_POS: Mutex<Option<ScreenPosition>> = Mutex::new(None);

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
        println!("InputSource: [DEBUG] set_remote({}) called", remote);
        let old = IS_REMOTE.swap(remote, Ordering::SeqCst);
        
        // REMOVED: Idempotency check caused failure to recover state
        // if old == remote { return; }

        unsafe {
            let main_display = CGMainDisplayID();
            
            // 0. Warp to Center (Nuclear Option)
            // Prevent immediate edge triggering or Hot Corner issues.
            let (max_width, max_height) = get_display_bounds();
            let center = core_graphics::geometry::CGPoint { 
                x: (max_width / 2.0) as f64, 
                y: (max_height / 2.0) as f64 
            };
            let _ = CGWarpMouseCursorPosition(center);

            // 1. CoreGraphics Cursor Association (The "Freeze" API)
            // true = cursor moves with mouse (Local)
            // false = cursor decoupled (Remote)
            let result = CGAssociateMouseAndMouseCursorPosition(!remote);
            if result != 0 {
                println!("InputSource: [ERROR] CGAssociateMouseAndMouseCursorPosition failed with error: {}", result);
            }

            // 2. Explicitly Hide/Show Cursor
            if remote {
                 let _ = CGDisplayHideCursor(main_display);
            } else {
                 let _ = CGDisplayShowCursor(main_display);
            }
        }

        if old && !remote {
            // Transition from Remote to Local: Start cooling period
            if let Ok(mut lock) = LAST_SWITCH_TIME.lock() {
                *lock = Some(Instant::now());
            }
        }
    }

    fn update_topology(topology: Topology) {
        if let Ok(mut lock) = TOPOLOGY.lock() {
            *lock = Some(topology);
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
                
                let mut ignore_delta = false;
                if delta_x.abs() > 50.0 || delta_y.abs() > 50.0 {
                     println!("InputSource: [DEBUG] Ignored massive delta ({}, {}) - likely warp artifact", delta_x, delta_y);
                     ignore_delta = true;
                }

                if let Ok(mut vc) = VIRTUAL_CURSOR.lock() {
                    if !ignore_delta {
                        // Update virtual coords (normalized)
                        vc.0 += delta_x / max_width; 
                        vc.1 += delta_y / max_height;
                        
                        // Clamp
                        if vc.0 < 0.0 { vc.0 = 0.0; }
                        if vc.0 > 1.0 { vc.0 = 1.0; }
                        if vc.1 < 0.0 { vc.1 = 0.0; }
                        if vc.1 > 1.0 { vc.1 = 1.0; }
                    }
                    
                    // CRITICAL: Always use Virtual Cursor position for edge checks when Remote.
                    // This prevents physical cursor movement (due to warp or leaks) from triggering accidental exits.
                    check_x = vc.0;
                }
            }

            // --- LIGIC: Windows (Left) - macOS (Right) ---

            // Switch to Remote: Triggered when at macOS LEFT edge
            // Switch to Remote: Triggered when at macOS LEFT edge
            // Switch to Remote: Check configured edges
            let mut triggered_remote = None;
            if !is_remote {
                 // Default to Left if no topology (Backwards comp)
                 let mut checked = false;
                 if let Ok(guard) = TOPOLOGY.lock() {
                     if let Some(topo) = &*guard {
                         checked = true;
                         for remote in &topo.remotes {
                             let hit = match remote.position {
                                 ScreenPosition::Left => abs_x <= 0.002,
                                 ScreenPosition::Right => abs_x >= 0.998,
                                 ScreenPosition::Top => abs_y <= 0.002,
                                 ScreenPosition::Bottom => abs_y >= 0.998,
                             };
                             if hit {
                                 triggered_remote = Some(remote.clone());
                                 break;
                             }
                         }
                     }
                 }
                 
                 // Fallback: Default Left Edge if config missing
                 if !checked && abs_x <= 0.002 {
                     // create dummy remote for fallback
                     // This is tricky without a real object, but we just set IS_REMOTE.
                     // We'll set ACTIVE_REMOTE_POS to Left.
                     if let Ok(mut pos) = ACTIVE_REMOTE_POS.lock() { *pos = Some(ScreenPosition::Left); }
                     IS_REMOTE.store(true, Ordering::SeqCst);
                     is_remote = true;
                     // Init VC at Right Edge (Assuming Left Remote)
                     if let Ok(mut vc) = VIRTUAL_CURSOR.lock() { *vc = (0.950, abs_y); check_x = vc.0; }
                     return Some(InputEvent::ScreenSwitch(platform_passer_core::ScreenSide::Remote));
                 }
            }

            if let Some(remote) = triggered_remote {
                IS_REMOTE.store(true, Ordering::SeqCst);
                is_remote = true;
                
                // Store active position
                if let Ok(mut pos) = ACTIVE_REMOTE_POS.lock() { *pos = Some(remote.position.clone()); }

                // Determine entry point on REMOTE screen
                // If we exit Local Left -> Enter Remote Right (x=0.95)
                // If we exit Local Right -> Enter Remote Left (x=0.05)
                // If we exit Local Top -> Enter Remote Bottom (y=0.95)
                // If we exit Local Bottom -> Enter Remote Top (y=0.05)
                let (entry_x, entry_y) = match remote.position {
                    ScreenPosition::Left => (0.950, abs_y),
                    ScreenPosition::Right => (0.050, abs_y),
                    ScreenPosition::Top => (abs_x, 0.950),
                    ScreenPosition::Bottom => (abs_x, 0.050),
                };

                if let Ok(mut vc) = VIRTUAL_CURSOR.lock() {
                    *vc = (entry_x, entry_y);
                    check_x = vc.0; 
                }
                
                println!("DEBUG: Switching to Remote ({:?})", remote.position);
                return Some(InputEvent::ScreenSwitch(platform_passer_core::ScreenSide::Remote));
            }
            
            // Return to Local
            if is_remote {
                let active_pos = if let Ok(guard) = ACTIVE_REMOTE_POS.lock() { guard.clone().unwrap_or(ScreenPosition::Left) } else { ScreenPosition::Left };
                
                let should_return = match active_pos {
                    ScreenPosition::Left => check_x >= 0.998, // Remote is on Left, so we return when Remote Cursor hits Right
                    ScreenPosition::Right => check_x <= 0.002, // Remote is on Right, return when Remote Cursor hits Left
                    ScreenPosition::Top => { 
                         // Check Y. We need check_y?
                         // Current logic uses check_x for everything. We need to check Y for Top/Bottom!
                         // But we calculated vc in handle_event start. We need access to vc.1.
                         // check_x was set to vc.0.
                         // Let's get y from VIRTUAL_CURSOR lock again or assume we need to change check var.
                         // Hack: Read vc again.
                         let vc_y = if let Ok(vc) = VIRTUAL_CURSOR.lock() { vc.1 } else { 0.5 };
                         vc_y >= 0.998
                    },
                    ScreenPosition::Bottom => {
                         let vc_y = if let Ok(vc) = VIRTUAL_CURSOR.lock() { vc.1 } else { 0.5 };
                         vc_y <= 0.002
                    },
                };

                if should_return {
                    // Restore to appropriate edge
                    let (bounds_w, bounds_h) = get_display_bounds();
                    let (ret_x, ret_y) = match active_pos {
                        ScreenPosition::Left => (10.0, if let Ok(vc) = VIRTUAL_CURSOR.lock() { vc.1 } else { 0.5 } * bounds_h),
                        ScreenPosition::Right => (bounds_w - 10.0, if let Ok(vc) = VIRTUAL_CURSOR.lock() { vc.1 } else { 0.5 } * bounds_h),
                        ScreenPosition::Top => (if let Ok(vc) = VIRTUAL_CURSOR.lock() { vc.0 } else { 0.5 } * bounds_w, 10.0),
                        ScreenPosition::Bottom => (if let Ok(vc) = VIRTUAL_CURSOR.lock() { vc.0 } else { 0.5 } * bounds_w, bounds_h - 10.0),
                    };

                    let edge_pos = core_graphics::geometry::CGPoint { 
                        x: ret_x as f64, 
                        y: ret_y as f64
                    };

                // CRITICAL: Call set_remote(false) FIRST to re-associate.
                // THEN warp. If we warp while disassociated, the re-association might snap back to the "Center Lock" hardware position.
                MacosInputSource::set_remote(false);
                
                unsafe {
                    let _ = CGWarpMouseCursorPosition(edge_pos);
                    println!("DEBUG: [W][M] Warped cursor to edge: ({}, {})", edge_pos.x, edge_pos.y);
                }

                is_remote = false;
                
                println!("DEBUG: [W][M] Returning to macOS. Triggered at virtual x={:.3}", check_x);
                return Some(InputEvent::ScreenSwitch(platform_passer_core::ScreenSide::Local));
            }
            }

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
            // kCGScrollWheelEventDeltaAxis1 = 11 (Vertical, Y)
            // kCGScrollWheelEventDeltaAxis2 = 12 (Horizontal, X)
            let dy = event.get_integer_value_field(11); 
            let dx = event.get_integer_value_field(12);
            Some(InputEvent::Scroll { dx: dx as f32, dy: dy as f32 })
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
                            let handled_ev = handle_event(etype, event);
                            if let Some(ev) = handled_ev {
                                callback_arc(ev);
                            }
                            
                            let is_remote_now = IS_REMOTE.load(Ordering::SeqCst);
                            let buttons_pressed = PRESSED_BUTTONS.load(Ordering::SeqCst) != 0;
                            let in_cooling = if let Ok(lock) = LAST_SWITCH_TIME.lock() {
                                lock.map_or(false, |t| t.elapsed().as_millis() < 300)
                            } else { false };

                            let result = if is_remote_now {
                                // Steady Remote
                                match etype {
                                    CGEventType::KeyDown | CGEventType::KeyUp | CGEventType::FlagsChanged => {
                                        // Keyboard: Convert to Null
                                        event.set_type(CGEventType::Null);
                                        Some(event.to_owned())
                                    }
                                    CGEventType::MouseMoved | CGEventType::LeftMouseDragged | CGEventType::RightMouseDragged => {
                                        // Movement: WARP-LOCK
                                        // CGAssociate is unreliable. We must physically hold the cursor in place.
                                        let (max_width, max_height) = get_display_bounds();
                                        let center = core_graphics::geometry::CGPoint { 
                                            x: (max_width / 2.0) as f64, 
                                            y: (max_height / 2.0) as f64 
                                        };
                                        unsafe {
                                            let _ = CGWarpMouseCursorPosition(center);
                                        }

                                        // Swallow the event (None) since we are warping.
                                        // We don't need the OS to process the move.
                                        None
                                    }
                                    CGEventType::LeftMouseDown | CGEventType::LeftMouseUp |
                                    CGEventType::RightMouseDown | CGEventType::RightMouseUp |
                                    CGEventType::OtherMouseDown | CGEventType::OtherMouseUp |
                                    CGEventType::ScrollWheel => {
                                        // Clicks/Scrolls: Convert to Null
                                        event.set_type(CGEventType::Null);
                                        Some(event.to_owned())
                                    }
                                    _ => {
                                        // Anything else: safe to drop? Or Null?
                                        event.set_type(CGEventType::Null);
                                        Some(event.to_owned())
                                    }
                                }
                            } else if was_remote_initially || in_cooling {
                                // Transitioning or Protected period
                                match etype {
                                    CGEventType::MouseMoved | CGEventType::LeftMouseDragged | CGEventType::RightMouseDragged => {
                                        Some(event.to_owned())
                                    }
                                    _ => {
                                        if matches!(etype, CGEventType::KeyDown | CGEventType::KeyUp | CGEventType::FlagsChanged) {
                                            event.set_type(CGEventType::Null);
                                            Some(event.to_owned())
                                        } else {
                                            None
                                        }
                                    }
                                }
                            } else {
                                // Steady Local
                                Some(event.to_owned())
                            };
                            
                            result
                        }
                    }
                },
            ).map_err(|e| {
                tracing::error!("InputSource: Failed to create HID event tap: {:?}. Is 'Input Monitoring' permission granted?", e);
                anyhow!("Failed to create HID event tap. Please ensure 'Input Monitoring' permission is enabled for the terminal/app.")
            })?;

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
                // Ensure cursor is re-associated and SHOWN on stop
                unsafe {
                    let _ = CGAssociateMouseAndMouseCursorPosition(true);
                    let _ = CGDisplayShowCursor(CGMainDisplayID());
                }
            }
        }
        Ok(())
    }

    fn update_config(&self, config: AppConfig) -> Result<()> {
        MacosInputSource::update_topology(config.topology);
        Ok(())
    }
}
