use crate::InputSource;
use anyhow::Result;
use platform_passer_core::InputEvent;
use std::sync::{Arc, Mutex};
use windows::Win32::Foundation::{LPARAM, WPARAM, LRESULT};
use windows::Win32::UI::WindowsAndMessaging::{
    SetWindowsHookExA, UnhookWindowsHookEx, CallNextHookEx, GetMessageA,
    WH_KEYBOARD_LL, WH_MOUSE_LL, HHOOK, KBDLLHOOKSTRUCT, MSLLHOOKSTRUCT, WM_KEYDOWN, WM_SYSKEYDOWN,
    WM_MOUSEMOVE,
};
use std::thread;

use std::sync::atomic::{AtomicBool, Ordering};

static IS_REMOTE: AtomicBool = AtomicBool::new(false);
static VIRTUAL_CURSOR: Mutex<(f32, f32)> = Mutex::new((0.0, 0.0));

// Global callback storage
type HookCallback = Box<dyn Fn(InputEvent) + Send + Sync>;
static GLOBAL_CALLBACK: Mutex<Option<Arc<HookCallback>>> = Mutex::new(None);
static mut KEYBOARD_HOOK: HHOOK = HHOOK(0);
static mut MOUSE_HOOK: HHOOK = HHOOK(0);

pub struct WindowsInputSource;

impl WindowsInputSource {
    pub fn new() -> Self {
        Self
    }

    pub fn set_remote(remote: bool) {
        IS_REMOTE.store(remote, Ordering::SeqCst);
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
        
        let screen_width = windows::Win32::UI::WindowsAndMessaging::GetSystemMetrics(windows::Win32::UI::WindowsAndMessaging::SM_CXSCREEN);
        let screen_height = windows::Win32::UI::WindowsAndMessaging::GetSystemMetrics(windows::Win32::UI::WindowsAndMessaging::SM_CYSCREEN);
        
        let abs_x = ms.pt.x as f32 / screen_width as f32;
        let abs_y = ms.pt.y as f32 / screen_height as f32;

        let mut event = None;
        let msg = wparam.0 as u32;

        match msg {
            WM_MOUSEMOVE => {
                let mut check_x = abs_x;
                
                if is_remote {
                    // In remote mode, accumulate deltas (Windows doesn't freeze cursor, but we bypass its effect)
                    // Simplified: Windows hooks still get physical points.
                    // For now, let's follow macOS style: if remote, use deltas.
                    // But Windows hooks provide absolute points.
                    // Handle edge detection to return
                    if abs_x <= 0.002 { // Left edge of Windows (Assuming Mac is on LEFT of Win now? No, User said [Win][Mac])
                        // User said: Windows is on LEFT, Mac is on RIGHT.
                        // So: Right edge of Windows (1.0) -> Mac.
                        // Left edge of Mac (0.0) -> Windows.
                    }
                }

                // Layout [Win][Mac]: 
                // Windows Right Edge (>= 0.998) -> Switch to Remote (Mac)
                if !is_remote && abs_x >= 0.998 {
                    IS_REMOTE.store(true, Ordering::SeqCst);
                    is_remote = true;
                    swallow = true;
                    tracing::info!("InputSource: [Win][Mac] -> Switched to Remote (Mac)");
                    event = Some(InputEvent::ScreenSwitch(platform_passer_core::ScreenSide::Remote));
                }
                
                // If remote, we stay at the edge and "pull" from it? 
                // Actually, let's keep it simple: if remote, we send normalizing move.
                if is_remote {
                    // Normalize as if we are on the Remote screen.
                    // For now, let's just send the absolute position on THIS monitor scaled.
                    event = Some(InputEvent::MouseMove { x: abs_x, y: abs_y });
                    
                    // ESCAPE HATCH for Windows -> Mac return?
                    // If we want to return: move to LEFT edge of Mac (which maps back to Windows RIGHT edge)
                    // This logic is tricky without knowing Mac's absolute pos.
                    // Let's assume Mac callback sends a ScreenSwitch(Local) back.
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

impl InputSource for WindowsInputSource {
    fn start_capture(&self, callback: Box<dyn Fn(InputEvent) + Send + Sync>) -> Result<()> {
        // Set global callback
        {
            let mut guard = GLOBAL_CALLBACK.lock().unwrap();
            *guard = Some(Arc::new(callback));
        }

        // Spawn thread for message loop
        thread::spawn(|| unsafe {
            // Note: In reality, hooks must be set in the thread that runs the loop.
            // Simplified for skeletal implementation.
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
}
