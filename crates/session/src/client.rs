use crate::events::{SessionEvent, LogLevel};
use crate::commands::SessionCommand;
use crate::{log_info, log_error, log_debug, log_warn};
use anyhow::Result;
use platform_passer_core::{Frame, ClipboardEvent, FileTransferRequest, write_frame, read_frame};
use platform_passer_transport::{make_client_endpoint};
use platform_passer_input::{InputSink, DefaultInputSink};
use platform_passer_clipboard::{ClipboardProvider, DefaultClipboard};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc::{self, Sender, Receiver};
use tokio::fs::File;


pub async fn run_client_session(
    server_addr: SocketAddr, 
    send_file_path: Option<PathBuf>,
    mut cmd_rx: Receiver<SessionCommand>,
    event_tx: Sender<SessionEvent>
) -> Result<()> {
    log_info!(&event_tx, "Attempting connection to {}", server_addr);

    // 1. Setup QUIC Client
    let endpoint = make_client_endpoint("0.0.0.0:0".parse()?)?;
    log_debug!(&event_tx, "QUIC Client endpoint bound. Starting handshake...");
    let connection = match endpoint.connect(server_addr, "localhost")?.await {
        Ok(c) => c,
        Err(e) => {
            log_error!(&event_tx, "QUIC Connection failed: {}", e);
            let _ = event_tx.send(SessionEvent::Error(format!("Connection failed: {}", e))).await;
            return Err(e.into());
        }
    };
    
    log_info!(&event_tx, "Handshake completed with {}", connection.remote_address());
    let _ = event_tx.send(SessionEvent::Connected(connection.remote_address().to_string())).await;

    // 2. Setup Input Sink (Client receives inputs and injects them)
    let sink = Arc::new(DefaultInputSink::new());
    log_debug!(&event_tx, "Local input sink initialized.");

    let (mut send, mut recv) = connection.open_bi().await?;
    log_debug!(&event_tx, "Main protocol stream opened successfully.");
    
    // 3. Setup Channel for Mixed Events
    let (tx, mut rx) = mpsc::channel::<Frame>(100);

    // 5. Spawn Read Loop in Background
    let event_tx_read = event_tx.clone();
    let sink_read = sink.clone();
    tokio::spawn(async move {
        read_frame_loop(recv, event_tx_read, sink_read).await;
    });

    // 6. Start Clipboard Listener
    let clip = DefaultClipboard::new();
    let tx_clip = tx.clone();
    let clip_reader = DefaultClipboard::new();
    
    clip.start_listener(Box::new(move || {
        if let Ok(text) = clip_reader.get_text() {
             if !text.is_empty() {
                 let _ = tx_clip.blocking_send(Frame::Clipboard(ClipboardEvent::Text(text)));
             }
        }
    }))?;
    log_info!(&event_tx, "Local clipboard synchronization active.");

    // 7. Main Outbound Loop
    loop {
        tokio::select! {
            // Priority 1: Commands from UI/CLI
            Some(cmd) = cmd_rx.recv() => {
                match cmd {
                    SessionCommand::SendFile(path) => {
                         if let Err(e) = perform_file_send(&connection, path, &event_tx).await {
                             let _ = event_tx.send(SessionEvent::Error(format!("File send error: {}", e))).await;
                         }
                    }
                    SessionCommand::Disconnect => break,
                }
            }
            // Priority 2: Outbound Frames (Clipboard, etc.)
            Some(frame) = rx.recv() => {
                if let Err(e) = write_frame(&mut send, &frame).await {
                    log_error!(&event_tx, "Failed to send frame: {}", e);
                    break;
                }
            }
        }
    }
    
    log_info!(&event_tx, "Client session terminating...");
    let _ = event_tx.send(SessionEvent::Disconnected).await;
    Ok(())
}

async fn perform_file_send(
    connection: &quinn::Connection, 
    path: PathBuf, 
    event_tx: &Sender<SessionEvent>
) -> Result<()> {
    log_info!(event_tx, "Initiating file transfer for: {:?}", path);
    if let Ok(metadata) = tokio::fs::metadata(&path).await {
        let filename = path.file_name().unwrap_or_default().to_string_lossy().to_string();
        let size = metadata.len();
        
        let req = Frame::FileTransferRequest(FileTransferRequest {
            id: rand::random(),
            filename,
            file_size: size,
        });

        // Open new bi-stream for negotiation to avoid race with main loop
        log_debug!(event_tx, "Opening new bi-stream for file transfer negotiation");
        let (mut req_send, mut req_recv) = connection.open_bi().await?;
        write_frame(&mut req_send, &req).await?;
        
        // Wait for response on THIS stream
        log_debug!(event_tx, "Awaiting file transfer response");
        match read_frame(&mut req_recv).await? {
             Some(Frame::FileTransferResponse(resp)) => {
                if resp.accepted {
                     log_info!(event_tx, "File transfer request accepted. Uploading...");
                     let mut uni_send = connection.open_uni().await?;
                     let mut file = File::open(&path).await?;
                     let n = tokio::io::copy(&mut file, &mut uni_send).await?;
                     uni_send.finish().await?;
                     log_info!(event_tx, "File upload complete: {} bytes.", n);
                } else {
                     log_warn!(event_tx, "File transfer request was rejected by server.");
                     let _ = event_tx.send(SessionEvent::Error("Transfer rejected.".into())).await;
                }
             }
             _ => {
                 log_error!(event_tx, "Received invalid response for file transfer");
                 let _ = event_tx.send(SessionEvent::Error("Invalid response for file transfer.".into())).await;
             }
        }
    } else {
        log_error!(event_tx, "Failed to get metadata for file: {:?}", path);
    }
    Ok(())
}

async fn read_frame_loop(
    mut recv: quinn::RecvStream, 
    event_tx: Sender<SessionEvent>, 
    sink: Arc<DefaultInputSink>
) {
    log_debug!(&event_tx, "Starting frame read loop");
    loop {
         match read_frame(&mut recv).await {
             Ok(Some(Frame::Input(event))) => {
                 if let Err(e) = sink.inject_event(event) {
                     log_debug!(&event_tx, "Warning: Failed to inject input event: {}", e);
                 }
             }
             Ok(Some(Frame::Clipboard(ClipboardEvent::Text(text)))) => {
                 log_info!(&event_tx, "Updating local clipboard from server synchronization.");
                 let clip = DefaultClipboard::new();
                 if let Err(e) = clip.set_text(text) {
                     log_error!(&event_tx, "Fatal: Clipboard update failed: {}", e);
                 }
             }
             Ok(Some(_)) => {
                 log_debug!(&event_tx, "Received unknown frame type from server.");
             },
             Ok(None) => {
                 log_info!(&event_tx, "Inbound protocol stream closed by server.");
                 return;
             },
             Err(e) => {
                 log_error!(&event_tx, "Inbound protocol stream error: {}", e);
                 return;
             },
         }
    }
}
