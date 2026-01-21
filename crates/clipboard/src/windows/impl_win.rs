use crate::traits::ClipboardProvider;
use anyhow::{Result, anyhow};
use std::ffi::c_void;
use std::sync::{Arc, Mutex, Once};
use std::thread;
use windows::core::{PCWSTR, w, HRESULT};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM, HANDLE, HGLOBAL, HINSTANCE, HMODULE, GlobalFree};
use windows::Win32::System::DataExchange::{
    OpenClipboard, CloseClipboard, EmptyClipboard, SetClipboardData, GetClipboardData,
    AddClipboardFormatListener, RemoveClipboardFormatListener,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, RegisterClassW,
    CS_DBLCLKS, MSG, WNDCLASSW, WS_OVERLAPPEDWINDOW, WM_CLIPBOARDUPDATE, WM_DESTROY,
    HMENU, WINDOW_EX_STYLE,
};

const CF_UNICODETEXT: u32 = 13;

static REGISTER_CLASS: Once = Once::new();
static GLOBAL_CALLBACK: Mutex<Option<Box<dyn Fn() + Send + Sync>>> = Mutex::new(None);

pub struct WindowsClipboard;

impl WindowsClipboard {
    pub fn new() -> Self {
        Self
    }
}

impl ClipboardProvider for WindowsClipboard {
    fn get_text(&self) -> Result<String> {
        unsafe {
            OpenClipboard(HWND(0)).map_err(|e| anyhow!("Failed to open clipboard: {}", e))?;
            
            struct Closer;
            impl Drop for Closer {
                fn drop(&mut self) { unsafe { let _ = CloseClipboard(); } }
            }
            let _closer = Closer;

            let handle = GetClipboardData(CF_UNICODETEXT).map_err(|e| anyhow!("GetClipboardData failed: {}", e))?;
            if handle.is_invalid() {
                return Ok(String::new());
            }

            let hglobal = HGLOBAL(handle.0 as *mut _);
            let ptr = GlobalLock(hglobal);
            if ptr.is_null() {
                return Err(anyhow!("GlobalLock failed"));
            }

            let mut len = 0;
            let ptr_u16 = ptr as *const u16;
            while *ptr_u16.add(len) != 0 {
                len += 1;
            }
            let slice = std::slice::from_raw_parts(ptr_u16, len);
            let s = String::from_utf16_lossy(slice);
            
            let _ = GlobalUnlock(hglobal).is_ok();
            Ok(s)
        }
    }

    fn set_text(&self, text: String) -> Result<()> {
        unsafe {
            OpenClipboard(HWND(0)).map_err(|e| anyhow!("Failed to open clipboard: {}", e))?;
            
            struct Closer;
            impl Drop for Closer {
                fn drop(&mut self) { unsafe { let _ = CloseClipboard(); } }
            }
            let _closer = Closer;

            EmptyClipboard().map_err(|e| anyhow!("EmptyClipboard failed: {}", e))?;

            let utf16: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
            let size = utf16.len() * 2;
            
            let hglobal = GlobalAlloc(GMEM_MOVEABLE, size).map_err(|e| anyhow!("GlobalAlloc failed: {}", e))?;
            if hglobal.is_invalid() {
                return Err(anyhow!("GlobalAlloc returned invalid handle"));
            }

            let ptr = GlobalLock(hglobal);
            if ptr.is_null() {
                let _ = GlobalFree(hglobal);
                return Err(anyhow!("GlobalLock failed"));
            }

            std::ptr::copy_nonoverlapping(utf16.as_ptr(), ptr as *mut u16, utf16.len());

            let _ = GlobalUnlock(hglobal).is_ok();

            let handle = HANDLE(hglobal.0 as isize);
            if let Err(e) = SetClipboardData(CF_UNICODETEXT, handle) {
                let _ = GlobalFree(hglobal);
                return Err(anyhow!("SetClipboardData failed: {}", e));
            }
            
            Ok(())
        }
    }

    fn start_listener(&self, callback: Box<dyn Fn() + Send + Sync>) -> Result<()> {
        {
            let mut guard = GLOBAL_CALLBACK.lock().unwrap();
            *guard = Some(callback);
        }

        thread::spawn(|| unsafe {
            let h_module = GetModuleHandleW(None).unwrap_or(HMODULE(0));
            let h_instance = HINSTANCE(h_module.0);
            let window_class_name = w!("PlatformPasserClipboardListener");
            
            REGISTER_CLASS.call_once(|| {
                let wc = WNDCLASSW {
                    hCursor: Default::default(),
                    hIcon: Default::default(),
                    lpszMenuName: PCWSTR::null(),
                    lpszClassName: window_class_name,
                    lpfnWndProc: Some(wnd_proc),
                    hInstance: h_instance,
                    style: CS_DBLCLKS,
                    ..Default::default()
                };
                let _ = RegisterClassW(&wc);
            });

            // Create message-only window
            let title = w!("ClipboardListener");
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                window_class_name,
                title,
                WS_OVERLAPPEDWINDOW,
                0, 0, 0, 0,
                HWND(0),
                HMENU(0),
                h_instance,
                None,
            );

            if hwnd.0 == 0 {
                return;
            }

            let _ = AddClipboardFormatListener(hwnd);

            let mut msg = MSG::default();
            while GetMessageW(&mut msg, HWND(0), 0, 0).into() {
                let _ = DispatchMessageW(&msg);
            }
            
            let _ = RemoveClipboardFormatListener(hwnd);
        });
        
        Ok(())
    }
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_CLIPBOARDUPDATE => {
            if let Ok(guard) = GLOBAL_CALLBACK.lock() {
                if let Some(cb) = &*guard {
                    cb();
                }
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            windows::Win32::UI::WindowsAndMessaging::PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
