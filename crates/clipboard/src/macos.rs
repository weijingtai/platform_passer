use crate::ClipboardProvider;
use anyhow::{Result, anyhow};
use cocoa::base::{id, nil};
use cocoa::foundation::{NSString, NSAutoreleasePool};
use objc::{msg_send, sel, sel_impl, runtime::Object};

pub struct MacosClipboard;

impl MacosClipboard {
    pub fn new() -> Self {
        Self
    }
}

impl ClipboardProvider for MacosClipboard {
    fn get_text(&self) -> Result<String> {
        unsafe {
            let _pool = NSAutoreleasePool::new(nil);
            let ns_pasteboard: id = msg_send![objc::class!(NSPasteboard), generalPasteboard];
            let ns_string: id = msg_send![ns_pasteboard, stringForType: cocoa::foundation::NSPasteboardTypeString];
            
            if ns_string == nil {
                return Ok(String::new());
            }
            
            let char_ptr: *const libc::c_char = msg_send![ns_string, UTF8String];
            let c_str = std::ffi::CStr::from_ptr(char_ptr);
            Ok(c_str.to_string_lossy().into_owned())
        }
    }

    fn set_text(&self, text: String) -> Result<()> {
        unsafe {
            let _pool = NSAutoreleasePool::new(nil);
            let ns_pasteboard: id = msg_send![objc::class!(NSPasteboard), generalPasteboard];
            let ns_string = NSString::alloc(nil).init_str(&text);
            
            let _: () = msg_send![ns_pasteboard, clearContents];
            let _: bool = msg_send![ns_pasteboard, setString:ns_string forType: cocoa::foundation::NSPasteboardTypeString];
            Ok(())
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
                        callback();
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
        });
        Ok(())
    }
}
