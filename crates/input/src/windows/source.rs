use crate::InputSource;
use anyhow::Result;
use platform_passer_core::{InputEvent, ScreenSide};
use platform_passer_core::config::{AppConfig, ScreenPosition};
use std::sync::{Arc, Mutex};
use windows::Win32::Foundation::{LPARAM, WPARAM, LRESULT};
use windows::Win32::UI::WindowsAndMessaging::{
    SetWindowsHookExA, UnhookWindowsHookEx, CallNextHookEx, GetMessageA,
    WH_KEYBOARD_LL, WH_MOUSE_LL, HHOOK, KBDLLHOOKSTRUCT, MSLLHOOKSTRUCT, WM_KEYDOWN, WM_SYSKEYDOWN,
    WM_MOUSEMOVE, GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
    GetCursorPos, SetCursorPos,
};
use tracing;
use std::thread;

use std::sync::atomic::{AtomicBool, Ordering};

static IS_REMOTE: AtomicBool = AtomicBool::new(false);
static VIRTUAL_CURSOR_POS: Mutex<Option<(f32, f32)>> = Mutex::new(None);

// Global callback storage
type HookCallback = Box<dyn Fn(InputEvent) + Send + Sync>;
static GLOBAL_CALLBACK: Mutex<Option<Arc<HookCallback>>> = Mutex::new(None);
static GLOBAL_CONFIG: Mutex<Option<AppConfig>> = Mutex::new(None);
static mut KEYBOARD_HOOK: HHOOK = HHOOK(0);
static mut MOUSE_HOOK: HHOOK = HHOOK(0);

pub struct WindowsInputSource;

impl WindowsInputSource {
    pub fn new() -> Self {
        Self
    }
}

impl InputSource for WindowsInputSource {
    fn start_capture(&self, callback: Box<dyn Fn(InputEvent) + Send + Sync>) -> Result<()> {
        // Set global callback
        {
            let mut guard = GLOBAL_CALLBACK.lock().unwrap();
            *guard = Some(Arc::new(callback));
        }

        // Spawn thread for message loop
        thread::spawn(|| unsafe {
             let h_instance = windows::Win32::System::LibraryLoader::GetModuleHandleA(None).unwrap();

             KEYBOARD_HOOK = SetWindowsHookExA(WH_KEYBOARD_LL, Some(keyboard_proc), h_instance, 0).unwrap();
             MOUSE_HOOK = SetWindowsHookExA(WH_MOUSE_LL, Some(mouse_proc), h_instance, 0).unwrap();

             let mut msg = Default::default();
             while GetMessageA(&mut msg, None, 0, 0).into() {
                 windows::Win32::UI::WindowsAndMessaging::TranslateMessage(&msg);
                 windows::Win32::UI::WindowsAndMessaging::DispatchMessageA(&msg);
             }
        });

        Ok(())
    }

    fn stop_capture(&self) -> Result<()> {
        unsafe {
            if KEYBOARD_HOOK.0 != 0 {
                UnhookWindowsHookEx(KEYBOARD_HOOK);
            }
            if MOUSE_HOOK.0 != 0 {
                UnhookWindowsHookEx(MOUSE_HOOK);
            }
        }
        Ok(())
    }

