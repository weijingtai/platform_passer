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
        
        if !remote {
            // Warp cursor 50 pixels inwards if at edge to prevent immediate re-trigger
            unsafe {
                let mut pt = windows::Win32::Foundation::POINT::default();
                if GetCursorPos(&mut pt).is_ok() {
                    let v_left = GetSystemMetrics(SM_XVIRTUALSCREEN);
                    let v_top = GetSystemMetrics(SM_YVIRTUALSCREEN);
                    let v_width = GetSystemMetrics(SM_CXVIRTUALSCREEN);
                    let v_height = GetSystemMetrics(SM_CYVIRTUALSCREEN);

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
        let mut is_remote = IS_REMOTE.load(Ordering::SeqCst);
        let mut swallow = is_remote;
        
        let v_left = unsafe { GetSystemMetrics(SM_XVIRTUALSCREEN) } as f32;
        let v_top = unsafe { GetSystemMetrics(SM_YVIRTUALSCREEN) } as f32;
        let v_width = unsafe { GetSystemMetrics(SM_CXVIRTUALSCREEN) } as f32;
        let v_height = unsafe { GetSystemMetrics(SM_CYVIRTUALSCREEN) } as f32;
        
        // Normalize coordinates relative to virtual desktop
        let abs_x = (ms.pt.x as f32 - v_left) / v_width;
        let abs_y = (ms.pt.y as f32 - v_top) / v_height;

        let mut event = None;
        let msg = wparam.0 as u32;

        match msg {
            WM_MOUSEMOVE => {
                // Dynamic Edge Detection
                if !is_remote {
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
                    // Fallback default
                    if GLOBAL_CONFIG.lock().unwrap().is_none() && abs_x >= 0.998 {
                        trigger_remote = true;
                    }

                    if trigger_remote {
                        IS_REMOTE.store(true, Ordering::SeqCst);
                        is_remote = true;
                        swallow = true;
                        tracing::info!("InputSource: Switched to Remote");
                        event = Some(InputEvent::ScreenSwitch(ScreenSide::Remote));
                    }
                }
                
                if is_remote {
                    event = Some(InputEvent::MouseMove { x: abs_x, y: abs_y });
                }
            }
            WM_LBUTTONDOWN | WM_LBUTTONUP | WM_RBUTTONDOWN | WM_RBUTTONUP | WM_MBUTTONDOWN | WM_MBUTTONUP => {
                if is_remote {
                    let button = match msg {
                        WM_LBUTTONDOWN | WM_LBUTTONUP => platform_passer_core::MouseButton::Left,
                        WM_RBUTTONDOWN | WM_RBUTTONUP => platform_passer_core::MouseButton::Right,
                        _ => platform_passer_core::MouseButton::Middle,
                    };
                    let is_down = msg == WM_LBUTTONDOWN || msg == WM_RBUTTONDOWN || msg == WM_MBUTTONDOWN;
                    event = Some(InputEvent::MouseButton { button, is_down });
                }
            }
            0x020A => { // WM_MOUSEWHEEL
                if is_remote {
                    let delta = (ms.mouseData >> 16) as i16 as f32 / 120.0;
                    event = Some(InputEvent::Scroll { dx: 0.0, dy: delta });
                }
            }
            0x020E => { // WM_MOUSEHWHEEL
                if is_remote {
                    let delta = (ms.mouseData >> 16) as i16 as f32 / 120.0;
                    event = Some(InputEvent::Scroll { dx: delta, dy: 0.0 });
                }
            }
            _ => {}
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

