use crate::events::SessionEvent;
use crate::{log_info, log_error, log_debug};
use anyhow::Result;
use platform_passer_core::{Frame, InputEvent, ClipboardEvent, Handshake, FileTransferResponse, write_frame, read_frame};
use platform_passer_transport::{generate_self_signed_cert, make_server_endpoint};
use platform_passer_input::{InputSource, DefaultInputSource};
use platform_passer_clipboard::{ClipboardProvider, DefaultClipboard};
use std::net::SocketAddr;
use tokio::sync::mpsc::Sender;
use tokio::fs::File;
use std::sync::Arc;

pub async fn run_server_session(bind_addr: SocketAddr, event_tx: Sender<SessionEvent>) -> Result<()> {
    log_info!(&event_tx, "Starting server session on {}", bind_addr);
    
    // 1. Setup Input Source (Server captures local input)
    let source = Arc::new(DefaultInputSource::new());
    let (input_tx, _input_rx) = tokio::sync::broadcast::channel::<InputEvent>(100);
    let input_tx_captured = input_tx.clone();
    
    source.start_capture(Box::new(move |event| {
        let _ = input_tx_captured.send(event);
    }))?;
    
    // 2. Setup QUIC Server
    let cert = generate_self_signed_cert(vec!["localhost".to_string()])?;
    let endpoint = make_server_endpoint(bind_addr, &cert)?;
    log_info!(&event_tx, "QUIC Server listening on {}", bind_addr);
    
    // 3. Accept Loop
    while let Some(conn) = endpoint.accept().await {
        log_debug!(&event_tx, "New incoming QUIC connection from {}", conn.remote_address());
        let event_tx_clone = event_tx.clone();
        
        let input_rx = input_tx.subscribe();
        
        tokio::spawn(async move {
            match handle_connection(conn, input_rx, event_tx_clone).await {
                Ok(_) => tracing::info!("Connection handled successfully"),
                Err(e) => tracing::error!("Error handling connection: {}", e),
            }
        });
    }
    tracing::info!("Server session loop terminated");
    Ok(())
}

async fn handle_connection(
    conn: quinn::Connecting, 
    mut input_rx: tokio::sync::broadcast::Receiver<InputEvent>, 
    event_tx: Sender<SessionEvent>
) -> Result<()> {
    let remote_addr = conn.remote_address();
    tracing::debug!("Awaiting QUIC handshake from {}", remote_addr);
    
    let connection = match conn.await {
        Ok(c) => c,
        Err(e) => {
            log_error!(&event_tx, "QUIC Handshake failed: {}", e);
            let _ = event_tx.send(SessionEvent::Error(format!("Handshake failed: {}", e))).await;
            return Err(e.into());
        }
    };
    
    log_info!(&event_tx, "QUIC Handshake completed with {}", remote_addr);
    let _ = event_tx.send(SessionEvent::Connected(remote_addr.to_string())).await;
    
    let clip = DefaultClipboard::new();

    // Allow client to open a bi-directional stream
    log_debug!(&event_tx, "Awaiting bi-directional protocol stream from {}", remote_addr);
    while let Ok((mut send, mut recv)) = connection.accept_bi().await {
        log_debug!(&event_tx, "New protocol stream accepted from {}. Performing application handshake...", remote_addr);
        
        // Protocol Handshake
        match read_frame(&mut recv).await? {
            Some(Frame::Handshake(h)) => {
                log_info!(&event_tx, "Received handshake from {} (Client: {})", remote_addr, h.client_id);
                // Send our handshake back
                let resp = Frame::Handshake(Handshake {
                    version: 1,
                    client_id: "macos-server".to_string(),
                    capabilities: vec!["input".to_string(), "clipboard".to_string(), "file-transfer".to_string()],
                });
                write_frame(&mut send, &resp).await?;
            }
            _ => {
                log_error!(&event_tx, "Application handshake failed with {}", remote_addr);
                break;
            }
        }

        let connection_clone = connection.clone();
        let event_tx_loop = event_tx.clone();

        loop {
            tokio::select! {
                // Read from client
                res = read_frame(&mut recv) => {
                    match res {
                        Ok(Some(Frame::Clipboard(ClipboardEvent::Text(text)))) => {
                            log_info!(&event_tx_loop, "Received clipboard text from {}", remote_addr);
                            if let Err(e) = clip.set_text(text) {
                               log_error!(&event_tx_loop, "Failed to set clipboard: {}", e);
                            }
                        }
                        Ok(Some(Frame::Heartbeat(hb))) => {
                            // Echo back heartbeats
                            let _ = write_frame(&mut send, &Frame::Heartbeat(hb)).await;
                        }
                        Ok(Some(Frame::FileTransferRequest(req))) => {
                            log_info!(&event_tx_loop, "Incoming file request: {} ({} bytes)", req.filename, req.file_size);
                            // Auto-accept
                            let resp = Frame::FileTransferResponse(FileTransferResponse {
                                id: req.id,
                                accepted: true,
                            });
                            if let Err(e) = write_frame(&mut send, &resp).await {
                                log_error!(&event_tx_loop, "Failed to send file transfer response: {}", e);
                                break;
                            }
                            
                            log_debug!(&event_tx_loop, "File data stream accepted for {}", req.filename);
                            if let Ok(mut uni_recv) = connection_clone.accept_uni().await {
                                
                                // Sanitize filename and ensure downloads dir exists
                                let safe_filename = std::path::Path::new(&req.filename)
                                    .file_name()
                                    .unwrap_or_default()
                                    .to_string_lossy();
                                
                                let download_dir = std::path::Path::new("downloads");
                                let _ = tokio::fs::create_dir_all(download_dir).await;
                                
                                let file_path = download_dir.join(format!("{}", safe_filename));
                                let file_path_str = file_path.to_string_lossy().to_string();
        
                                let tx_file = event_tx_loop.clone();
                                
                                tokio::spawn(async move {
                                    if let Ok(mut file) = File::create(&file_path).await {
                                        match tokio::io::copy(&mut uni_recv, &mut file).await {
                                            Ok(n) => {
                                                log_info!(&tx_file, "File saved successfully: {} ({} bytes)", file_path_str, n);
                                            }
                                            Err(e) => {
                                                log_error!(&tx_file, "Failed to save file {}: {}", file_path_str, e);
                                            }
                                        }
                                    }
                                });
                            }
                        }
                        Ok(Some(_)) => {}
                        Ok(None) => {
                            log_info!(&event_tx_loop, "Protocol stream closed by {}", remote_addr);
                            break;
                        }
                        Err(e) => {
                            log_error!(&event_tx_loop, "Protocol stream error from {}: {}", remote_addr, e);
                            break;
                        }
                    }
                }
                // Send inputs to client
                Ok(event) = input_rx.recv() => {
                    if let platform_passer_core::InputEvent::ScreenSwitch(side) = event {
                        log_info!(&event_tx_loop, "Screen focus switched to {:?}", side);
                    }
                    if let Err(e) = write_frame(&mut send, &Frame::Input(event)).await {
                        log_debug!(&event_tx_loop, "Failed to send input to client: {}", e);
                        break;
                    }
                }
            }
        }
    }
    
    let _ = event_tx.send(SessionEvent::Disconnected).await;
    Ok(())
}