    fn set_remote(&self, remote: bool) -> Result<()> {
        IS_REMOTE.store(remote, Ordering::SeqCst);
        
        unsafe {
            let v_left = GetSystemMetrics(SM_XVIRTUALSCREEN);
            let v_top = GetSystemMetrics(SM_YVIRTUALSCREEN);
            let v_width = GetSystemMetrics(SM_CXVIRTUALSCREEN);
            let v_height = GetSystemMetrics(SM_CYVIRTUALSCREEN);

            if remote {
                 // Initialize virtual cursor to current position
                let mut pt = windows::Win32::Foundation::POINT::default();
                if GetCursorPos(&mut pt).is_ok() {
                    let abs_x = (pt.x - v_left) as f32 / v_width as f32;
                    let abs_y = (pt.y - v_top) as f32 / v_height as f32;
                    *VIRTUAL_CURSOR_POS.lock().unwrap() = Some((abs_x, abs_y));
                    
                    // Center the cursor to start relative tracking
                    let center_x = v_left + v_width / 2;
                    let center_y = v_top + v_height / 2;
                    let _ = SetCursorPos(center_x, center_y);
                }
            } else {
                // Return from remote
                // Reset virtual cursor
                *VIRTUAL_CURSOR_POS.lock().unwrap() = None;

                // Warp cursor inwards
                let mut pt = windows::Win32::Foundation::POINT::default();
                if GetCursorPos(&mut pt).is_ok() {
                    let mut new_x = pt.x;
                    let mut new_y = pt.y;

                    // Check which edge we are at and move 50px inwards
                    if pt.x <= v_left + 1 { new_x += 50; }
                    if pt.x >= v_left + v_width - 1 { new_x -= 50; }
                    if pt.y <= v_top + 1 { new_y += 50; }
                    if pt.y >= v_top + v_height - 1 { new_y -= 50; }

                    if new_x != pt.x || new_y != pt.y {
                        let _ = SetCursorPos(new_x, new_y);
                    }
                }
            }
        }
        Ok(())
    }

    fn update_config(&self, config: AppConfig) -> Result<()> {
        let mut guard = GLOBAL_CONFIG.lock().unwrap();
        *guard = Some(config);
        Ok(())
    }
}

unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        let kbd = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
        let is_remote = IS_REMOTE.load(Ordering::SeqCst);
        
        let is_down = wparam.0 as u32 == WM_KEYDOWN || wparam.0 as u32 == WM_SYSKEYDOWN;
        
        let event = InputEvent::Keyboard {
            key_code: kbd.vkCode,
            is_down,
        };

        if is_remote {
            if let Ok(guard) = GLOBAL_CALLBACK.lock() {
                if let Some(cb) = &*guard {
                    cb(event);
                }
            }
            return LRESULT(1); // Swallow
        }

        if let Ok(guard) = GLOBAL_CALLBACK.lock() {
            if let Some(cb) = &*guard {
                cb(event);
            }
        }
    }
    CallNextHookEx(KEYBOARD_HOOK, code, wparam, lparam)
}

unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        let ms = &*(lparam.0 as *const MSLLHOOKSTRUCT);
        // Important: check if this event was injected by us (SetCursorPos)
        // LLMHF_INJECTED (bit 0) or LLMHF_LOWER_IL_INJECTED (bit 1)
        let injected = (ms.flags & 0x01) != 0 || (ms.flags & 0x02) != 0;

        let mut is_remote = IS_REMOTE.load(Ordering::SeqCst);
        let mut swallow = is_remote;
        
        let v_left = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let v_top = GetSystemMetrics(SM_YVIRTUALSCREEN);
        let v_width = GetSystemMetrics(SM_CXVIRTUALSCREEN);
        let v_height = GetSystemMetrics(SM_CYVIRTUALSCREEN);
        
        // Normalize physical coordinates for local logic
        let abs_x = (ms.pt.x - v_left) as f32 / v_width as f32;
        let abs_y = (ms.pt.y - v_top) as f32 / v_height as f32;

        let mut event = None;
        let msg = wparam.0 as u32;

        if is_remote {
            // REMOTE MODE: Center Locking Logic
            if msg == WM_MOUSEMOVE {
                if !injected {
                    let center_x = v_left + v_width / 2;
                    let center_y = v_top + v_height / 2;

                    let dx = ms.pt.x - center_x;
                    let dy = ms.pt.y - center_y;

                    // Only process if moved from center
                    if dx != 0 || dy != 0 {
                        let mut guard = VIRTUAL_CURSOR_POS.lock().unwrap();
                        if let Some((vx, vy)) = *guard {
                            let mut new_vx = vx + (dx as f32 / v_width as f32);
                            let mut new_vy = vy + (dy as f32 / v_height as f32);

                            // Clamp
                            new_vx = new_vx.max(0.0).min(1.0);
                            new_vy = new_vy.max(0.0).min(1.0);
                            
                            *guard = Some((new_vx, new_vy));
                            
                            // Send Absolute Virtual Position
                            use std::time::{Instant, Duration};
                            static mut LAST_SEND: Option<Instant> = None;
                            
                            let now = Instant::now();
                            let should_send = unsafe {
                                match LAST_SEND {
                                    Some(last) => now.duration_since(last) >= Duration::from_millis(8),
                                    None => true,
                                }
                            };

                            if should_send {
                                unsafe { LAST_SEND = Some(now); }
                                event = Some(InputEvent::MouseMove { x: new_vx, y: new_vy });
                            }
                            
                            // Re-center physical cursor
                            let _ = SetCursorPos(center_x, center_y);
                        }
                    }
                }
            } else {
                 event = match msg {
                    WM_LBUTTONDOWN | WM_LBUTTONUP => Some(InputEvent::MouseButton { button: platform_passer_core::MouseButton::Left, is_down: msg == WM_LBUTTONDOWN }),
                    WM_RBUTTONDOWN | WM_RBUTTONUP => Some(InputEvent::MouseButton { button: platform_passer_core::MouseButton::Right, is_down: msg == WM_RBUTTONDOWN }),
                    WM_MBUTTONDOWN | WM_MBUTTONUP => Some(InputEvent::MouseButton { button: platform_passer_core::MouseButton::Middle, is_down: msg == WM_MBUTTONDOWN }),
                    0x020A => { // WM_MOUSEWHEEL
                        let delta = (ms.mouseData >> 16) as i16 as f32 / 120.0;
                         Some(InputEvent::Scroll { dx: 0.0, dy: delta })
                    },
                    0x020E => { // WM_MOUSEHWHEEL
                        let delta = (ms.mouseData >> 16) as i16 as f32 / 120.0;
                        Some(InputEvent::Scroll { dx: delta, dy: 0.0 })
                    },
                     _ => None
                };
            }
        } else {
            // LOCAL MODE: Edge Detection
             if msg == WM_MOUSEMOVE {
                let mut trigger_remote = false;
                if let Ok(config_opt) = GLOBAL_CONFIG.lock() {
                   if let Some(config) = &*config_opt {
                       for remote in &config.topology.remotes {
                           match remote.position {
                               ScreenPosition::Right => if abs_x >= 0.999 { trigger_remote = true; },
                               ScreenPosition::Left => if abs_x <= 0.001 { trigger_remote = true; },
                               ScreenPosition::Top => if abs_y <= 0.001 { trigger_remote = true; },
                               ScreenPosition::Bottom => if abs_y >= 0.999 { trigger_remote = true; },
                           }
                       }
                   }
                }
                // REMOVED: Fallback default edge detection
                // This caused Client to accidentally enter Remote mode and freeze cursor
                // Edge detection should ONLY happen when explicitly configured via Topology
                // if GLOBAL_CONFIG.lock().unwrap().is_none() && abs_x >= 0.998 {
                //     trigger_remote = true;
                // }

                if trigger_remote {
                    // Switch to Remote
                    IS_REMOTE.store(true, Ordering::SeqCst);
                    swallow = true;
                    
                    // Initialize Virtual Cursor
                    *VIRTUAL_CURSOR_POS.lock().unwrap() = Some((abs_x, abs_y));
                    
                    // Center Lock immediately
                    let center_x = v_left + v_width / 2;
                    let center_y = v_top + v_height / 2;
                    let _ = SetCursorPos(center_x, center_y);

                    tracing::info!("InputSource: Switched to Remote");
                    event = Some(InputEvent::ScreenSwitch(ScreenSide::Remote));
                }
            }
        }

        if let Some(ev) = event {
            if let Ok(guard) = GLOBAL_CALLBACK.lock() {
                if let Some(cb) = &*guard {
                    cb(ev);
                }
            }
        }

        if swallow {
            return LRESULT(1);
        }
    }
    CallNextHookEx(MOUSE_HOOK, code, wparam, lparam)
}

const WM_LBUTTONDOWN: u32 = 0x0201;
const WM_LBUTTONUP: u32 = 0x0202;
const WM_RBUTTONDOWN: u32 = 0x0204;
const WM_RBUTTONUP: u32 = 0x0205;
const WM_MBUTTONDOWN: u32 = 0x0207;
const WM_MBUTTONUP: u32 = 0x0208;

