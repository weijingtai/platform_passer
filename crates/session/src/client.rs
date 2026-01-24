use crate::events::SessionEvent;
use crate::commands::SessionCommand;
use crate::{log_info, log_error, log_debug, log_warn};
use anyhow::Result;
use platform_passer_core::{Frame, ClipboardEvent, Handshake, Heartbeat};
use platform_passer_transport::{connect_ws};
use platform_passer_input::{InputSink, DefaultInputSink};
use platform_passer_clipboard::{ClipboardProvider, DefaultClipboard};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc::{self, Sender, Receiver};
use std::time::Duration;
use tokio_tungstenite::tungstenite::Message;
use futures_util::{StreamExt, SinkExt};

pub async fn run_client_session(
    server_addr: SocketAddr, 
    _send_file_path: Option<PathBuf>,
    mut cmd_rx: Receiver<SessionCommand>,
    event_tx: Sender<SessionEvent>
) -> Result<()> {
    // 1. Persistent Setup (Clipboard & Input Sink)
    // These survive across connection retries.
    let (local_tx, mut local_rx) = mpsc::channel::<Frame>(100);
    let sink = Arc::new(DefaultInputSink::new());

    // Start Clipboard Listener Once
    let clip_tx = local_tx.clone();
    let clip_log = event_tx.clone();
    let clipboard = DefaultClipboard::new();
    
    // We try to start the listener, but if it fails (e.g. platform issue), we log and continue without it.
    if let Err(e) = clipboard.start_listener(Box::new(move || {
        if let Ok(text) = DefaultClipboard::new().get_text() {
            if !text.is_empty() {
                // Determine if this is a "local" copy or echo. 
                // Simple logic: just send it. The server/other side should filter echoes if needed or we rely on logic there.
                // Note: ideally we check if we just received this content to avoid loops.
                let _ = clip_tx.blocking_send(Frame::Clipboard(ClipboardEvent::Text(text)));
            }
        }
    })) {
        log_error!(&clip_log, "Failed to start clipboard listener: {}", e);
    }

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
                continue; // Ignore other commands while connecting?
            }
        };

        match stream_result {
            Ok(ws_stream) => {
                log_info!(&event_tx, "Connected to {}.", server_addr);
                let _ = event_tx.send(SessionEvent::Connected(server_addr.to_string())).await;

                let (mut ws_sink, mut ws_stream) = ws_stream.split();

                // 3. Handshake
                // We need to handle UI Disconnect during handshake too
                let handshake = Frame::Handshake(Handshake {
                    version: 1,
                    client_id: "macos-client".to_string(), // TODO: Make dynamic?
                    capabilities: vec!["input".to_string(), "clipboard".to_string()],
                });

                if let Err(e) = ws_sink.send(Message::Binary(bincode::serialize(&handshake)?)).await {
                    log_error!(&event_tx, "Handshake send failed: {}", e);
                    // Trigger retry logic
                } else {
                     // Wait for response
                     let handshake_resp_fut = ws_stream.next();
                     let handshake_res = tokio::select! {
                        res = handshake_resp_fut => res,
                        Some(cmd) = cmd_rx.recv() => {
                             if matches!(cmd, SessionCommand::Disconnect) {
                                 return Ok(());
                             }
                             None // Ignore
                        }
                     };

                     match handshake_res {
                        Some(Ok(Message::Binary(bytes))) => {
                            if let Ok(Frame::Handshake(_)) = bincode::deserialize(&bytes) {
                                log_info!(&event_tx, "Handshake accepted. Session active.");
                                
                                // 4. Active Session Loop
                                // Use a separate channel for heartbeat task to signal it to stop
                                let (hb_stop_tx, mut hb_stop_rx) = mpsc::channel::<()>(1);
                                let hb_local_tx = local_tx.clone();
                                let hb_log = event_tx.clone();
                                
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
                                        // A. Outbound (Clipboard, Heartbeat)
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
                                                                log_info!(&event_tx, "Clipboard sync from server.");
                                                                // Prevent echo loop? If we set text, listener might fire.
                                                                // Current impl depends on basic loop prevention or acceptance of one echo. 
                                                                // Ideally, use `set_text` that suppresses next event or verify content.
                                                                let _ = DefaultClipboard::new().set_text(text);
                                                            }
                                                            Frame::Heartbeat(_) => {}, // Server HB?
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
                                            if matches!(cmd, SessionCommand::Disconnect) {
                                                log_info!(&event_tx, "Disconnecting...");
                                                let _ = hb_stop_tx.send(()).await; // Stop HB
                                                // Optional: Send Close frame
                                                let _ = ws_sink.close().await;
                                                return Ok(()); // Full exit
                                            }
                                        }
                                    }
                                }
                                
                                // Clean up before retry
                                let _ = hb_stop_tx.send(()).await;
                                let _ = event_tx.send(SessionEvent::Disconnected).await;

                            } else {
                                log_error!(&event_tx, "Invalid handshake response.");
                            }
                        }
                        _ => {
                            log_error!(&event_tx, "Handshake timed out or failed.");
                        }
                     }
                }
            }
            Err(e) => {
                // Connection Failed
                log_error!(&event_tx, "Connection failed: {}. Retrying in 3s...", e);
            }
        }

        // Delay with interrupt
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(3)) => {},
            Some(cmd) = cmd_rx.recv() => {
                if matches!(cmd, SessionCommand::Disconnect) {
                    return Ok(());
                }
            }
        }
    }
}
