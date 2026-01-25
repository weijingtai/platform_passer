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
