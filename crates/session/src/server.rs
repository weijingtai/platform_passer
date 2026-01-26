use crate::events::SessionEvent;
use crate::{log_info, log_error, log_debug, log_warn};
use anyhow::Result;
use platform_passer_core::{Frame, InputEvent, ClipboardEvent, Handshake};
use platform_passer_transport::{make_ws_listener};
use platform_passer_input::{InputSource, DefaultInputSource};
use platform_passer_clipboard::{ClipboardProvider, DefaultClipboard};
use std::net::SocketAddr;
use tokio::sync::mpsc::{Sender, Receiver};
use tokio_tungstenite::{accept_async, tungstenite::Message as WsMessage};
use crate::commands::SessionCommand;
use std::sync::{Arc, Mutex};
use crate::clipboard_utils::{LocalClipboardContent, calculate_hash};
use futures_util::{StreamExt, SinkExt};
use std::collections::HashMap;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use std::path::PathBuf;

pub async fn run_server_session(bind_addr: SocketAddr, mut cmd_rx: Receiver<SessionCommand>, event_tx: Sender<SessionEvent>) -> Result<()> {
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
    let _clip_log = event_tx.clone();
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
        log_error!(&event_tx, "Failed to start clipboard listener: {}", e);
    }

    // 4. Setup WebSocket Listener
    let listener = make_ws_listener(bind_addr).await?;
    log_info!(&event_tx, "WebSocket Server listening on {}", bind_addr);

    // 5. Main Server Loop (Commands + Accept)
    let cmd_broadcast_tx = broadcast_tx.clone();
    let cmd_event_tx = event_tx.clone();
    let pending_sends: Arc<Mutex<HashMap<u32, PathBuf>>> = Arc::new(Mutex::new(HashMap::new()));
    let pending_sends_clone = pending_sends.clone();
    let mut file_id_counter = 0u32;
    let source_cmd = source.clone();
    
    let mut session_tasks = Vec::new();

    loop {
        tokio::select! {
             // Handle Session Commands
            cmd_opt = cmd_rx.recv() => {
                match cmd_opt {
                    Some(SessionCommand::SendFile(path)) => {
                        if path.exists() {
                            file_id_counter += 1;
                            let id = file_id_counter;
                            let filename = path.file_name().unwrap_or_default().to_string_lossy().to_string();
                            let file_size = path.metadata().map(|m| m.len()).unwrap_or(0);
                            
                            if let Ok(mut lock) = pending_sends_clone.lock() {
                                lock.insert(id, path);
                            }
                            
                            let req = Frame::FileTransferRequest(platform_passer_core::FileTransferRequest {
                                id,
                                filename,
                                file_size,
                            });
                            let _ = cmd_broadcast_tx.send(req);
                        }
                    }
                    Some(SessionCommand::UpdateConfig(config)) => {
                        // Update source config (Server as sender)
                        if let Err(e) = source_cmd.update_config(config) {
                            log_error!(&cmd_event_tx, "Failed to update server source config: {}", e);
                        }
                    }
                    Some(SessionCommand::Disconnect) => {
                        log_info!(&cmd_event_tx, "Server disconnect command received. Shutting down.");
                        break;
                    }
                    None => {
                        log_info!(&cmd_event_tx, "Command channel closed. Shutting down server.");
                        break;
                    }
                }
            }
            // Handle New Connections
            accept_res = listener.accept() => {
                match accept_res {
                    Ok((stream, addr)) => {
                         let log_tx_spawn = event_tx.clone();
                        let broadcast_rx = broadcast_tx.subscribe();
                        let broadcast_tx_session = broadcast_tx.clone();
                        let last_remote_clip_conn = last_remote_clip.clone();
                        let pending_sends_session = pending_sends.clone();
                        let source_clone = source.clone();
                
                        let handle = tokio::spawn(async move {
                            match accept_async(stream).await {
                                Ok(ws_stream) => {
                                    log_info!(&log_tx_spawn, "WebSocket handshake successful with {}", addr);
                                    
                                    if let Err(e) = ws_stream.get_ref().set_nodelay(true) {
                                        log_warn!(&log_tx_spawn, "Failed to set TCP_NODELAY on server: {}", e);
                                    }
                
                                    let _ = log_tx_spawn.send(SessionEvent::Connected(addr.to_string())).await;
                                    
                                    if let Err(e) = handle_protocol_session(ws_stream, broadcast_rx, log_tx_spawn.clone(), source_clone, last_remote_clip_conn, pending_sends_session, broadcast_tx_session).await {
                                        log_error!(&log_tx_spawn, "Protocol error with {}: {}", addr, e);
                                    }
                                }
                                Err(e) => {
                                    log_error!(&log_tx_spawn, "WebSocket handshake failed with {}: {}", addr, e);
                                }
                            }
                        });
                        session_tasks.push(handle);
                    }
                    Err(e) => {
                         log_error!(&event_tx, "Listener accept error: {}", e);
                    }
                }
            }
        }
    }
    
    // Abort all active sessions
    for task in session_tasks {
        task.abort();
    }
    
    Ok(())
}

