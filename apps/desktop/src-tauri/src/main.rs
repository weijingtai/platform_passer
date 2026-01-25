#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use tauri::{command, WebviewWindow, State, Emitter, Manager};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{TrayIconBuilder, TrayIconEvent, MouseButton, MouseButtonState};
use tauri_plugin_notification::NotificationExt;
use platform_passer_session::{run_client_session, run_server_session, SessionEvent, SessionCommand};
use std::net::SocketAddr;
use tokio::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::path::PathBuf;

use platform_passer_session::logging::GuiLogLayer;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use platform_passer_core::config::AppConfig;
// struct LogState {
//     tx: Arc<Mutex<Option<mpsc::Sender<SessionEvent>>>>,
// }

// Simple state to hold active session handle? 
struct AppState {
    running: Arc<Mutex<bool>>,
    command_tx: Arc<Mutex<Option<mpsc::Sender<SessionCommand>>>>,
    log_tx: Arc<Mutex<Option<mpsc::Sender<SessionEvent>>>>,
    config: Arc<Mutex<AppConfig>>,
}

#[command]
fn get_config(state: State<AppState>) -> AppConfig {
    state.config.lock().unwrap().clone()
}

#[command]
fn save_config(config: AppConfig, state: State<AppState>) -> Result<(), String> {
    *state.config.lock().unwrap() = config.clone();
    
    // Persist to disk
    let config_path = get_config_path();
    if let Some(parent) = config_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    
    let file = std::fs::File::create(config_path).map_err(|e| e.to_string())?;
    serde_json::to_writer_pretty(file, &config).map_err(|e| e.to_string())?;
    
    // If session is running, switch config immediately
    let tx_opt = state.command_tx.lock().unwrap();
    if let Some(tx) = &*tx_opt {
        let tx_clone = tx.clone();
        let config_clone = config.clone();
        tauri::async_runtime::spawn(async move {
            let _ = tx_clone.send(SessionCommand::UpdateConfig(config_clone)).await;
        });
    }
    
    Ok(())
}

fn get_config_path() -> PathBuf {
    #[cfg(target_os = "windows")]
    let base = PathBuf::from(std::env::var("APPDATA").unwrap_or_else(|_| ".".to_string()));
    #[cfg(not(target_os = "windows"))]
    let base = PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".to_string())).join(".config");
    
    let mut path = base;
    path.push("platform-passer");
    path.push("config.json");
    path
}

fn load_config() -> Option<AppConfig> {
    let path = get_config_path();
    if path.exists() {
        let file = std::fs::File::open(path).ok()?;
        serde_json::from_reader(file).ok()
    } else {
        None
    }
}

#[command]
fn send_file_action(path: String, state: State<AppState>) -> String {
    let tx_opt = state.command_tx.lock().unwrap();
    if let Some(tx) = &*tx_opt {
        let tx_clone = tx.clone();
        let path_buf = PathBuf::from(path); // Verify existence?
        tauri::async_runtime::spawn(async move {
            let _ = tx_clone.send(SessionCommand::SendFile(path_buf)).await;
        });
        "Queued file transfer".to_string()
    } else {
        "No active session".to_string()
    }
}

#[command]
fn start_server(ip: String, port: u16, window: WebviewWindow, state: State<AppState>) -> String {
    let mut running = state.running.lock().unwrap();
    if *running {
        return "Session already running".to_string();
    }
    *running = true;

    // Clear old tx if any
    let (cmd_tx, cmd_rx) = mpsc::channel(10);
    *state.command_tx.lock().unwrap() = Some(cmd_tx);

    let running_clone = state.running.clone();
    let log_tx_clone = state.log_tx.clone();
    let config_clone = state.config.clone();
    let app_handle = window.app_handle().clone();
    
    // Spawn async task
    tauri::async_runtime::spawn(async move {
        let (tx, mut rx) = mpsc::channel(100);
        
        // Update global log forwarder
        *log_tx_clone.lock().unwrap() = Some(tx.clone());

        let bind_addr: SocketAddr = format!("{}:{}", ip, port).parse().unwrap_or_else(|_| "0.0.0.0:4433".parse().unwrap());
        
        let _session_task = tokio::spawn(async move {
            run_server_session(bind_addr, cmd_rx, tx).await
        });
        
        // Event Forwarder Loop
        while let Some(event) = rx.recv().await {
            let (event_type, message) = match event {
                SessionEvent::Log { level, message } => ("Log".to_string(), format!("[{:?}] {}", level, message)),
                SessionEvent::Connected(ref s) => {
                    eprintln!("DEBUG: Received Connected event in event loop: {}", s);
                    let enabled = config_clone.lock().unwrap().notifications_enabled;
                    if enabled {
                         let _ = app_handle.notification().builder()
                            .title("Platform Passer")
                            .body(format!("Connected to {}", s))
                            .show();
                    }
                    ("Connected".to_string(), format!("Connected to {}", s))
                },
                SessionEvent::Disconnected => {
                    let enabled = config_clone.lock().unwrap().notifications_enabled;
                    if enabled {
                         let _ = app_handle.notification().builder()
                            .title("Platform Passer")
                            .body("Disconnected")
                            .show();
                    }
                    ("Disconnected".to_string(), "Disconnected".to_string())
                },
                SessionEvent::Error(ref s) => ("Error".to_string(), format!("Error: {}", s)),
            };

            if let Err(e) = window.emit("session-event", Payload { event_type, message }) {
                tracing::error!("Failed to emit session-event to GUI: {}", e);
            }
        }
        
        *running_clone.lock().unwrap() = false;
        *log_tx_clone.lock().unwrap() = None;
    });

    "Server starting...".to_string()
}

