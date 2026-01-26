use crate::InputSource;
use anyhow::Result;
use platform_passer_core::{InputEvent, ScreenSide};
use platform_passer_core::config::{AppConfig, ScreenPosition};
use std::sync::{Arc, Mutex};
use windows::Win32::Foundation::{LPARAM, WPARAM, LRESULT};
use windows::Win32::UI::WindowsAndMessaging::{
    SetWindowsHookExA, UnhookWindowsHookEx, CallNextHookEx, GetMessageA,
    WH_KEYBOARD_LL, WH_MOUSE_LL, HHOOK, KBDLLHOOKSTRUCT, MSLLHOOKSTRUCT, WM_KEYDOWN, WM_SYSKEYDOWN,
    WM_MOUSEMOVE, GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
    GetCursorPos, SetCursorPos,
};
use std::thread;
use std::sync::atomic::{AtomicBool, Ordering};

static IS_REMOTE: AtomicBool = AtomicBool::new(false);
static VIRTUAL_CURSOR_POS: Mutex<Option<(f32, f32)>> = Mutex::new(None);
static ACTIVE_REMOTE_POS: Mutex<Option<ScreenPosition>> = Mutex::new(None);

// Global callback storage
type HookCallback = Box<dyn Fn(InputEvent) + Send + Sync>;
static GLOBAL_CALLBACK: Mutex<Option<Arc<HookCallback>>> = Mutex::new(None);
static GLOBAL_CONFIG: Mutex<Option<AppConfig>> = Mutex::new(None);
static mut KEYBOARD_HOOK: HHOOK = HHOOK(0);
static mut MOUSE_HOOK: HHOOK = HHOOK(0);

// Cached metrics to avoid repeated GetSystemMetrics calls in the hot path
struct Metrics {
    left: i32,
    top: i32,
    width: i32,
    height: i32,
}
static CACHED_METRICS: Mutex<Option<Metrics>> = Mutex::new(None);

fn update_metrics() {
    unsafe {
        let left = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let top = GetSystemMetrics(SM_YVIRTUALSCREEN);
        let width = GetSystemMetrics(SM_CXVIRTUALSCREEN);
        let height = GetSystemMetrics(SM_CYVIRTUALSCREEN);
        if let Ok(mut guard) = CACHED_METRICS.lock() {
            *guard = Some(Metrics { left, top, width, height });
        }
    }
}

pub struct WindowsInputSource;

impl WindowsInputSource {
    pub fn new() -> Self {
        update_metrics();
        Self
    }
}

impl InputSource for WindowsInputSource {
    fn start_capture(&self, callback: Box<dyn Fn(InputEvent) + Send + Sync>) -> Result<()> {
        update_metrics();
        {
            let mut guard = GLOBAL_CALLBACK.lock().unwrap();
            *guard = Some(Arc::new(callback));
        }

        thread::spawn(|| unsafe {
             let h_instance = windows::Win32::System::LibraryLoader::GetModuleHandleA(None).unwrap();
             KEYBOARD_HOOK = SetWindowsHookExA(WH_KEYBOARD_LL, Some(keyboard_proc), h_instance, 0).unwrap();
             MOUSE_HOOK = SetWindowsHookExA(WH_MOUSE_LL, Some(mouse_proc), h_instance, 0).unwrap();

             let mut msg = Default::default();
             while GetMessageA(&mut msg, None, 0, 0).into() {
                 windows::Win32::UI::WindowsAndMessaging::TranslateMessage(&msg);
                 windows::Win32::UI::WindowsAndMessaging::DispatchMessageA(&msg);
             }
        });

        Ok(())
    }

    fn stop_capture(&self) -> Result<()> {
        unsafe {
            if KEYBOARD_HOOK.0 != 0 {
                let _ = UnhookWindowsHookEx(KEYBOARD_HOOK);
                KEYBOARD_HOOK = HHOOK::default();
            }
            if MOUSE_HOOK.0 != 0 {
                let _ = UnhookWindowsHookEx(MOUSE_HOOK);
                MOUSE_HOOK = HHOOK::default();
            }
        }
        Ok(())
    }