async fn handle_protocol_session(
    ws_stream: tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
    mut broadcast_rx: tokio::sync::broadcast::Receiver<Frame>,
    event_tx: Sender<SessionEvent>,
    source: Arc<dyn InputSource>,
    last_remote_clip: Arc<Mutex<Option<LocalClipboardContent>>>,
    pending_sends: Arc<Mutex<HashMap<u32, PathBuf>>>,
    broadcast_tx: tokio::sync::broadcast::Sender<Frame>,
) -> Result<()> {
    let (mut ws_sink, mut ws_stream) = ws_stream.split();
    let clip = DefaultClipboard::new();
    let _log_addr = "Remote Client";

    // 1. Protocol Handshake
    log_debug!(&event_tx, "Awaiting application handshake...");
    if let Some(Ok(WsMessage::Binary(bytes))) = ws_stream.next().await {
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
                ws_sink.send(WsMessage::Binary(bincode::serialize(&resp)?)).await?;
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
                    Some(Ok(WsMessage::Binary(bytes))) => {
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
                                    Frame::FileTransferResponse(resp) => {
                                        log_info!(&event_tx, "File transfer response for ID {}: accepted={}", resp.id, resp.accepted);
                                        if resp.accepted {
                                            let mut path_opt = None;
                                            if let Ok(mut lock) = pending_sends.lock() {
                                                path_opt = lock.remove(&resp.id);
                                            }
                                            
                                            if let Some(path) = path_opt {
                                                let broadcast_tx_file = broadcast_tx.clone();
                                                let event_tx_file = event_tx.clone();
                                                let file_id = resp.id;
                                                
                                                tokio::spawn(async move {
                                                    match tokio::fs::File::open(&path).await {
                                                        Ok(mut file) => {
                                                            let mut buffer = vec![0u8; 65536];
                                                            while let Ok(n) = tokio::io::AsyncReadExt::read(&mut file, &mut buffer).await {
                                                                if n == 0 { break; }
                                                                let chunk = buffer[..n].to_vec();
                                                                if broadcast_tx_file.send(Frame::FileData { id: file_id, chunk }).is_err() { break; }
                                                            }
                                                            let _ = broadcast_tx_file.send(Frame::FileEnd { id: file_id });
                                                            log_info!(&event_tx_file, "Server file sender completed for ID: {}", file_id);
                                                        }
                                                        Err(e) => {
                                                            log_error!(&event_tx_file, "Server failed to open file for sending {:?}: {}", path, e);
                                                        }
                                                    }
                                                });
                                            }
                                        }
                                    }
                                    Frame::Heartbeat(hb) => {
                                        let _ = ws_sink.send(WsMessage::Binary(bincode::serialize(&Frame::Heartbeat(hb))?)).await;
                                    }
                                    Frame::Input(event) => {
                                        match event {
                                            platform_passer_core::InputEvent::ScreenSwitch(platform_passer_core::ScreenSide::Local) => {
                                                let _ = source.set_remote(false); // If we were remote, return to local
                                                // If we are a sink, we'd call reset_input here, but Server 
                                                // usually isn't a sink. Still adding for safety.
                                            }
                                            _ => {
                                                // Server usually doesn't inject input received from client
                                            }
                                        }
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
                                                let _ = ws_sink.send(WsMessage::Binary(bincode::serialize(&resp)?)).await;
                                                log_info!(&event_tx, "Accepted file transfer ID: {}", req.id);
                                            }
                                            Err(e) => {
                                                log_error!(&event_tx, "Failed to create file {:?}: {}", file_path, e);
                                                let resp = Frame::FileTransferResponse(platform_passer_core::FileTransferResponse { id: req.id, accepted: false });
                                                let _ = ws_sink.send(WsMessage::Binary(bincode::serialize(&resp)?)).await;
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
                                        // Silent ignore for other frame types to avoid spam
                                    }
                                }
                            }
                            Err(e) => {
                                log_error!(&event_tx, "Failed to deserialize frame: {}", e);
                            }
                        }
                    }
                    Some(Ok(WsMessage::Close(_))) | None => {
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
                        if let Err(e) = ws_sink.send(WsMessage::Binary(bytes)).await {
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
