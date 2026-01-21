#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use tauri::{command, WebviewWindow, State, Emitter};
use platform_passer_session::{run_client_session, run_server_session, SessionEvent, SessionCommand};
use std::net::SocketAddr;
use tokio::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::path::PathBuf;

// Simple state to hold active session handle? 
// For now we just fire and forget, but let's prevent multiple sessions.
struct AppState {
    // We only support one active session for now
    running: Arc<Mutex<bool>>,
    command_tx: Arc<Mutex<Option<mpsc::Sender<SessionCommand>>>>,
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
    
    // Spawn async task
    tauri::async_runtime::spawn(async move {
        let (tx, mut rx) = mpsc::channel(100);
        let bind_addr: SocketAddr = format!("{}:{}", ip, port).parse().unwrap_or_else(|_| "0.0.0.0:4433".parse().unwrap());
        
        let session_task = tokio::spawn(async move {
            run_server_session(bind_addr, tx).await
        });
        
        // Event Forwarder Loop
        while let Some(event) = rx.recv().await {
            let _ = window.emit("session-event", Payload {
                event_type: format!("{:?}", event), // naive stringification
                message: match event {
                    SessionEvent::Log(s) => s,
                    SessionEvent::Connected(s) => format!("Connected to {}", s),
                    SessionEvent::Disconnected => "Disconnected".to_string(),
                    SessionEvent::Error(s) => format!("Error: {}", s),
                }
            });
        }
        
        *running_clone.lock().unwrap() = false;
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

    let ip_clone = ip.clone();
    tauri::async_runtime::spawn(async move {
        let (tx, mut rx) = mpsc::channel(100);
        
        // Handle IPv6 brackets if needed, or simple concatenation
        let server_addr_str = format!("{}:{}", ip_clone, port);
        let server_addr: SocketAddr = server_addr_str.parse().unwrap_or_else(|_| "127.0.0.1:4433".parse().unwrap());
        
        let session_task = tokio::spawn(async move {
            run_client_session(server_addr, None, cmd_rx, tx).await
        });

        // Event Forwarder Loop
        while let Some(event) = rx.recv().await {
            let _ = window.emit("session-event", Payload {
                event_type: format!("{:?}", event), 
                message: match event {
                    SessionEvent::Log(s) => s,
                    SessionEvent::Connected(s) => format!("Connected to {}", s),
                    SessionEvent::Disconnected => "Disconnected".to_string(),
                    SessionEvent::Error(s) => format!("Error: {}", s),
                }
            });
        }
        *running_clone.lock().unwrap() = false;
        *tx_clone.lock().unwrap() = None;
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
    tracing_subscriber::fmt::init();
    
    tauri::Builder::default()
        .manage(AppState { 
            running: Arc::new(Mutex::new(false)),
            command_tx: Arc::new(Mutex::new(None)),
        })
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![start_server, connect_to, send_file_action, check_accessibility])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