#[command]
fn connect_to(ip: String, port: u16, window: WebviewWindow, state: State<AppState>) -> String {
    let mut running = state.running.lock().unwrap();
    if *running {
        return "Session already running".to_string();
    }
    *running = true;
    
    // Create command channel
    let (cmd_tx, cmd_rx) = mpsc::channel(10);
    *state.command_tx.lock().unwrap() = Some(cmd_tx);
    
    let running_clone = state.running.clone();
    let tx_clone = state.command_tx.clone();
    let log_tx_clone = state.log_tx.clone();
    let config_clone = state.config.clone();
    let app_handle = window.app_handle().clone();

    let ip_clone = ip.clone();
    tauri::async_runtime::spawn(async move {
        let (tx, mut rx) = mpsc::channel(100);
        
        // Update global log forwarder
        *log_tx_clone.lock().unwrap() = Some(tx.clone());
        
        // Handle IPv6 brackets if needed, or simple concatenation
        let server_addr_str = format!("{}:{}", ip_clone, port);
        let server_addr: SocketAddr = server_addr_str.parse().unwrap_or_else(|_| "127.0.0.1:4433".parse().unwrap());
        
        let _session_task = tokio::spawn(async move {
            run_client_session(server_addr, None, cmd_rx, tx).await
        });

        // Event Forwarder Loop
        while let Some(event) = rx.recv().await {
            let (event_type, message) = match event {
                SessionEvent::Log { level, message } => ("Log".to_string(), format!("[{:?}] {}", level, message)),
                SessionEvent::Connected(ref s) => {
                    let enabled = config_clone.lock().unwrap().notifications_enabled;
                    if enabled {
                         let _ = app_handle.notification().builder()
                            .title("Platform Passer")
                            .body(format!("Connected to {}", s))
                            .show();
                    }
                    ("Connected".to_string(), format!("Connected to {}", s))
                },
                SessionEvent::Disconnected => {
                     let enabled = config_clone.lock().unwrap().notifications_enabled;
                    if enabled {
                         let _ = app_handle.notification().builder()
                            .title("Platform Passer")
                            .body("Disconnected")
                            .show();
                    }
                    ("Disconnected".to_string(), "Disconnected".to_string())
                },
                SessionEvent::Error(ref s) => ("Error".to_string(), format!("Error: {}", s)),
            };

            if let Err(e) = window.emit("session-event", Payload { event_type, message }) {
                tracing::error!("Failed to emit session-event to GUI: {}", e);
            }
        }
        *running_clone.lock().unwrap() = false;
        *tx_clone.lock().unwrap() = None;
        *log_tx_clone.lock().unwrap() = None;
    });

    format!("Connecting to {}:{}...", ip, port)
}

#[command]
fn check_accessibility() -> bool {
    #[cfg(target_os = "macos")]
    {
        platform_passer_input::macos::utils::is_accessibility_trusted()
    }
    #[cfg(not(target_os = "macos"))]
    {
        true
    }
}

#[derive(serde::Serialize, Clone)]
struct Payload {
    event_type: String,
    message: String,
}

fn main() {
    let filter = "debug,quinn=debug,rustls=info"; // Force debug for now to help user

    let log_tx = Arc::new(Mutex::new(None));
    let gui_layer = GuiLogLayer { tx: log_tx.clone() };

    // File logging setup
    let log_path = PathBuf::from("../../docs/windows/debug/latest.log");
    if let Some(parent) = log_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    
    // Create file appender that overwrites (std::fs::File::create truncates)
    let file_appender = if let Ok(f) = std::fs::File::create(&log_path) {
        Some(Arc::new(Mutex::new(f)))
    } else {
        None
    };

    let file_layer = if let Some(appender) = file_appender {
        Some(tracing_subscriber::fmt::layer()
            .with_ansi(false)
            .with_writer(move || {
                SharedFileWriter {
                    file: appender.clone(),
                }
            }))
    } else {
        None
    };

    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| filter.into()),
        ))
        .with(gui_layer)
        .with(file_layer)
        .init();
    
    tauri::Builder::default()
        .manage(AppState { 
            running: Arc::new(Mutex::new(false)),
            command_tx: Arc::new(Mutex::new(None)),
            log_tx,
            config: Arc::new(Mutex::new(load_config().unwrap_or_default())),
        })
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            // Tray setup
            let quit_i = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let show_i = MenuItem::with_id(app, "show", "Show", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show_i, &quit_i])?;
            
            let _tray = TrayIconBuilder::new()
                .menu(&menu)
                .on_menu_event(|app, event| {
                    match event.id.as_ref() {
                        "quit" => {
                            std::process::exit(0);
                        }
                        "show" => {
                            if let Some(window) = app.get_webview_window("main") {
                                let _ = window.show();
                                let _ = window.set_focus();
                            }
                        }
                        _ => {}
                    }
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                         let app = tray.app_handle();
                         if let Some(window) = app.get_webview_window("main") {
                             let _ = window.show();
                             let _ = window.set_focus();
                         }
                    }
                })
                .build(app)?;
            
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                window.hide().unwrap();
                api.prevent_close();
            }
        })
        .invoke_handler(tauri::generate_handler![start_server, connect_to, send_file_action, check_accessibility, get_config, save_config])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

struct SharedFileWriter {
    file: Arc<Mutex<std::fs::File>>,
}

impl std::io::Write for SharedFileWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let mut file = self.file.lock().map_err(|_| std::io::Error::new(std::io::ErrorKind::Other, "Lock poisoned"))?;
        file.write(buf)
    }
    
    fn flush(&mut self) -> std::io::Result<()> {
        let mut file = self.file.lock().map_err(|_| std::io::Error::new(std::io::ErrorKind::Other, "Lock poisoned"))?;
        file.flush()
    }
}
