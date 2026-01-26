use crate::events::{SessionEvent, LogLevel};
use crate::commands::SessionCommand;
use crate::log_error;
use anyhow::Result;
use platform_passer_core::{Frame, ClipboardEvent, Handshake, Heartbeat};
use platform_passer_transport::connect_ws;
use platform_passer_input::{InputSink, DefaultInputSink, InputSource, DefaultInputSource};
use platform_passer_clipboard::{ClipboardProvider, DefaultClipboard};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc::{self, Sender, Receiver};
use std::time::Duration;
use crate::clipboard_utils::{LocalClipboardContent, calculate_hash};
use tokio_tungstenite::tungstenite::Message;
use futures_util::{StreamExt, SinkExt};
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use platform_passer_core::{FileManifest, FileMeta, TransferPurpose};
use std::collections::HashMap;

enum SessionInternalMsg {
    SendClipboardFiles { batch_id: u64, files: Vec<PathBuf> },
}


pub async fn run_client_session(
    server_addr: SocketAddr, 
    _send_file_path: Option<PathBuf>,
    mut cmd_rx: Receiver<SessionCommand>,
    event_tx: Sender<SessionEvent>
) -> Result<()> {
    // 1. Persistent Setup (Clipboard & Input Sink & Input Source)
    let (local_tx, mut local_rx) = mpsc::channel::<Frame>(1024); // Increased capacity for input buffering
    let (internal_tx, mut internal_rx) = mpsc::channel::<SessionInternalMsg>(100);
    let sink = Arc::new(DefaultInputSink::new());
    let source = Arc::new(DefaultInputSource::new());
    let _ = sink.reset_input();

    // Start Clipboard Listener Once
    let clip_tx = local_tx.clone();
    let clip_log = event_tx.clone();
    let clipboard = DefaultClipboard::new();
    
    let last_remote_clip = Arc::new(Mutex::new(None::<LocalClipboardContent>));
    let last_remote_clip_listener = last_remote_clip.clone();

    let internal_tx_clip = internal_tx.clone();
    if let Err(e) = clipboard.start_listener(Box::new(move || {
        let clip = DefaultClipboard::new();

        // Priority 1: Files
        if let Ok(Some(files)) = clip.get_files() {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            use std::hash::Hash; use std::hash::Hasher;
            files.hash(&mut hasher);
            let files_hash = hasher.finish();

            let should_send = if let Ok(lock) = last_remote_clip_listener.lock() {
                match &*lock {
                    Some(LocalClipboardContent::Files(last_hash)) => *last_hash != files_hash,
                    _ => true,
                }
            } else { true };

            if should_send {
                 let mut total_size = 0;
                 let mut file_metas = Vec::new();
                 for path_str in &files {
                     let path = std::path::PathBuf::from(path_str);
                     if let Ok(meta) = std::fs::metadata(&path) {
                         if meta.is_file() {
                             total_size += meta.len();
                             file_metas.push(FileMeta {
                                 name: path.file_name().unwrap_or_default().to_string_lossy().to_string(),
                                 size: meta.len(),
                             });
                         }
                     }
                 }

                 if total_size > 0 {
                     if total_size > 10 * 1024 * 1024 {
                         let _ = clip_tx.blocking_send(Frame::Notification { 
                             title: "Clipboard Sync Skipped".to_string(), 
                             message: "files > 10MB".to_string() 
                         });
                     } else {
                         let batch_id = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos() as u64;
                         let manifest = FileManifest { files: file_metas, total_size, batch_id };
                         let _ = clip_tx.try_send(Frame::Clipboard(ClipboardEvent::Files { manifest }));
                         let _ = internal_tx_clip.try_send(SessionInternalMsg::SendClipboardFiles { batch_id, files: files.iter().map(PathBuf::from).collect() });
                     }
                  }
                  return;
             } else {
                 return;
             }
         }
        
        // Priority 2: Text
        if let Ok(text) = clip.get_text() {
            if !text.is_empty() {
                let should_send = if let Ok(lock) = last_remote_clip_listener.lock() {
                    match &*lock {
                        Some(LocalClipboardContent::Text(last)) => *last != text,
                        _ => true,
                    }
                } else { true };

                if should_send {
                     let _ = clip_tx.try_send(Frame::Clipboard(ClipboardEvent::Text(text)));
                }
                return;
            }
        }
        
        // Priority 3: Image
        if let Ok(Some(img_data)) = clip.get_image() {
            let img_hash = calculate_hash(&img_data);
             let should_send = if let Ok(lock) = last_remote_clip_listener.lock() {
                match &*lock {
                    Some(LocalClipboardContent::Image(last_hash)) => *last_hash != img_hash,
                    _ => true,
                }
            } else { true };
            
            if should_send {
                 let _ = clip_tx.try_send(Frame::Clipboard(ClipboardEvent::Image { data: img_data }));
            }
        }
     })) {
        log_error!(&clip_log, "Failed to start clipboard listener: {}", e);
    }

    // Capture input
    let input_tx = local_tx.clone();
    let input_log = event_tx.clone();
    if let Err(e) = source.start_capture(Box::new(move |event| {
        // Use try_send to avoid blocking the input hook thread
        if input_tx.try_send(Frame::Input(event)).is_err() {
            // Log once in a while or just ignore overflow for MouseMove
        }
    })) {
        log_error!(&input_log, "Failed to start input capture: {}", e);
    }
    
    let _ = source.set_remote(false);

    let mut backoff = Duration::from_secs(1);
    let max_backoff = Duration::from_secs(30);

    // Initial Connecting state
    let _ = event_tx.send(SessionEvent::Connecting(server_addr.to_string())).await;

    loop {
        // Enforce timeout on connection attempt
        let connect_fut = tokio::time::timeout(Duration::from_secs(5), connect_ws(server_addr));
        let stream_result = tokio::select! {
            res = connect_fut => {
                match res {
                    Ok(inner) => inner,
                    Err(_) => Err(anyhow::anyhow!("Connection timed out")),
                }
            },
            Some(cmd) = cmd_rx.recv() => {
                if matches!(cmd, SessionCommand::Disconnect) { return Ok(()); }
                continue; 
            }
        };

        match stream_result {
            Ok(ws_stream) => {
                backoff = Duration::from_secs(1);
                let _ = event_tx.send(SessionEvent::Connected(server_addr.to_string())).await;

                let (mut ws_sink, mut ws_stream) = ws_stream.split();
                let clip = DefaultClipboard::new();
                
                // Handshake
                let screen_info = {
                    #[cfg(target_os = "macos")] { platform_passer_input::get_screen_info() }
                    #[cfg(not(target_os = "macos"))] { None }
                };

                let handshake = Frame::Handshake(Handshake {
                    version: 1,
                    client_id: format!("{}-client", std::env::consts::OS),
                    capabilities: vec!["input".to_string(), "clipboard".to_string()],
                    screen_info,
                });

                if let Err(e) = ws_sink.send(Message::Binary(bincode::serialize(&handshake)?)).await {
                    log_error!(&event_tx, "Handshake send failed: {}", e);
                    continue;
                }

                let mut active_files: HashMap<u32, File> = HashMap::new();
                let mut pending_sends: HashMap<u32, PathBuf> = HashMap::new();
                let mut incoming_batches: HashMap<u64, (usize, Vec<PathBuf>)> = HashMap::new();
                let mut active_downloads: HashMap<u32, (u64, PathBuf)> = HashMap::new();
                let mut file_id_counter = 0u32;
                
                let (hb_stop_tx, mut hb_stop_rx) = mpsc::channel::<()>(1);
                let hb_local_tx = local_tx.clone();
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

                loop {
                    tokio::select! {
                        Some(frame) = local_rx.recv() => {
                            let bytes = bincode::serialize(&frame)?;
                            if let Err(e) = ws_sink.send(Message::Binary(bytes)).await {
                                log_error!(&event_tx, "Send failed: {}", e);
                                break;
                            }
                        }
                        Some(msg) = internal_rx.recv() => {
                            match msg {
                                SessionInternalMsg::SendClipboardFiles { batch_id, files } => {
                                    for path in files {
                                        if path.exists() {
                                            file_id_counter += 1;
                                            let id = file_id_counter;
                                            let filename = path.file_name().unwrap_or_default().to_string_lossy().to_string();
                                            let file_size = path.metadata().map(|m| m.len()).unwrap_or(0);
                                            pending_sends.insert(id, path.clone());
                                            let req = Frame::FileTransferRequest(platform_passer_core::FileTransferRequest {
                                                id, filename, file_size, purpose: TransferPurpose::ClipboardSync { batch_id },
                                            });
                                             // Use try_send to avoid deadlock in main loop
                                             let _ = local_tx.try_send(req);
                                        }
                                    }
                                }
                            }
                        }
                        msg_opt = tokio::time::timeout(Duration::from_secs(15), ws_stream.next()) => {
                            match msg_opt {
                                Ok(Some(Ok(Message::Binary(bytes)))) => {
                                    if let Ok(frame) = bincode::deserialize::<Frame>(&bytes) {
                                        match frame {
                                            Frame::Input(event) => {
                                                match event {
                                                    platform_passer_core::InputEvent::ScreenSwitch(_) => {
                                                        // When Client receives focus, ensure it stays in Local mode (not swallowing)
                                                        let _ = sink.reset_input(); 
                                                        let _ = source.set_remote(false);
                                                    }
                                                    _ => { let _ = sink.inject_event(event); }
                                                }
                                            }
                                            Frame::Clipboard(ClipboardEvent::Text(text)) => {
                                                if let Ok(mut lock) = last_remote_clip.lock() {
                                                    *lock = Some(LocalClipboardContent::Text(text.clone()));
                                                }
                                                let _ = clip.set_text(text);
                                            }
                                            Frame::Clipboard(ClipboardEvent::Image { data }) => {
                                                let hash = calculate_hash(&data);
                                                if let Ok(mut lock) = last_remote_clip.lock() {
                                                    *lock = Some(LocalClipboardContent::Image(hash));
                                                }
                                                let _ = clip.set_image(data);
                                            }
                                            Frame::Clipboard(ClipboardEvent::Files { manifest }) => {
                                                incoming_batches.insert(manifest.batch_id, (manifest.files.len(), Vec::new()));
                                            }
                                            Frame::Notification { title, message } => {
                                                let _ = event_tx.send(SessionEvent::Log { level: LogLevel::Info, message: format!("Remote Notification: {} - {}", title, message) }).await;
                                            }
                                            Frame::FileTransferRequest(req) => {
                                                let (should_dload, save_dir, batch_id_opt) = match req.purpose {
                                                    TransferPurpose::Manual => (true, std::path::PathBuf::from("downloads"), None),
                                                    TransferPurpose::ClipboardSync { batch_id } => {
                                                        (true, std::env::temp_dir().join(format!("platform_passer_clip_{}", batch_id)), Some(batch_id))
                                                    }
                                                };
                                                if should_dload {
                                                    let _ = tokio::fs::create_dir_all(&save_dir).await;
                                                    let file_path = save_dir.join(&req.filename);
                                                    match File::create(&file_path).await {
                                                        Ok(file) => {
                                                            active_files.insert(req.id, file);
                                                            if let Some(bid) = batch_id_opt { active_downloads.insert(req.id, (bid, file_path)); }
                                                            let _ = ws_sink.send(Message::Binary(bincode::serialize(&Frame::FileTransferResponse(platform_passer_core::FileTransferResponse { id: req.id, accepted: true }))?)).await;
                                                        }
                                                        Err(_) => {
                                                            let _ = ws_sink.send(Message::Binary(bincode::serialize(&Frame::FileTransferResponse(platform_passer_core::FileTransferResponse { id: req.id, accepted: false }))?)).await;
                                                        }
                                                    }
                                                }
                                            }
                                            Frame::FileData { id, chunk } => {
                                                if let Some(file) = active_files.get_mut(&id) { let _ = file.write_all(&chunk).await; }
                                            }
                                            Frame::FileEnd { id } => {
                                                if let Some(mut file) = active_files.remove(&id) {
                                                    let _ = file.flush().await;
                                                    if let Some((batch_id, path)) = active_downloads.remove(&id) {
                                                        if let Some((remaining, paths)) = incoming_batches.get_mut(&batch_id) {
                                                            paths.push(path);
                                                            *remaining -= 1;
                                                            if *remaining == 0 {
                                                                let final_paths: Vec<String> = paths.iter().map(|p| p.to_string_lossy().to_string()).collect();
                                                                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                                                                use std::hash::Hash; use std::hash::Hasher;
                                                                final_paths.hash(&mut hasher);
                                                                if let Ok(mut lock) = last_remote_clip.lock() { *lock = Some(LocalClipboardContent::Files(hasher.finish())); }
                                                                let _ = clip.set_files(final_paths);
                                                                incoming_batches.remove(&batch_id);
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                            Frame::FileTransferResponse(resp) => {
                                                if resp.accepted {
                                                    if let Some(path) = pending_sends.remove(&resp.id) {
                                                        let local_tx_file = local_tx.clone();
                                                        let file_id = resp.id;
                                                        tokio::spawn(async move {
                                                            if let Ok(mut file) = tokio::fs::File::open(&path).await {
                                                                let mut buffer = vec![0u8; 65536];
                                                                while let Ok(n) = tokio::io::AsyncReadExt::read(&mut file, &mut buffer).await {
                                                                    if n == 0 { break; }
                                                                    let _ = local_tx_file.send(Frame::FileData { id: file_id, chunk: buffer[..n].to_vec() }).await;
                                                                }
                                                                let _ = local_tx_file.send(Frame::FileEnd { id: file_id }).await;
                                                            }
                                                        });
                                                    }
                                                } else { pending_sends.remove(&resp.id); }
                                            }
                                            Frame::Heartbeat(hb) => { let _ = ws_sink.send(Message::Binary(bincode::serialize(&Frame::Heartbeat(hb))?)).await; }
                                            _ => {}
                                        }
                                    }
                                }
                                Ok(Some(Ok(Message::Close(_)))) | Ok(None) => break,
                                Ok(Some(Err(e))) => {
                                    log_error!(&event_tx, "WebSocket Error: {}", e);
                                    break;
                                }
                                Err(_) => {
                                    log_error!(&event_tx, "Server timed out (no heartbeat).");
                                    break;
                                }
                                _ => {}
                            }
                        }
                        Some(cmd) = cmd_rx.recv() => {
                            match cmd {
                                SessionCommand::SendFile(path) => {
                                    if path.exists() {
                                        file_id_counter += 1;
                                        let id = file_id_counter;
                                        let filename = path.file_name().unwrap_or_default().to_string_lossy().to_string();
                                        let file_size = path.metadata().map(|m| m.len()).unwrap_or(0);
                                        pending_sends.insert(id, path.clone());
                                        let req = Frame::FileTransferRequest(platform_passer_core::FileTransferRequest {
                                            id, filename, file_size, purpose: TransferPurpose::Manual,
                                        });
                                         // Use try_send to avoid deadlock in main loop
                                         let _ = local_tx.try_send(req);
                                    }
                                },
                                SessionCommand::Disconnect => {
                                    let _ = hb_stop_tx.send(()).await;
                                    let _ = ws_sink.close().await;
                                    return Ok(());
                                },
                                SessionCommand::UpdateConfig(config) => {
                                    let _ = sink.update_config(config.clone());
                                    let _ = source.update_config(config);
                                },
                            }
                        }
                    }
                }
                let _ = source.set_remote(false);
                let _ = sink.reset_input();
                let _ = hb_stop_tx.send(()).await;
                // Don't send Disconnected here, we will Reconnect
                // let _ = event_tx.send(SessionEvent::Disconnected).await;
            }
            Err(_) => { 
                let _ = event_tx.send(SessionEvent::Reconnecting(server_addr.to_string())).await;
                tokio::time::sleep(backoff).await; 
                backoff = std::cmp::min(backoff * 2, max_backoff); 
            }
        }
    }
}
