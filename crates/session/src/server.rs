use crate::events::SessionEvent;
use crate::{log_info, log_error, log_debug, log_warn};
use anyhow::Result;
use platform_passer_core::{Frame, InputEvent, ClipboardEvent, Handshake};
use platform_passer_transport::{make_ws_listener};
use platform_passer_input::{InputSource, DefaultInputSource};
use platform_passer_clipboard::{ClipboardProvider, DefaultClipboard};
use std::net::SocketAddr;
use tokio::sync::mpsc::Sender;
use std::sync::Arc;
use tokio_tungstenite::{accept_async, tungstenite::Message};
use futures_util::{StreamExt, SinkExt};
use std::collections::HashMap;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;

pub async fn run_server_session(bind_addr: SocketAddr, event_tx: Sender<SessionEvent>) -> Result<()> {
    log_info!(&event_tx, "Starting WebSocket server session on {}", bind_addr);
    
    // 1. Setup Input Source (Server captures local input)
    let source = Arc::new(DefaultInputSource::new());
    let (input_tx, _input_rx) = tokio::sync::broadcast::channel::<InputEvent>(100);
    let input_tx_captured = input_tx.clone();
    
    source.start_capture(Box::new(move |event| {
        let _ = input_tx_captured.send(event);
    }))?;
    
    // 2. Setup WebSocket Listener
    let listener = make_ws_listener(bind_addr).await?;
    log_info!(&event_tx, "WebSocket Server listening on {}", bind_addr);
    
    // 3. Accept Loop
    while let Ok((stream, addr)) = listener.accept().await {
        log_debug!(&event_tx, "New incoming TCP connection from {}", addr);
        let event_tx_clone = event_tx.clone();
        let input_rx = input_tx.subscribe();
        
        tokio::spawn(async move {
            match accept_async(stream).await {
                Ok(ws_stream) => {
                    log_info!(&event_tx_clone, "WebSocket handshake successful with {}", addr);
                    let _ = event_tx_clone.send(SessionEvent::Connected(addr.to_string())).await;
                    
                    if let Err(e) = handle_protocol_session(ws_stream, input_rx, event_tx_clone.clone()).await {
                        log_error!(&event_tx_clone, "Protocol error with {}: {}", addr, e);
                    }
                }
                Err(e) => {
                    log_error!(&event_tx_clone, "WebSocket handshake failed with {}: {}", addr, e);
                }
            }
        });
    }
    
    Ok(())
}

async fn handle_protocol_session(
    ws_stream: tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
    mut input_rx: tokio::sync::broadcast::Receiver<InputEvent>,
    event_tx: Sender<SessionEvent>
) -> Result<()> {
    let (mut ws_sink, mut ws_stream) = ws_stream.split();
    let clip = DefaultClipboard::new();
    let _log_addr = "Remote Client";

    // 1. Protocol Handshake
    log_debug!(&event_tx, "Awaiting application handshake...");
    if let Some(Ok(Message::Binary(bytes))) = ws_stream.next().await {
        let frame: Frame = bincode::deserialize(&bytes)?;
        match frame {
            Frame::Handshake(h) => {
                log_info!(&event_tx, "Received handshake (Client: {})", h.client_id);
                let resp = Frame::Handshake(Handshake {
                    version: 1,
                    client_id: "macos-server".to_string(), // TODO: Make dynamic
                    capabilities: vec!["input".to_string(), "clipboard".to_string()],
                    screen_info: None,
                });
                ws_sink.send(Message::Binary(bincode::serialize(&resp)?)).await?;
            }
            _ => {
                log_error!(&event_tx, "Invalid handshake frame");
                return Err(anyhow::anyhow!("Invalid handshake"));
            }
        }
    }

    let mut active_files: HashMap<u32, File> = HashMap::new();

    log_debug!(&event_tx, "Entering protocol loop...");
    loop {
        tokio::select! {
            // Read from client
            msg = ws_stream.next() => {
                match msg {
                    Some(Ok(Message::Binary(bytes))) => {
                        match bincode::deserialize::<Frame>(&bytes) {
                            Ok(frame) => {
                                match frame {
                                    Frame::Clipboard(ClipboardEvent::Text(text)) => {
                                        log_debug!(&event_tx, "Received clipboard update ({} chars)", text.len());
                                        let _ = clip.set_text(text);
                                    }
                                    Frame::Heartbeat(hb) => {
                                        let _ = ws_sink.send(Message::Binary(bincode::serialize(&Frame::Heartbeat(hb))?)).await;
                                    }
                                    Frame::FileTransferRequest(req) => {
                                        log_info!(&event_tx, "File transfer request: {} ({} bytes)", req.filename, req.file_size);
                                        let download_dir = std::path::Path::new("downloads");
                                        let _ = tokio::fs::create_dir_all(download_dir).await;
                                        let file_path = download_dir.join(&req.filename);
                                        
                                        match File::create(&file_path).await {
                                            Ok(file) => {
                                                active_files.insert(req.id, file);
                                                let resp = Frame::FileTransferResponse(platform_passer_core::FileTransferResponse { id: req.id, accepted: true });
                                                let _ = ws_sink.send(Message::Binary(bincode::serialize(&resp)?)).await;
                                                log_info!(&event_tx, "Accepted file transfer ID: {}", req.id);
                                            }
                                            Err(e) => {
                                                log_error!(&event_tx, "Failed to create file {:?}: {}", file_path, e);
                                                let resp = Frame::FileTransferResponse(platform_passer_core::FileTransferResponse { id: req.id, accepted: false });
                                                let _ = ws_sink.send(Message::Binary(bincode::serialize(&resp)?)).await;
                                            }
                                        }
                                    }
                                    Frame::FileData { id, chunk } => {
                                        if let Some(file) = active_files.get_mut(&id) {
                                            if let Err(e) = file.write_all(&chunk).await {
                                                log_error!(&event_tx, "Failed to write chunk for file {}: {}", id, e);
                                                active_files.remove(&id);
                                            }
                                        }
                                    }
                                    Frame::FileEnd { id } => {
                                        if let Some(mut file) = active_files.remove(&id) {
                                            let _ = file.flush().await;
                                            log_info!(&event_tx, "File transfer completed for ID: {}", id);
                                        }
                                    }
                                    _ => {
                                        log_debug!(&event_tx, "Received unhandled frame type: {:?}", frame);
                                    }
                                }
                            }
                            Err(e) => {
                                log_error!(&event_tx, "Failed to deserialize frame: {}", e);
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => {
                        log_info!(&event_tx, "Client closed connection.");
                        break;
                    }
                    Some(Err(e)) => {
                        log_error!(&event_tx, "WebSocket read error: {}", e);
                        break;
                    }
                    _ => {}
                }
            }
            // Send inputs to client
            result = input_rx.recv() => {
                match result {
                    Ok(event) => {
                        if matches!(event, InputEvent::ScreenSwitch(_)) {
                            log_info!(&event_tx, "Switching focus: {:?}", event);
                        }
                        let bytes = bincode::serialize(&Frame::Input(event))?;
                        if let Err(e) = ws_sink.send(Message::Binary(bytes)).await {
                            log_error!(&event_tx, "Failed to send input: {}", e);
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        log_warn!(&event_tx, "Input broadcast LAGGED by {} messages. Skipping frames.", n);
                        continue;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        log_error!(&event_tx, "Input broadcast channel closed.");
                        break;
                    }
                }
            }
        }
    }

    log_info!(&event_tx, "Session terminated. Resetting focus.");
    DefaultInputSource::set_remote(false);
    let _ = event_tx.send(SessionEvent::Disconnected).await;
    Ok(())
}
