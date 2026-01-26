use crate::InputSink;
use anyhow::Result;
use platform_passer_core::{InputEvent, MouseButton};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, 
    MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_MOVE, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP,
    MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP,
    MOUSEEVENTF_WHEEL, MOUSEEVENTF_HWHEEL,
    KEYEVENTF_KEYUP, VIRTUAL_KEY,
};
use std::mem::size_of;
use platform_passer_core::config::AppConfig;

pub struct WindowsInputSink;

impl WindowsInputSink {
    pub fn new() -> Self {
        Self
    }
}

impl InputSink for WindowsInputSink {
    fn inject_event(&self, event: InputEvent) -> Result<()> {
        match event {
            InputEvent::MouseMove { x, y } => {
                let dx = (x * 65535.0) as i32;
                let dy = (y * 65535.0) as i32;
                let mut input = INPUT {
                    r#type: INPUT_MOUSE,
                    Anonymous: INPUT_0 {
                        mi: windows::Win32::UI::Input::KeyboardAndMouse::MOUSEINPUT {
                            dx,
                            dy,
                            mouseData: 0,
                            dwFlags: MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_MOVE,
                            time: 0,
                            dwExtraInfo: 0,
                        },
                    },
                };
                unsafe { SendInput(&[input], size_of::<INPUT>() as i32) };
            }
            InputEvent::MouseButton { button, is_down } => {
                let flags = match button {
                    MouseButton::Left => if is_down { MOUSEEVENTF_LEFTDOWN } else { MOUSEEVENTF_LEFTUP },
                    MouseButton::Right => if is_down { MOUSEEVENTF_RIGHTDOWN } else { MOUSEEVENTF_RIGHTUP },
                    MouseButton::Middle => if is_down { MOUSEEVENTF_MIDDLEDOWN } else { MOUSEEVENTF_MIDDLEUP },
                };
                let mut input = INPUT {
                    r#type: INPUT_MOUSE,
                    Anonymous: INPUT_0 {
                        mi: windows::Win32::UI::Input::KeyboardAndMouse::MOUSEINPUT {
                            dx: 0,
                            dy: 0,
                            mouseData: 0,
                            dwFlags: flags,
                            time: 0,
                            dwExtraInfo: 0,
                        },
                    },
                };
                unsafe { SendInput(&[input], size_of::<INPUT>() as i32) };
            }
            InputEvent::Scroll { dx, dy } => {
                if dy != 0.0 {
                    let mut input = INPUT {
                        r#type: INPUT_MOUSE,
                        Anonymous: INPUT_0 {
                            mi: windows::Win32::UI::Input::KeyboardAndMouse::MOUSEINPUT {
                                dx: 0,
                                dy: 0,
                                mouseData: (dy * 120.0) as i32 as u32,
                                dwFlags: MOUSEEVENTF_WHEEL,
                                time: 0,
                                dwExtraInfo: 0,
                            },
                        },
                    };
                    unsafe { SendInput(&[input], size_of::<INPUT>() as i32) };
                }
                if dx != 0.0 {
                    let mut input = INPUT {
                        r#type: INPUT_MOUSE,
                        Anonymous: INPUT_0 {
                            mi: windows::Win32::UI::Input::KeyboardAndMouse::MOUSEINPUT {
                                dx: 0,
                                dy: 0,
                                mouseData: (dx * 120.0) as i32 as u32,
                                dwFlags: MOUSEEVENTF_HWHEEL,
                                time: 0,
                                dwExtraInfo: 0,
                            },
                        },
                    };
                    unsafe { SendInput(&[input], size_of::<INPUT>() as i32) };
                }
            }
            InputEvent::Keyboard { key_code, is_down } => {
                use windows::Win32::UI::Input::KeyboardAndMouse::KEYBD_EVENT_FLAGS;
                let flags = if is_down { KEYBD_EVENT_FLAGS(0) } else { KEYEVENTF_KEYUP };
                let mut input = INPUT {
                    r#type: INPUT_KEYBOARD,
                    Anonymous: INPUT_0 {
                        ki: windows::Win32::UI::Input::KeyboardAndMouse::KEYBDINPUT {
                            wVk: VIRTUAL_KEY(key_code as u16),
                            wScan: 0,
                            dwFlags: flags,
                            time: 0,
                            dwExtraInfo: 0,
                        },
                    },
                };
                unsafe { SendInput(&[input], size_of::<INPUT>() as i32) };
            }
            _ => {}
        }
        Ok(())
    }

    fn reset_input(&self) -> Result<()> {
        // Reset logical state if needed?
        // Mostly handled by OS, but could explicitly release held keys?
        Ok(())
    }

    fn update_config(&self, _config: AppConfig) -> Result<()> {
        Ok(())
    }
}

pub fn force_release_modifiers() {
    use windows::Win32::UI::Input::KeyboardAndMouse::{keybd_event, VK_CONTROL, VK_LWIN, VK_MENU, VK_SHIFT, KEYEVENTF_KEYUP, KEYBD_EVENT_FLAGS};
    unsafe {
        // Force send KeyUp for common modifiers
        // 0 = no scan code needed for virtual keys usually in this legacy API, but safe to pass 0
        keybd_event(VK_CONTROL.0 as u8, 0, KEYBD_EVENT_FLAGS(KEYEVENTF_KEYUP.0), 0);
        keybd_event(VK_MENU.0 as u8, 0, KEYBD_EVENT_FLAGS(KEYEVENTF_KEYUP.0), 0);
        keybd_event(VK_SHIFT.0 as u8, 0, KEYBD_EVENT_FLAGS(KEYEVENTF_KEYUP.0), 0);
        keybd_event(VK_LWIN.0 as u8, 0, KEYBD_EVENT_FLAGS(KEYEVENTF_KEYUP.0), 0);
    }
}