    fn set_remote(&self, remote: bool) -> Result<()> {
        IS_REMOTE.store(remote, Ordering::SeqCst);
        
        if remote {
            update_metrics();
            let metrics = CACHED_METRICS.lock().unwrap();
            if let Some(m) = &*metrics {
                unsafe {
                    let mut pt = windows::Win32::Foundation::POINT::default();
                    if GetCursorPos(&mut pt).is_ok() {
                        let abs_x = (pt.x - m.left) as f32 / m.width as f32;
                        let abs_y = (pt.y - m.top) as f32 / m.height as f32;
                        *VIRTUAL_CURSOR_POS.lock().unwrap() = Some((abs_x, abs_y));
                        
                        let center_x = m.left + m.width / 2;
                        let center_y = m.top + m.height / 2;
                        let _ = SetCursorPos(center_x, center_y);
                    }
                }
            }
        } else {
            *VIRTUAL_CURSOR_POS.lock().unwrap() = None;
            if let Ok(mut guard) = ACTIVE_REMOTE_POS.lock() { *guard = None; }
            update_metrics();
            let metrics = CACHED_METRICS.lock().unwrap();
            if let Some(m) = &*metrics {
                unsafe {
                    let mut pt = windows::Win32::Foundation::POINT::default();
                    if GetCursorPos(&mut pt).is_ok() {
                        let mut new_x = pt.x;
                        let mut new_y = pt.y;
                        if pt.x <= m.left + 1 { new_x += 50; }
                        if pt.x >= m.left + m.width - 1 { new_x -= 50; }
                        if pt.y <= m.top + 1 { new_y += 50; }
                        if pt.y >= m.top + m.height - 1 { new_y -= 50; }
                        if new_x != pt.x || new_y != pt.y {
                            let _ = SetCursorPos(new_x, new_y);
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn update_config(&self, config: AppConfig) -> Result<()> {
        let mut guard = GLOBAL_CONFIG.lock().unwrap();
        *guard = Some(config);
        update_metrics();
        Ok(())
    }
}

unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        let is_remote = IS_REMOTE.load(Ordering::Relaxed);
        if is_remote {
            let kbd = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
            let is_down = wparam.0 as u32 == WM_KEYDOWN || wparam.0 as u32 == WM_SYSKEYDOWN;
            let event = InputEvent::Keyboard { key_code: kbd.vkCode, is_down };
            if let Ok(guard) = GLOBAL_CALLBACK.try_lock() {
                if let Some(cb) = &*guard {
                    cb(event);
                }
            }
            return LRESULT(1); // Swallow
        }
    }
    CallNextHookEx(KEYBOARD_HOOK, code, wparam, lparam)
}

unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code < 0 {
        return CallNextHookEx(MOUSE_HOOK, code, wparam, lparam);
    }

    let ms = &*(lparam.0 as *const MSLLHOOKSTRUCT);
    let injected = (ms.flags & 0x01) != 0 || (ms.flags & 0x02) != 0;
    
    // 1. FAST PATH: Ignore all injected events
    if injected {
        return CallNextHookEx(MOUSE_HOOK, code, wparam, lparam);
    }

    let is_remote = IS_REMOTE.load(Ordering::Relaxed);
    let msg = wparam.0 as u32;

    // 2. FAST PATH: If not remote and not move, let it pass immediately
    if !is_remote && msg != WM_MOUSEMOVE {
        return CallNextHookEx(MOUSE_HOOK, code, wparam, lparam);
    }

    let metrics_guard = CACHED_METRICS.try_lock();
    let metrics = if let Ok(ref guard) = metrics_guard { guard.as_ref() } else { None };

    let mut event = None;
    let mut swallow = is_remote;

    if is_remote {
        if let Some(m) = metrics {
            if msg == WM_MOUSEMOVE {
                let center_x = m.left + m.width / 2;
                let center_y = m.top + m.height / 2;
                let dx = ms.pt.x - center_x;
                let dy = ms.pt.y - center_y;

                if dx != 0 || dy != 0 {
                    if let Ok(mut guard) = VIRTUAL_CURSOR_POS.try_lock() {
                        if let Some((vx, vy)) = *guard {
                            let new_vx = (vx + (dx as f32 / m.width as f32)).max(0.0).min(1.0);
                            let new_vy = (vy + (dy as f32 / m.height as f32)).max(0.0).min(1.0);
                            *guard = Some((new_vx, new_vy));
                            
                            // Return to Local Logic (Based on Virtual Cursor)
                            let mut should_return = false;
                            if let Ok(pos_guard) = ACTIVE_REMOTE_POS.try_lock() {
                                if let Some(pos) = &*pos_guard {
                                    should_return = match pos {
                                        ScreenPosition::Right => new_vx <= 0.001,
                                        ScreenPosition::Left => new_vx >= 0.999,
                                        ScreenPosition::Top => new_vy >= 0.999,
                                        ScreenPosition::Bottom => new_vy <= 0.001,
                                    };
                                }
                            }

                            if should_return {
                                IS_REMOTE.store(false, Ordering::SeqCst);
                                swallow = false;
                                *guard = None;
                                if let Ok(mut pos_guard) = ACTIVE_REMOTE_POS.try_lock() { *pos_guard = None; }
                                event = Some(InputEvent::ScreenSwitch(ScreenSide::Local));
                                // Center the physical cursor on the original edge to avoid immediate re-trigger
                                let target_x = match ACTIVE_REMOTE_POS.lock().unwrap().clone() {
                                    Some(ScreenPosition::Right) => m.left + m.width - 50,
                                    Some(ScreenPosition::Left) => m.left + 50,
                                    _ => ms.pt.x,
                                };
                                let _ = SetCursorPos(target_x, ms.pt.y);
                            } else {
                                // Rate limit Move
                                use std::time::Instant;
                                static mut LAST_SEND: Option<Instant> = None;
                                let now = Instant::now();
                                if LAST_SEND.map_or(true, |l| now.duration_since(l).as_millis() >= 8) {
                                    LAST_SEND = Some(now);
                                    event = Some(InputEvent::MouseMove { x: new_vx, y: new_vy });
                                }
                                let _ = SetCursorPos(center_x, center_y);
                            }
                        }
                    }
                }
            } else {
                event = match msg {
                    WM_LBUTTONDOWN | WM_LBUTTONUP => Some(InputEvent::MouseButton { button: platform_passer_core::MouseButton::Left, is_down: msg == WM_LBUTTONDOWN }),
                    WM_RBUTTONDOWN | WM_RBUTTONUP => Some(InputEvent::MouseButton { button: platform_passer_core::MouseButton::Right, is_down: msg == WM_RBUTTONDOWN }),
                    WM_MBUTTONDOWN | WM_MBUTTONUP => Some(InputEvent::MouseButton { button: platform_passer_core::MouseButton::Middle, is_down: msg == WM_MBUTTONDOWN }),
                    0x020A => Some(InputEvent::Scroll { dx: 0.0, dy: (ms.mouseData >> 16) as i16 as f32 / 120.0 }),
                    0x020E => Some(InputEvent::Scroll { dx: (ms.mouseData >> 16) as i16 as f32 / 120.0, dy: 0.0 }),
                    _ => None
                };
            }
        }
    } else if msg == WM_MOUSEMOVE {
        // LOCAL MODE: Edge Detection
        if let Some(m) = metrics {
            let abs_x = (ms.pt.x - m.left) as f32 / m.width as f32;
            let abs_y = (ms.pt.y - m.top) as f32 / m.height as f32;
            let mut triggered_remote = None;

            if let Ok(config_opt) = GLOBAL_CONFIG.try_lock() {
                if let Some(config) = &*config_opt {
                    for remote in &config.topology.remotes {
                        let hit = match remote.position {
                            ScreenPosition::Right => abs_x >= 0.999,
                            ScreenPosition::Left => abs_x <= 0.001,
                            ScreenPosition::Top => abs_y <= 0.001,
                            ScreenPosition::Bottom => abs_y >= 0.999,
                        };
                        if hit {
                            triggered_remote = Some(remote.position.clone());
                            break;
                        }
                    }
                }
            }

            if let Some(pos) = triggered_remote {
                IS_REMOTE.store(true, Ordering::SeqCst);
                swallow = true;
                if let Ok(mut v_guard) = VIRTUAL_CURSOR_POS.try_lock() {
                    *v_guard = Some((abs_x, abs_y));
                }
                if let Ok(mut pos_guard) = ACTIVE_REMOTE_POS.try_lock() {
                    *pos_guard = Some(pos);
                }
                let center_x = m.left + m.width / 2;
                let center_y = m.top + m.height / 2;
                let _ = SetCursorPos(center_x, center_y);
                event = Some(InputEvent::ScreenSwitch(ScreenSide::Remote));
            }
        }
    }

    if let Some(ev) = event {
        if let Ok(guard) = GLOBAL_CALLBACK.try_lock() {
            if let Some(cb) = &*guard { cb(ev); }
        }
    }

    if swallow { LRESULT(1) } else { CallNextHookEx(MOUSE_HOOK, code, wparam, lparam) }
}

const WM_LBUTTONDOWN: u32 = 0x0201;
const WM_LBUTTONUP: u32 = 0x0202;
const WM_RBUTTONDOWN: u32 = 0x0204;
const WM_RBUTTONUP: u32 = 0x0205;
const WM_MBUTTONDOWN: u32 = 0x0207;
const WM_MBUTTONUP: u32 = 0x0208;
