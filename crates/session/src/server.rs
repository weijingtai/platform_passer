use crate::events::SessionEvent;
use anyhow::Result;
use platform_passer_core::{Frame, InputEvent, ClipboardEvent, FileTransferResponse, write_frame, read_frame};
use platform_passer_transport::{generate_self_signed_cert, make_server_endpoint};
use platform_passer_input::{InputSource, DefaultInputSource};
use platform_passer_clipboard::{ClipboardProvider, DefaultClipboard};
use std::net::SocketAddr;
use tokio::sync::mpsc::Sender;
use tokio::fs::File;
use std::sync::Arc;

pub async fn run_server_session(bind_addr: SocketAddr, event_tx: Sender<SessionEvent>) -> Result<()> {
    tracing::info!("Starting server session on {}", bind_addr);
    let _ = event_tx.send(SessionEvent::Log(format!("Starting server on {}", bind_addr))).await;
    
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
    tracing::info!("QUIC server listening on {}", bind_addr);
    let _ = event_tx.send(SessionEvent::Log(format!("Server listening on {}", bind_addr))).await;
    
    // 3. Accept Loop
    while let Some(conn) = endpoint.accept().await {
        tracing::debug!("New incoming connection from {}", conn.remote_address());
        let event_tx_clone = event_tx.clone();
        
        // Setup Clipboard Provider per connection or global? 
        // For now, let's keep it simple and create one per connection if needed, 
        // but really it should be one global provider. 
        // Since `WindowsClipboard` is a unit struct effectively, it's fine.
        
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
            tracing::error!("Handshake failed with {}: {}", remote_addr, e);
            let _ = event_tx.send(SessionEvent::Error(format!("Handshake failed: {}", e))).await;
            return Err(e.into());
        }
    };
    
    tracing::info!("Handshake completed with {}", remote_addr);
    let _ = event_tx.send(SessionEvent::Log(format!(">>> HANDSHAKE SUCCESS: Remote client connected from {}", remote_addr))).await;
    let _ = event_tx.send(SessionEvent::Connected(remote_addr.to_string())).await;
    
    let clip = DefaultClipboard::new();

    // Allow client to open a bi-directional stream
    tracing::debug!("Awaiting bi-directional protocol stream from {}", remote_addr);
    while let Ok((mut send, mut recv)) = connection.accept_bi().await {
        tracing::info!("Protocol Stream accepted from {}", remote_addr);
        let _ = event_tx.send(SessionEvent::Log("Protocol Stream accepted".into())).await;
        let connection_clone = connection.clone();
        let event_tx_loop = event_tx.clone();

        loop {
            tokio::select! {
                // Read from client
                res = read_frame(&mut recv) => {
                    match res {
                        Ok(Some(Frame::Clipboard(ClipboardEvent::Text(text)))) => {
                            tracing::debug!("Received clipboard text from {}", remote_addr);
                            let _ = event_tx_loop.send(SessionEvent::Log("Received clipboard text.".into())).await;
                            if let Err(e) = clip.set_text(text) {
                               tracing::error!("Failed to set local clipboard: {}", e);
                            }
                        }
                        Ok(Some(Frame::FileTransferRequest(req))) => {
                            tracing::info!("File Request from {}: {} ({})", remote_addr, req.filename, req.file_size);
                            let _ = event_tx_loop.send(SessionEvent::Log(format!("File Request: {} ({})", req.filename, req.file_size))).await;
                            // Auto-accept
                            let resp = Frame::FileTransferResponse(FileTransferResponse {
                                id: req.id,
                                accepted: true,
                            });
                            if let Err(e) = write_frame(&mut send, &resp).await {
                                tracing::error!("Failed to send file transfer response to {}: {}", remote_addr, e);
                                break;
                            }
                            
                            tracing::debug!("Awaiting uni-directional file data stream from {}", remote_addr);
                            if let Ok(mut uni_recv) = connection_clone.accept_uni().await {
                                tracing::info!("File Data Stream accepted from {}", remote_addr);
                                let _ = event_tx_loop.send(SessionEvent::Log("File Data Stream accepted".into())).await;
                                
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
                                    tracing::debug!("Starting file save to {}", file_path_str);
                                    if let Ok(mut file) = File::create(&file_path).await {
                                        if let Ok(n) = tokio::io::copy(&mut uni_recv, &mut file).await {
                                            tracing::info!("File saved: {} ({} bytes)", file_path_str, n);
                                            let _ = tx_file.send(SessionEvent::Log(format!("File saved: {} ({} bytes)", file_path_str, n))).await;
                                        } else {
                                            tracing::error!("Failed to copy file data to {}", file_path_str);
                                        }
                                    } else {
                                        tracing::error!("Failed to create file at {}", file_path_str);
                                    }
                                });
                            }
                        }
                        Ok(Some(_)) => {
                            tracing::warn!("Received unexpected frame from {}", remote_addr);
                        }
                        Ok(None) => {
                            tracing::info!("Protocol stream closed by {}", remote_addr);
                            let _ = event_tx_loop.send(SessionEvent::Log("Stream closed".into())).await;
                            return Ok(());
                        }
                        Err(e) => {
                            tracing::error!("Stream error from {}: {}", remote_addr, e);
                            let _ = event_tx_loop.send(SessionEvent::Error(format!("Stream error: {}", e))).await;
                            return Ok(());
                        }
                    }
                }
                // Send inputs to client
                Ok(event) = input_rx.recv() => {
                    if let Err(e) = write_frame(&mut send, &Frame::Input(event)).await {
                        tracing::error!("Failed to send input event to {}: {}", remote_addr, e);
                        break;
                    }
                }
            }
        }
    }
    
    tracing::info!("Connection with {} closed", remote_addr);
    let _ = event_tx.send(SessionEvent::Disconnected).await;
    Ok(())
}
