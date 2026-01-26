use crate::ClipboardProvider;
use anyhow::Result;
use arboard::{Clipboard, ImageData};
use std::borrow::Cow;
use image::ImageOutputFormat;
use anyhow::anyhow;
// Keep other imports for listener if needed, but we can potentially replace get/set with arboard too?
// For consistency, let's keep listener native and get/set via arboard.
use cocoa::base::{id, nil};
use cocoa::foundation::{NSString, NSAutoreleasePool};
use objc::{msg_send, sel, sel_impl};
use std::ffi::CStr;

pub fn nsstring_to_string(ns_string: id) -> String {
    unsafe {
        let utf8: *const i8 = msg_send![ns_string, UTF8String];
        if utf8.is_null() {
            String::new()
        } else {
            CStr::from_ptr(utf8).to_string_lossy().into_owned()
        }
    }
}

pub struct MacosClipboard;

impl MacosClipboard {
    pub fn new() -> Self {
        Self
    }
}

impl ClipboardProvider for MacosClipboard {
    fn get_text(&self) -> Result<String> {
        let mut clipboard = Clipboard::new().map_err(|e| anyhow!("Failed to init clipboard: {}", e))?;
        clipboard.get_text().map_err(|e| anyhow!("Failed to get text: {}", e))
    }

    fn set_text(&self, text: String) -> Result<()> {
        let mut clipboard = Clipboard::new().map_err(|e| anyhow!("Failed to init clipboard: {}", e))?;
        clipboard.set_text(text).map_err(|e| anyhow!("Failed to set text: {}", e))
    }

    fn get_image(&self) -> Result<Option<Vec<u8>>> {
        let mut clipboard = Clipboard::new().map_err(|e| anyhow!("Init failed: {}", e))?;
        if let Ok(image) = clipboard.get_image() {
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
            let _pool = NSAutoreleasePool::new(nil);
            let ns_pasteboard: id = msg_send![objc::class!(NSPasteboard), generalPasteboard];
            
            // Define classes: [NSURL class]
            let ns_url_class: id = msg_send![objc::class!(NSURL), class];
            let classes: id = msg_send![objc::class!(NSArray), arrayWithObject:ns_url_class];
            
            // Define options: Empty dict
            let options: id = msg_send![objc::class!(NSDictionary), dictionary];
            
            let urls: id = msg_send![ns_pasteboard, readObjectsForClasses:classes options:options];
            
            if urls == nil {
                return Ok(None);
            }
            
            let count: u64 = msg_send![urls, count];
            if count == 0 {
                return Ok(None);
            }
            
            let mut file_paths = Vec::new();
            for i in 0..count {
                let url: id = msg_send![urls, objectAtIndex:i];
                // Check if file URL: [url isFileURL]
                let is_file: bool = msg_send![url, isFileURL];
                if is_file {
                    let path: id = msg_send![url, path]; // NSString
                    let path_str = nsstring_to_string(path);
                    if !path_str.is_empty() {
                         file_paths.push(path_str);
                    }
                }
            }
            
            if file_paths.is_empty() {
                Ok(None)
            } else {
                Ok(Some(file_paths))
            }
        }
    }

    fn set_files(&self, files: Vec<String>) -> Result<()> {
        unsafe {
            let _pool = NSAutoreleasePool::new(nil);
            let ns_pasteboard: id = msg_send![objc::class!(NSPasteboard), generalPasteboard];
            
            // [pasteboard clearContents]
            let _: isize = msg_send![ns_pasteboard, clearContents];
            
            // Create array of NSURLs
            // let ns_files: id = NSMutableArray::arrayWithCapacity(nil, files.len() as u64);
            let ns_files: id = msg_send![objc::class!(NSMutableArray), arrayWithCapacity:files.len()];
            
            for file_path in files {
                let ns_path = NSString::alloc(nil).init_str(&file_path);
                // [NSURL fileURLWithPath:path]
                let url: id = msg_send![objc::class!(NSURL), fileURLWithPath:ns_path];
                let _: () = msg_send![ns_files, addObject:url];
                // Release temporary NSString? It's autoreleased usually but better safe if looped.
                // Assuming standard autorelease pool.
            }
            
            // [pasteboard writeObjects:files]
            let success: bool = msg_send![ns_pasteboard, writeObjects:ns_files];
             
            if success {
                Ok(())
            } else {
                Err(anyhow!("Failed to write files to pasteboard"))
            }
        }
    }

    fn start_listener(&self, callback: Box<dyn Fn() + Send + Sync>) -> Result<()> {
        // Polling implementation for MVP
        let callback = std::sync::Arc::new(callback);
        std::thread::spawn(move || {
            let mut last_count: isize = 0;
            loop {
                unsafe {
                    let _pool = NSAutoreleasePool::new(nil);
                    let ns_pasteboard: id = msg_send![objc::class!(NSPasteboard), generalPasteboard];
                    let change_count: isize = msg_send![ns_pasteboard, changeCount];
                    
                    if change_count != last_count {
                        last_count = change_count;
                        // println!("[Clipboard] Detected macOS clipboard change (count: {})", change_count);
                        callback();
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
        });
        Ok(())
    }
}
