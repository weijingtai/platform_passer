use crate::traits::ClipboardProvider;
use anyhow::{Result, anyhow};
use std::sync::{Mutex, Once};
use std::thread;
use windows::core::{PCWSTR, w};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM, HANDLE, HINSTANCE, HMODULE, GlobalFree};
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
use windows::Win32::UI::Shell::{DragQueryFileW, HDROP, DROPFILES};

const CF_HDROP: u32 = 15;

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

    fn get_files(&self) -> Result<Option<Vec<String>>> {
        unsafe {
            if OpenClipboard(HWND(0)).is_err() {
                return Err(anyhow!("Failed to open clipboard"));
            }
            let h_data = GetClipboardData(CF_HDROP).unwrap_or(HANDLE(0));
            if h_data.0 == 0 {
                let _ = CloseClipboard();
                return Ok(None);
            }
            
            let h_drop = HDROP(h_data.0);
            let count = DragQueryFileW(h_drop, 0xFFFFFFFF, None);
            let mut files = Vec::new();
            
            for i in 0..count {
                let len = DragQueryFileW(h_drop, i, None);
                let mut buffer = vec![0u16; len as usize + 1];
                DragQueryFileW(h_drop, i, Some(&mut buffer));
                // Remove null terminator for String conversion
                if let Ok(path) = String::from_utf16(&buffer[..len as usize]) {
                    files.push(path);
                }
            }
            
            let _ = CloseClipboard();
            Ok(Some(files))
        }
    }

    fn set_files(&self, files: Vec<String>) -> Result<()> {
        IGNORE_EVENTS.fetch_add(1, Ordering::SeqCst);
        unsafe {
            if OpenClipboard(HWND(0)).is_err() {
                return Err(anyhow!("Failed to open clipboard"));
            }
            let _ = EmptyClipboard();
            
            let mut total_size = std::mem::size_of::<DROPFILES>();
            let mut paths_wide = Vec::new();
            for file in files {
                let mut wide: Vec<u16> = file.encode_utf16().collect();
                wide.push(0);
                total_size += wide.len() * 2;
                paths_wide.push(wide);
            }
            total_size += 2; // Final double null
            
            let h_global = GlobalAlloc(GMEM_MOVEABLE, total_size).map_err(|e| anyhow!("GlobalAlloc failed: {}", e))?;
            let ptr = GlobalLock(h_global);
            if ptr.is_null() {
                 let _ = GlobalFree(h_global);
                 let _ = CloseClipboard();
                 return Err(anyhow!("GlobalLock failed"));
            }
            
            let dropfiles = DROPFILES {
                pFiles: std::mem::size_of::<DROPFILES>() as u32,
                pt: windows::Win32::Foundation::POINT { x: 0, y: 0 },
                fNC: windows::Win32::Foundation::BOOL(0),
                fWide: windows::Win32::Foundation::BOOL(1),
            };
            
            std::ptr::copy_nonoverlapping(&dropfiles, ptr as *mut DROPFILES, 1);
            let mut offset = std::mem::size_of::<DROPFILES>();
            for wide in paths_wide {
                std::ptr::copy_nonoverlapping(wide.as_ptr(), (ptr as usize + offset) as *mut u16, wide.len());
                offset += wide.len() * 2;
            }
            // Double null at expiration
            std::ptr::write_bytes((ptr as usize + offset) as *mut u8, 0, 2);
            
            let _ = GlobalUnlock(h_global);
            
            if let Err(e) = SetClipboardData(CF_HDROP, HANDLE(h_global.0 as isize)) {
                let _ = GlobalFree(h_global);
                let _ = CloseClipboard();
                return Err(anyhow!("SetClipboardData failed: {}", e));
            }
            
            let _ = CloseClipboard();
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

            // Create message-only window (HWND_MESSAGE = -3)
            let title = w!("ClipboardListener");
            let hwnd_message = HWND(-3isize as isize); 
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                window_class_name,
                title,
                windows::Win32::UI::WindowsAndMessaging::WS_POPUP, // Use POPUP instead of OVERLAPPEDWINDOW
                0, 0, 0, 0,
                hwnd_message, // Parent = HWND_MESSAGE
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
