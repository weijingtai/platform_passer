use crate::events::SessionEvent;
use crate::{log_info, log_error, log_debug, log_warn};
use anyhow::Result;
use platform_passer_core::{Frame, InputEvent, ClipboardEvent, Handshake};
use platform_passer_transport::{make_ws_listener};
use platform_passer_input::{InputSource, DefaultInputSource};
use platform_passer_clipboard::{ClipboardProvider, DefaultClipboard};
use std::net::SocketAddr;
use tokio::sync::mpsc::Sender;
use tokio_tungstenite::{accept_async, tungstenite::Message};
use std::sync::{Arc, Mutex};
use crate::clipboard_utils::{LocalClipboardContent, calculate_hash};
use futures_util::{StreamExt, SinkExt};
use std::collections::HashMap;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;

pub async fn run_server_session(bind_addr: SocketAddr, event_tx: Sender<SessionEvent>) -> Result<()> {
    log_info!(&event_tx, "Starting WebSocket server session on {}", bind_addr);
    
    // 1. Setup Shared Outbound channel for all events (Input, Clipboard)
    let (broadcast_tx, _broadcast_rx) = tokio::sync::broadcast::channel::<Frame>(100);
    
    // 2. Setup Input Source (Server captures local input)
    let source = Arc::new(DefaultInputSource::new());
    let broadcast_tx_captured = broadcast_tx.clone();
    
    source.start_capture(Box::new(move |event| {
        let _ = broadcast_tx_captured.send(Frame::Input(event));
    }))?;

    // 3. Setup Clipboard Listener
    let clip_tx = broadcast_tx.clone();
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
                let should_send = if let Ok(lock) = last_remote_clip_listener.lock() {
                    match &*lock {
                        Some(LocalClipboardContent::Text(last)) => *last != text,
                        _ => true,
                    }
                } else { true };

                if should_send {
                     let _ = clip_tx.send(Frame::Clipboard(ClipboardEvent::Text(text)));
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
                 let _ = clip_tx.send(Frame::Clipboard(ClipboardEvent::Image { data: img_data }));
            }
        }
    })) {
        log_error!(&clip_log, "Failed to start clipboard listener: {}", e);
    }
    
    // 2. Setup WebSocket Listener
    let listener = make_ws_listener(bind_addr).await?;
    log_info!(&event_tx, "WebSocket Server listening on {}", bind_addr);
    
    // 3. Accept Loop
    while let Ok((stream, addr)) = listener.accept().await {
        log_debug!(&event_tx, "New incoming TCP connection from {}", addr);
        let event_tx_clone = event_tx.clone();
        let broadcast_rx = broadcast_tx.subscribe();
        let last_remote_clip_conn = last_remote_clip.clone();
        
        let source_clone = source.clone();
        tokio::spawn(async move {
            match accept_async(stream).await {
                Ok(ws_stream) => {
                    log_info!(&event_tx_clone, "WebSocket handshake successful with {}", addr);
                    let _ = event_tx_clone.send(SessionEvent::Connected(addr.to_string())).await;
                    
                    if let Err(e) = handle_protocol_session(ws_stream, broadcast_rx, event_tx_clone.clone(), source_clone, last_remote_clip_conn).await {
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
    mut broadcast_rx: tokio::sync::broadcast::Receiver<Frame>,
    event_tx: Sender<SessionEvent>,
    source: Arc<dyn InputSource>,
    last_remote_clip: Arc<Mutex<Option<LocalClipboardContent>>>,
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
                                        if let Ok(mut lock) = last_remote_clip.lock() {
                                            *lock = Some(LocalClipboardContent::Text(text.clone()));
                                        }
                                        let _ = clip.set_text(text);
                                    }
                                    Frame::Clipboard(ClipboardEvent::Image { data }) => {
                                        log_debug!(&event_tx, "Received clipboard image ({} bytes)", data.len());
                                        let hash = calculate_hash(&data);
                                        if let Ok(mut lock) = last_remote_clip.lock() {
                                            *lock = Some(LocalClipboardContent::Image(hash));
                                        }
                                        let _ = clip.set_image(data);
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
            // Send events to client
            result = broadcast_rx.recv() => {
                match result {
                    Ok(frame) => {
                        if let Frame::Input(InputEvent::ScreenSwitch(_)) = &frame {
                            log_info!(&event_tx, "Switching focus: {:?}", frame);
                        }
                        let bytes = bincode::serialize(&frame)?;
                        if let Err(e) = ws_sink.send(Message::Binary(bytes)).await {
                            log_error!(&event_tx, "Failed to send frame: {}", e);
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        log_warn!(&event_tx, "Broadcast LAGGED by {} messages. Skipping frames.", n);
                        continue;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        log_error!(&event_tx, "Broadcast channel closed.");
                        break;
                    }
                }
            }
        }
    }

    log_info!(&event_tx, "Session terminated. Resetting focus.");
    let _ = source.set_remote(false);
    let _ = event_tx.send(SessionEvent::Disconnected).await;
    Ok(())
}
