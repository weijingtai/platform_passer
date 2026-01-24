#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use tauri::{command, WebviewWindow, State, Emitter};
use platform_passer_session::{run_client_session, run_server_session, SessionEvent, SessionCommand};
use std::net::SocketAddr;
use tokio::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::path::PathBuf;

use platform_passer_session::logging::GuiLogLayer;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use std::io::Write;

// Shared log sender for GUI updates
struct LogState {
    tx: Arc<Mutex<Option<mpsc::Sender<SessionEvent>>>>,
}

// Simple state to hold active session handle? 
struct AppState {
    running: Arc<Mutex<bool>>,
    command_tx: Arc<Mutex<Option<mpsc::Sender<SessionCommand>>>>,
    log_tx: Arc<Mutex<Option<mpsc::Sender<SessionEvent>>>>,
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
    *state.command_tx.lock().unwrap() = None;

    let running_clone = state.running.clone();
    let log_tx_clone = state.log_tx.clone();
    
    // Spawn async task
    tauri::async_runtime::spawn(async move {
        let (tx, mut rx) = mpsc::channel(100);
        
        // Update global log forwarder
        *log_tx_clone.lock().unwrap() = Some(tx.clone());

        let bind_addr: SocketAddr = format!("{}:{}", ip, port).parse().unwrap_or_else(|_| "0.0.0.0:4433".parse().unwrap());
        
        let _session_task = tokio::spawn(async move {
            run_server_session(bind_addr, tx).await
        });
        
        // Event Forwarder Loop
        while let Some(event) = rx.recv().await {
            let (event_type, message) = match event {
                SessionEvent::Log { level, message } => ("Log".to_string(), format!("[{:?}] {}", level, message)),
                SessionEvent::Connected(s) => ("Connected".to_string(), format!("Connected to {}", s)),
                SessionEvent::Disconnected => ("Disconnected".to_string(), "Disconnected".to_string()),
                SessionEvent::Error(s) => ("Error".to_string(), format!("Error: {}", s)),
            };

            let _ = window.emit("session-event", Payload { event_type, message });
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
                SessionEvent::Connected(s) => ("Connected".to_string(), format!("Connected to {}", s)),
                SessionEvent::Disconnected => ("Disconnected".to_string(), "Disconnected".to_string()),
                SessionEvent::Error(s) => ("Error".to_string(), format!("Error: {}", s)),
            };

            let _ = window.emit("session-event", Payload { event_type, message });
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
        })
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![start_server, connect_to, send_file_action, check_accessibility])
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
