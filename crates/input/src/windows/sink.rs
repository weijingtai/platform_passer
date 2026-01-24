use crate::InputSink;
use anyhow::{Result, anyhow};
use platform_passer_core::{InputEvent, MouseButton};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_KEYBOARD, INPUT_MOUSE, VIRTUAL_KEY,
    KEYBDINPUT, MOUSEINPUT, KEYEVENTF_KEYUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_ABSOLUTE,
    MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP,
    MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP,
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
                // Normalized (0-1) to Windows absolute (0-65535)
                input.Anonymous.mi = MOUSEINPUT {
                    dx: (x * 65535.0) as i32,
                    dy: (y * 65535.0) as i32,
                    mouseData: 0,
                    dwFlags: MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE,
                    time: 0,
                    dwExtraInfo: 0,
                };
            }
            InputEvent::MouseButton { button, is_down } => {
                input.r#type = INPUT_MOUSE;
                let flags = match (button, is_down) {
                    (MouseButton::Left, true) => MOUSEEVENTF_LEFTDOWN,
                    (MouseButton::Left, false) => MOUSEEVENTF_LEFTUP,
                    (MouseButton::Right, true) => MOUSEEVENTF_RIGHTDOWN,
                    (MouseButton::Right, false) => MOUSEEVENTF_RIGHTUP,
                    (MouseButton::Middle, true) => MOUSEEVENTF_MIDDLEDOWN,
                    (MouseButton::Middle, false) => MOUSEEVENTF_MIDDLEUP,
                };
                input.Anonymous.mi = MOUSEINPUT {
                    dx: 0,
                    dy: 0,
                    mouseData: 0,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                };
            }
            InputEvent::Scroll { .. } => {
                // TODO: Implement scroll processing
                return Ok(());
            }
            InputEvent::ScreenSwitch(_) => {
                // Sinks don't handle screen switches directly yet
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
