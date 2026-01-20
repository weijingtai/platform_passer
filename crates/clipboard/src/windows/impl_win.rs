use crate::trait_def::ClipboardProvider;
use anyhow::{Result, anyhow, Context};
use std::ffi::c_void;
use std::sync::{Arc, Mutex, Once};
use std::thread;
use windows::core::{PCSTR, s};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM, HANDLE, GlobalLock, GlobalUnlock, GlobalFree};
use windows::Win32::System::DataExchange::{
    OpenClipboard, CloseClipboard, EmptyClipboard, SetClipboardData, GetClipboardData,
    AddClipboardFormatListener, RemoveClipboardFormatListener, CF_TEXT,
};
use windows::Win32::System::Memory::{GlobalAlloc, GMEM_MOVEABLE, GMEM_DDESHARE};
use windows::Win32::System::LibraryLoader::GetModuleHandleA;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExA, DefWindowProcA, DispatchMessageA, GetMessageA, RegisterClassA,
    CS_DBLCLKS, MSG, WNDCLASSA, WS_OVERLAPPEDWINDOW, WM_CLIPBOARDUPDATE, WM_DESTROY,
};

// Re-export trait for internal use if needed, or just use crate::traits
use crate::traits::ClipboardProvider as ClipboardTrait;

static REGISTER_CLASS: Once = Once::new();
static WINDOW_CLASS_NAME: PCSTR = s!("PlatformPasserClipboardListener");
static GLOBAL_CALLBACK: Mutex<Option<Box<dyn Fn() + Send + Sync>>> = Mutex::new(None);

pub struct WindowsClipboard;

impl WindowsClipboard {
    pub fn new() -> Self {
        Self
    }
}

impl ClipboardTrait for WindowsClipboard {
    fn get_text(&self) -> Result<String> {
        unsafe {
            if !OpenClipboard(HWND(0)).as_bool() {
                return Err(anyhow!("Failed to open clipboard"));
            }
            
            // Defers CloseClipboard
            struct Closer;
            impl Drop for Closer {
                fn drop(&mut self) { unsafe { CloseClipboard(); } }
            }
            let _closer = Closer;

            let handle = GetClipboardData(CF_TEXT.0);
            if handle.is_invalid() {
                // Not text or empty
                return Ok(String::new());
            }

            let ptr = GlobalLock(handle);
            if ptr.is_null() {
                return Err(anyhow!("GlobalLock failed"));
            }

            let c_str = std::ffi::CStr::from_ptr(ptr as *const i8);
            let s = c_str.to_string_lossy().into_owned();
            
            GlobalUnlock(handle);
            Ok(s)
        }
    }

    fn set_text(&self, text: String) -> Result<()> {
        unsafe {
            if !OpenClipboard(HWND(0)).as_bool() {
                return Err(anyhow!("Failed to open clipboard"));
            }
            
             struct Closer;
            impl Drop for Closer {
                fn drop(&mut self) { unsafe { CloseClipboard(); } }
            }
            let _closer = Closer;

            EmptyClipboard();

            // Allocate global memory
            // +1 for null terminator
            let len = text.len() + 1;
            let handle = GlobalAlloc(GMEM_MOVEABLE, len);
            if handle.is_invalid() {
                return Err(anyhow!("GlobalAlloc failed"));
            }

            let ptr = GlobalLock(handle);
            if ptr.is_null() {
                GlobalFree(handle);
                return Err(anyhow!("GlobalLock failed"));
            }

            // Copy data
            std::ptr::copy_nonoverlapping(text.as_ptr(), ptr as *mut u8, text.len());
            *(ptr as *mut u8).add(text.len()) = 0;

            GlobalUnlock(handle);

            if SetClipboardData(CF_TEXT.0, handle).is_invalid() {
                GlobalFree(handle);
                return Err(anyhow!("SetClipboardData failed"));
            }
            
            // System now owns the memory
            Ok(())
        }
    }

    fn start_listener(&self, callback: Box<dyn Fn() + Send + Sync>) -> Result<()> {
        {
            let mut guard = GLOBAL_CALLBACK.lock().unwrap();
            *guard = Some(callback);
        }

        thread::spawn(|| unsafe {
            let h_instance = GetModuleHandleA(None).unwrap();
            
            REGISTER_CLASS.call_once(|| {
                let wc = WNDCLASSA {
                    hCursor: Default::default(),
                    hIcon: Default::default(),
                    lpszMenuName: PCSTR::null(),
                    lpszClassName: WINDOW_CLASS_NAME,
                    lpfnWndProc: Some(wnd_proc),
                    hInstance: h_instance,
                    style: CS_DBLCLKS,
                    ..Default::default()
                };
                RegisterClassA(&wc);
            });

            // Create message-only window (HWND_MESSAGE = -3 as pointer is not easy here, use Parent=0 for now invisible)
            let hwnd = CreateWindowExA(
                Default::default(),
                WINDOW_CLASS_NAME,
                s!("ClipboardListener"),
                WS_OVERLAPPEDWINDOW,
                0, 0, 0, 0,
                HWND(0),
                Default::default(),
                h_instance,
                None,
            );

            if hwnd.0 == 0 {
                return;
            }

            AddClipboardFormatListener(hwnd);

            let mut msg = MSG::default();
            while GetMessageA(&mut msg, HWND(0), 0, 0).into() {
                DispatchMessageA(&msg);
            }
            
            RemoveClipboardFormatListener(hwnd);
        });
        
        Ok(())
    }
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_CLIPBOARDUPDATE => {
            if let Ok(guard) = GLOBAL_CALLBACK.lock() {
                if let Some(cb) = &*guard {
                    // Logic to avoid loop feedback? 
                    // For now, naive implementation: just notify.
                    // Verification phase will handle "don't re-send what we just received".
                    cb();
                }
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            windows::Win32::UI::WindowsAndMessaging::PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcA(hwnd, msg, wparam, lparam),
    }
}
