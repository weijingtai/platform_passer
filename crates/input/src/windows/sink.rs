use crate::InputSink;
use anyhow::{Result, anyhow};
use platform_passer_core::InputEvent;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, VIRTUAL_KEY,
    KEYBDINPUT, MOUSEINPUT, KEYEVENTF_KEYUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_ABSOLUTE,
};
use std::mem::size_of;

pub struct WindowsInputSink;

impl WindowsInputSink {
    pub fn new() -> Self {
        Self
    }
}

impl InputSink for WindowsInputSink {
    fn inject_event(&self, event: InputEvent) -> Result<()> {
        let mut input = INPUT::default();
        
        match event {
            InputEvent::Keyboard { key_code, is_down } => {
                input.r#type = INPUT_KEYBOARD;
                let mut flags = Default::default();
                if !is_down {
                    flags |= KEYEVENTF_KEYUP;
                }
                input.Anonymous.ki = KEYBDINPUT {
                    wVk: VIRTUAL_KEY(key_code as u16),
                    wScan: 0,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                };
            }
            InputEvent::MouseMove { x, y } => {
                input.r#type = INPUT_MOUSE;
                // Note: Coordinates need to be mapped to 0-65535 for ABSOLUTE
                input.Anonymous.mi = MOUSEINPUT {
                    dx: (x * 65535.0) as i32,
                    dy: (y * 65535.0) as i32,
                    mouseData: 0,
                    dwFlags: MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE,
                    time: 0,
                    dwExtraInfo: 0,
                };
            }
            InputEvent::MouseButton { .. } => {
                // TODO: Implement button processing
                return Ok(()); 
            }
            InputEvent::Scroll { .. } => {
               // TODO: Implement scroll processing
               return Ok(());
            }
        }

        unsafe {
            if SendInput(&[input], size_of::<INPUT>() as i32) == 0 {
                return Err(anyhow!("SendInput failed"));
            }
        }
        Ok(())
    }
}
