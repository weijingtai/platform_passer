use crate::events::SessionEvent;
use crate::commands::SessionCommand;
use crate::{log_info, log_error, log_debug, log_warn};
use anyhow::Result;
use platform_passer_core::{Frame, ClipboardEvent, Handshake, Heartbeat};
use platform_passer_transport::{connect_ws};
use platform_passer_input::{InputSink, DefaultInputSink, InputSource, DefaultInputSource};
use platform_passer_clipboard::{ClipboardProvider, DefaultClipboard};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc::{self, Sender, Receiver};
use std::time::Duration;
use tokio_tungstenite::tungstenite::Message;
use futures_util::{StreamExt, SinkExt};
use std::hash::{Hash, Hasher};
use std::collections::hash_map::DefaultHasher;

// Helper to track last content for loop protection
#[derive(Debug, Clone, PartialEq)]
enum LocalClipboardContent {
    Text(String),
    Image(u64), // Hash of the image data
}

fn calculate_hash<T: Hash>(t: &T) -> u64 {
    let mut s = DefaultHasher::new();
    t.hash(&mut s);
    s.finish()
}

pub async fn run_client_session(
    server_addr: SocketAddr, 
    _send_file_path: Option<PathBuf>,
    mut cmd_rx: Receiver<SessionCommand>,
    event_tx: Sender<SessionEvent>
) -> Result<()> {
    // 1. Persistent Setup (Clipboard & Input Sink & Input Source)
    // These survive across connection retries.
    let (local_tx, mut local_rx) = mpsc::channel::<Frame>(100);
    let sink = Arc::new(DefaultInputSink::new());
    let source = Arc::new(DefaultInputSource::new());

    // Start Clipboard Listener Once
    let clip_tx = local_tx.clone();
    let clip_log = event_tx.clone();
    let clipboard = DefaultClipboard::new();
    
    // Loop Protection: Store last received content hash/string to avoid echo
    let last_remote_clip = Arc::new(Mutex::new(None::<LocalClipboardContent>));
    let last_remote_clip_listener = last_remote_clip.clone();

    if let Err(e) = clipboard.start_listener(Box::new(move || {
        let clip = DefaultClipboard::new();
        
        // Priority 1: Text
        if let Ok(text) = clip.get_text() {
            if !text.is_empty() {
                // Check against last remote
                let should_send = if let Ok(lock) = last_remote_clip_listener.lock() {
                    match &*lock {
                        Some(LocalClipboardContent::Text(last)) => *last != text,
                        _ => true,
                    }
                } else { true };

                if should_send {
                     let _ = clip_tx.blocking_send(Frame::Clipboard(ClipboardEvent::Text(text)));
                }
                return;
            }
        }
        
        // Priority 2: Image
        if let Ok(Some(img_data)) = clip.get_image() {
            let img_hash = calculate_hash(&img_data);
             let should_send = if let Ok(lock) = last_remote_clip_listener.lock() {
                match &*lock {
                    Some(LocalClipboardContent::Image(last_hash)) => *last_hash != img_hash,
                    _ => true,
                }
            } else { true };
            
            if should_send {
                 let _ = clip_tx.blocking_send(Frame::Clipboard(ClipboardEvent::Image { data: img_data }));
            }
        }
    })) {
        log_error!(&clip_log, "Failed to start clipboard listener: {}", e);
    }

    // Start Input Capture Once (Server receives events from Client)
    let input_tx = local_tx.clone();
    let input_log = event_tx.clone();
    if let Err(e) = source.start_capture(Box::new(move |event| {
        let _ = input_tx.blocking_send(Frame::Input(event));
    })) {
        log_error!(&input_log, "Failed to start input capture: {}", e);
    }

    let mut backoff = Duration::from_secs(1);
    let max_backoff = Duration::from_secs(30);

    // 2. Main Connection Retry Loop
    loop {
        log_info!(&event_tx, "Attempting connection to {}...", server_addr);

        // Attempt connection with ability to abort via UI
        let connect_fut = connect_ws(server_addr);
        let stream_result = tokio::select! {
            res = connect_fut => res,
            Some(cmd) = cmd_rx.recv() => {
                if matches!(cmd, SessionCommand::Disconnect) {
                    log_info!(&event_tx, "Disconnect requested by user.");
                    return Ok(());
                }
                continue; 
            }
        };

        match stream_result {
            Ok(ws_stream) => {
                // Reset backoff on successful connection
                backoff = Duration::from_secs(1);
                
                log_info!(&event_tx, "Connected to {}.", server_addr);
                let _ = event_tx.send(SessionEvent::Connected(server_addr.to_string())).await;

                let (mut ws_sink, mut ws_stream) = ws_stream.split();

                // 3. Handshake
                let screen_info = {
                    #[cfg(target_os = "macos")]
                    {
                        platform_passer_input::get_screen_info()
                    }
                    #[cfg(not(target_os = "macos"))]
                    {
                        None
                    }
                };

                let handshake = Frame::Handshake(Handshake {
                    version: 1,
                    client_id: "macos-client".to_string(), // TODO: Make dynamic
                    capabilities: vec!["input".to_string(), "clipboard".to_string()],
                    screen_info,
                });

                let mut handshake_success = false;
                if let Err(e) = ws_sink.send(Message::Binary(bincode::serialize(&handshake)?)).await {
                    log_error!(&event_tx, "Handshake send failed: {}", e);
                } else {
                     // Wait for response
                     let handshake_resp_fut = ws_stream.next();
                     let handshake_res = tokio::select! {
                        res = handshake_resp_fut => res,
                        Some(cmd) = cmd_rx.recv() => {
                             if matches!(cmd, SessionCommand::Disconnect) {
                                  let _ = ws_sink.close().await;
                                  return Ok(());
                             }
                             None
                        }
                     };

                     match handshake_res {
                        Some(Ok(Message::Binary(bytes))) => {
                            if let Ok(Frame::Handshake(_)) = bincode::deserialize(&bytes) {
                                log_info!(&event_tx, "Handshake accepted. Session active.");
                                handshake_success = true;
                            } else {
                                log_error!(&event_tx, "Invalid handshake response.");
                            }
                        }
                        _ => {
                            log_error!(&event_tx, "Handshake timed out or failed.");
                        }
                     }
                }

                if handshake_success {
                    // 4. Active Session Loop
                    let (hb_stop_tx, mut hb_stop_rx) = mpsc::channel::<()>(1);
                    let hb_local_tx = local_tx.clone();
                    
                    // Start Heartbeat
                    tokio::spawn(async move {
                        loop {
                            tokio::select! {
                                _ = tokio::time::sleep(Duration::from_secs(5)) => {
                                    let hb = Frame::Heartbeat(Heartbeat {
                                        timestamp: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs(),
                                    });
                                    if hb_local_tx.send(hb).await.is_err() { break; }
                                }
                                _ = hb_stop_rx.recv() => { break; }
                            }
                        }
                    });

                    // Event Loop
                    loop {
                        tokio::select! {
                            // A. Outbound (Clipboard, Heartbeat, Input Events)
                            Some(frame) = local_rx.recv() => {
                                let bytes = bincode::serialize(&frame)?;
                                if let Err(e) = ws_sink.send(Message::Binary(bytes)).await {
                                    log_error!(&event_tx, "Send failed: {}", e);
                                    break; // Break inner loop -> Reconnect
                                }
                            }

                            // B. Inbound (Network)
                            Some(msg_res) = ws_stream.next() => {
                                match msg_res {
                                    Ok(Message::Binary(bytes)) => {
                                        if let Ok(frame) = bincode::deserialize::<Frame>(&bytes) {
                                            match frame {
                                                Frame::Input(event) => {
                                                    if let platform_passer_core::InputEvent::ScreenSwitch(side) = event {
                                                        log_info!(&event_tx, "Focus switched to {:?}", side);
                                                    } else {
                                                        let _ = sink.inject_event(event);
                                                    }
                                                }
                                                Frame::Clipboard(ClipboardEvent::Text(text)) => {
                                                    log_info!(&event_tx, "Clipboard sync from server (Text).");
                                                    if let Ok(mut lock) = last_remote_clip.lock() {
                                                        *lock = Some(LocalClipboardContent::Text(text.clone()));
                                                    }
                                                    let _ = DefaultClipboard::new().set_text(text);
                                                }
                                                Frame::Clipboard(ClipboardEvent::Image { data }) => {
                                                    log_info!(&event_tx, "Clipboard sync from server (Image, {} bytes).", data.len());
                                                    let hash = calculate_hash(&data);
                                                    if let Ok(mut lock) = last_remote_clip.lock() {
                                                        *lock = Some(LocalClipboardContent::Image(hash));
                                                    }
                                                    let _ = DefaultClipboard::new().set_image(data);
                                                }
                                                Frame::Heartbeat(_) => {},
                                                Frame::FileTransferResponse(resp) => {
                                                    log_info!(&event_tx, "File transfer accepted: {}", resp.accepted);
                                                }
                                                _ => {}
                                            }
                                        }
                                    }
                                    Ok(Message::Close(_)) => {
                                        log_info!(&event_tx, "Server closed connection.");
                                        break; 
                                    }
                                    Err(e) => {
                                        log_error!(&event_tx, "WebSocket Error: {}", e);
                                        break; 
                                    }
                                    _ => {}
                                }
                            }

                            // C. User Command
                            Some(cmd) = cmd_rx.recv() => {
                                match cmd {
                                    SessionCommand::Disconnect => {
                                        log_info!(&event_tx, "Disconnecting...");
                                        let _ = hb_stop_tx.send(()).await;
                                        let _ = ws_sink.close().await;
                                        return Ok(());
                                    },
                                    SessionCommand::UpdateConfig(config) => {
                                        log_info!(&event_tx, "Updating session configuration...");
                                        // Update Sink and Source
                                        if let Err(e) = sink.update_config(config.clone()) {
                                            log_error!(&event_tx, "Failed to update sink config: {}", e);
                                        }
                                        if let Err(e) = source.update_config(config) {
                                            log_error!(&event_tx, "Failed to update source config: {}", e);
                                        }
                                    },
                                    _ => {} 
                                }
                            }
                        }
                    }
                    
                    // Clean up before retry
                    let _ = hb_stop_tx.send(()).await;
                }
                
                let _ = event_tx.send(SessionEvent::Disconnected).await;
            }
            Err(e) => {
                log_error!(&event_tx, "Connection failed: {}. Retrying in {:?}...", e, backoff);
            }
        }

        // Delay with interrupt and exponential backoff
        tokio::select! {
            _ = tokio::time::sleep(backoff) => {
                backoff = std::cmp::min(backoff * 2, max_backoff);
            },
            Some(cmd) = cmd_rx.recv() => {
                if matches!(cmd, SessionCommand::Disconnect) {
                    return Ok(());
                }
            }
        }
    }
}
