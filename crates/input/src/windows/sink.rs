use crate::InputSink;
use anyhow::{Result, anyhow};
use platform_passer_core::{InputEvent, MouseButton};
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::Win32::Foundation::*;
use std::sync::Mutex;
use platform_passer_core::config::AppConfig;
use std::mem::size_of;
use std::collections::HashSet;

pub struct WindowsInputSink {
    pressed_keys: Mutex<HashSet<u16>>,
    pressed_buttons: Mutex<HashSet<u32>>,
}

impl WindowsInputSink {
    pub fn new() -> Self {
        Self {
            pressed_keys: Mutex::new(HashSet::new()),
            pressed_buttons: Mutex::new(HashSet::new()),
        }
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
                
                let vk = key_code as u16;
                if let Ok(mut keys) = self.pressed_keys.lock() {
                    if is_down {
                        keys.insert(vk);
                    } else {
                        keys.remove(&vk);
                    }
                }

                input.Anonymous.ki = KEYBDINPUT {
                    wVk: VIRTUAL_KEY(vk),
                    wScan: 0,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                };
            }
            InputEvent::MouseMove { x, y } => {
                // Mapping 0.0..1.0 to 0..65535 for MOUSEEVENTF_ABSOLUTE
                // This covers the entire Virtual Screen (primary + secondary monitors)
                let abs_x = (x * 65535.0) as i32;
                let abs_y = (y * 65535.0) as i32;

                input.r#type = INPUT_MOUSE;
                input.Anonymous.mi = MOUSEINPUT {
                    dx: abs_x,
                    dy: abs_y,
                    mouseData: 0,
                    dwFlags: MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSE_EVENT_FLAGS(0x4000), // VIRTUAL_DESK
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

                // Track button state
                let btn_flag = match button {
                    MouseButton::Left => 1, // Custom ID logic
                    MouseButton::Right => 2,
                    MouseButton::Middle => 3,
                };
                
                if let Ok(mut btns) = self.pressed_buttons.lock() {
                    if is_down {
                        btns.insert(btn_flag);
                    } else {
                        btns.remove(&btn_flag);
                    }
                }

                input.Anonymous.mi = MOUSEINPUT {
                    dx: 0,
                    dy: 0,
                    mouseData: 0,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                };
            }
            InputEvent::Scroll { dx, dy } => {
                
                // Vertical scroll
                if dy.abs() > 0.0 {
                    let mut v_input = INPUT::default();
                    v_input.r#type = INPUT_MOUSE;
                    v_input.Anonymous.mi = MOUSEINPUT {
                        dx: 0,
                        dy: 0,
                        mouseData: (dy * 120.0) as i32 as u32, // WHEEL_DELTA = 120, cast to i32 then bit-cast to u32
                        dwFlags: MOUSEEVENTF_WHEEL,
                        time: 0,
                        dwExtraInfo: 0,
                    };
                    unsafe { SendInput(&[v_input], size_of::<INPUT>() as i32); }
                }

                // Horizontal scroll
                if dx.abs() > 0.0 {
                    let mut h_input = INPUT::default();
                    h_input.r#type = INPUT_MOUSE;
                    h_input.Anonymous.mi = MOUSEINPUT {
                        dx: 0,
                        dy: 0,
                        mouseData: (dx * 120.0) as i32 as u32,
                        dwFlags: MOUSEEVENTF_HWHEEL,
                        time: 0,
                        dwExtraInfo: 0,
                    };
                    unsafe { SendInput(&[h_input], size_of::<INPUT>() as i32); }
                }
                
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

    fn update_config(&self, _config: AppConfig) -> Result<()> {
        Ok(())
    }

    fn reset_input(&self) -> Result<()> {
        let mut inputs = Vec::new();

        // Release keys
        if let Ok(mut keys) = self.pressed_keys.lock() {
            for vk in keys.drain() {
                let mut input = INPUT::default();
                input.r#type = INPUT_KEYBOARD;
                input.Anonymous.ki = KEYBDINPUT {
                    wVk: VIRTUAL_KEY(vk),
                    wScan: 0,
                    dwFlags: KEYEVENTF_KEYUP,
                    time: 0,
                    dwExtraInfo: 0,
                };
                inputs.push(input);
            }
        }

        // Release mouse buttons
        if let Ok(mut btns) = self.pressed_buttons.lock() {
            for btn in btns.drain() {
                let flags = match btn {
                    1 => MOUSEEVENTF_LEFTUP,
                    2 => MOUSEEVENTF_RIGHTUP,
                    3 => MOUSEEVENTF_MIDDLEUP,
                    _ => continue,
                };
                
                let mut input = INPUT::default();
                input.r#type = INPUT_MOUSE;
                input.Anonymous.mi = MOUSEINPUT {
                    dx: 0,
                    dy: 0,
                    mouseData: 0,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                };
                inputs.push(input);
            }
        }

        if !inputs.is_empty() {
            unsafe {
                SendInput(&inputs, size_of::<INPUT>() as i32);
            }
        }

        Ok(())
    }
}

