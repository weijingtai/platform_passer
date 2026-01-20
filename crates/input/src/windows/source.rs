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

// Global callback storage is tricky. For this MVG (Minimum Viable Generation), 
// we will use a static Mutex. In production, this needs to be more robust.
type HookCallback = Box<dyn Fn(InputEvent) + Send + Sync>;

static GLOBAL_CALLBACK: Mutex<Option<Arc<HookCallback>>> = Mutex::new(None);
static mut KEYBOARD_HOOK: HHOOK = HHOOK(0);
static mut MOUSE_HOOK: HHOOK = HHOOK(0);

pub struct WindowsInputSource;

impl WindowsInputSource {
    pub fn new() -> Self {
        Self
    }
}

unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        let kbd = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
        // Basic mapping
        let is_down = wparam.0 as u32 == WM_KEYDOWN || wparam.0 as u32 == WM_SYSKEYDOWN;
        
        let event = InputEvent::Keyboard {
            key_code: kbd.vkCode,
            is_down,
        };

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
        if wparam.0 as u32 == WM_MOUSEMOVE {
             let event = InputEvent::MouseMove {
                x: ms.pt.x as f32, // Note: Absolute coords for now
                y: ms.pt.y as f32,
            };
            if let Ok(guard) = GLOBAL_CALLBACK.lock() {
                if let Some(cb) = &*guard {
                    cb(event);
                }
            }
        }
    }
    CallNextHookEx(MOUSE_HOOK, code, wparam, lparam)
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
