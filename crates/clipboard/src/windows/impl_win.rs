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
use arboard::{Clipboard, ImageData};
use std::borrow::Cow;
use image::ImageOutputFormat;

const CF_UNICODETEXT: u32 = 13;

static REGISTER_CLASS: Once = Once::new();
static GLOBAL_CALLBACK: Mutex<Option<Box<dyn Fn() + Send + Sync>>> = Mutex::new(None);
use std::sync::atomic::{AtomicUsize, Ordering};
static IGNORE_EVENTS: AtomicUsize = AtomicUsize::new(0);

pub struct WindowsClipboard;

impl WindowsClipboard {
    pub fn new() -> Self {
        Self
    }
}

impl ClipboardProvider for WindowsClipboard {
    fn get_text(&self) -> Result<String> {
        let mut clipboard = Clipboard::new().map_err(|e| anyhow!("Failed to init clipboard: {}", e))?;
        clipboard.get_text().map_err(|e| anyhow!("Failed to get text: {}", e))
    }

    fn set_text(&self, text: String) -> Result<()> {
        IGNORE_EVENTS.fetch_add(1, Ordering::SeqCst);
        let mut clipboard = Clipboard::new().map_err(|e| anyhow!("Failed to init clipboard: {}", e))?;
        clipboard.set_text(text).map_err(|e| anyhow!("Failed to set text: {}", e))
    }

    fn get_image(&self) -> Result<Option<Vec<u8>>> {
        let mut clipboard = Clipboard::new().map_err(|e| anyhow!("Init failed: {}", e))?;
        if let Ok(image) = clipboard.get_image() {
            // Convert RGBA to PNG
            let mut buf = Vec::new();
            let safe_image = image::ImageBuffer::<image::Rgba<u8>, _>::from_raw(
                image.width as u32, 
                image.height as u32, 
                image.bytes.into_owned()
            ).ok_or(anyhow!("Invalid image buffer"))?;
            
            let mut cursor = std::io::Cursor::new(&mut buf);
            safe_image.write_to(&mut cursor, ImageOutputFormat::Png)?;
            Ok(Some(buf))
        } else {
            Ok(None)
        }
    }

    fn set_image(&self, png_data: Vec<u8>) -> Result<()> {
        IGNORE_EVENTS.fetch_add(1, Ordering::SeqCst);
        let mut clipboard = Clipboard::new().map_err(|e| anyhow!("Init failed: {}", e))?;
        let img = image::load_from_memory(&png_data)?.to_rgba8();
        let width = img.width() as usize;
        let height = img.height() as usize;
        let raw = img.into_raw();
        
        let image_data = ImageData {
            width,
            height,
            bytes: Cow::from(raw),
        };
        clipboard.set_image(image_data).map_err(|e| anyhow!("Set image failed: {}", e))?;
        Ok(())
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
                // println!("[Clipboard] Received message: {:?}", msg.message);
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
            if IGNORE_EVENTS.load(Ordering::SeqCst) > 0 {
                IGNORE_EVENTS.fetch_sub(1, Ordering::SeqCst);
                // println!("[Clipboard] Ignored clipboard event (internal update)");
                return LRESULT(0);
            }
            // println!("[Clipboard] Detected clipboard change");
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
