use crate::events::SessionEvent;
use crate::commands::SessionCommand;
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
    tracing::info!("Starting client session to {}", server_addr);
    let _ = event_tx.send(SessionEvent::Log(format!("Connecting to {}", server_addr))).await;

    // 1. Setup QUIC Client
    let endpoint = make_client_endpoint("0.0.0.0:0".parse()?)?;
    tracing::debug!("Awaiting handshake with {}", server_addr);
    let _ = event_tx.send(SessionEvent::Log("Awaiting handshake...".into())).await;
    
    let connection = match endpoint.connect(server_addr, "localhost")?.await {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("Connection to {} failed: {}", server_addr, e);
            let _ = event_tx.send(SessionEvent::Error(format!("Connection failed: {}", e))).await;
            return Err(e.into());
        }
    };
    
    tracing::info!("Connected to {}!", server_addr);
    let _ = event_tx.send(SessionEvent::Connected(connection.remote_address().to_string())).await;
    let _ = event_tx.send(SessionEvent::Log("Connected!".into())).await;

    tracing::debug!("Opening bi-directional protocol stream to {}", server_addr);
    let (mut send, mut recv) = connection.open_bi().await?;
    tracing::info!("Protocol Stream opened to {}", server_addr);
    let _ = event_tx.send(SessionEvent::Log("Protocol Stream opened".into())).await;
    
    // 2.5 Handle Initial File Send (Legacy M4 CLI support)
    // We can just queue it? Or run it.
    if let Some(path) = send_file_path {
        tracing::info!("Initial file send requested: {:?}", path);
        // reuse perform_file_send but we need streams.
        // For MVP, since we changed protocol to use NEW stream for request (see above), 
        // we can just call it.
        // Wait, perform_file_send creates new stream.
        let _ = connection.open_uni().await?; // Hack to satisfy type if we passed refs, but we changed sig.
        // Actually perform_file_send takes connection.
        
        // We don't need 'send'/'recv' args if we open new streams?
        // Let's update perform_file_send to NOT take send/recv streams, but open its own.
        // Correct.
        perform_file_send(&connection, path, &event_tx).await?;
        return Ok(());
    }

    // 3. Setup Channel for Mixed Events
    let (tx, mut rx) = mpsc::channel::<Frame>(100);

    // 2. Setup Input Sink (Client receives inputs and injects them)
    let sink = Arc::new(DefaultInputSink::new());
    let _ = event_tx.send(SessionEvent::Log("Input sink ready.".into())).await;
    
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
    let _ = event_tx.send(SessionEvent::Log("Clipboard listener started.".into())).await;

    // 5. Main Loop
    loop {
        tokio::select! {
            // Priority 1: Commands
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
            // Priority 2: Input Events
            Some(frame) = rx.recv() => {
                if let Err(e) = write_frame(&mut send, &frame).await {
                    let _ = event_tx.send(SessionEvent::Error(format!("Send error: {}", e))).await;
                    break;
                }
            }
            // Priority 3: Keepalive/Incoming? 
            // We need to read 'recv' for inputs and clipboard
             _ = read_frame_loop(&mut recv, &event_tx, sink.clone()) => {
                 // If this returns, stream closed or error
                 break;
             }
        }
    }
    
    let _ = event_tx.send(SessionEvent::Disconnected).await;
    Ok(())
}

async fn perform_file_send(
    connection: &quinn::Connection, 
    path: PathBuf, 
    event_tx: &Sender<SessionEvent>
) -> Result<()> {
    tracing::info!("Sending file: {:?}", path);
    let _ = event_tx.send(SessionEvent::Log(format!("Sending file: {:?}", path))).await;
    if let Ok(metadata) = tokio::fs::metadata(&path).await {
        let filename = path.file_name().unwrap_or_default().to_string_lossy().to_string();
        let size = metadata.len();
        
        let req = Frame::FileTransferRequest(FileTransferRequest {
            id: rand::random(),
            filename,
            file_size: size,
        });

        // Open new bi-stream for negotiation to avoid race with main loop
        tracing::debug!("Opening new bi-stream for file transfer negotiation");
        let (mut req_send, mut req_recv) = connection.open_bi().await?;
        write_frame(&mut req_send, &req).await?;
        
        // Wait for response on THIS stream
        tracing::debug!("Awaiting file transfer response");
        match read_frame(&mut req_recv).await? {
             Some(Frame::FileTransferResponse(resp)) => {
                if resp.accepted {
                     tracing::info!("File transfer accepted by remote");
                     let _ = event_tx.send(SessionEvent::Log("Transfer accepted. Sending data...".into())).await;
                     let mut uni_send = connection.open_uni().await?;
                     let mut file = File::open(&path).await?;
                     tracing::debug!("Copying file data to uni-stream");
                     tokio::io::copy(&mut file, &mut uni_send).await?;
                     uni_send.finish().await?;
                     tracing::info!("File sent successfully: {}", path.display());
                     let _ = event_tx.send(SessionEvent::Log("File sent successfully.".into())).await;
                } else {
                     tracing::warn!("File transfer rejected by remote");
                     let _ = event_tx.send(SessionEvent::Error("Transfer rejected.".into())).await;
                }
             }
             _ => {
                 tracing::error!("Received invalid response for file transfer");
                 let _ = event_tx.send(SessionEvent::Error("Invalid response for file transfer.".into())).await;
             }
        }
    } else {
        tracing::error!("Failed to get metadata for file: {:?}", path);
    }
    Ok(())
}

async fn read_frame_loop(
    recv: &mut quinn::RecvStream, 
    _event_tx: &Sender<SessionEvent>, 
    sink: Arc<DefaultInputSink>
) {
    let _remote_addr = "remote"; // We don't have it here easily, but let's just log
    tracing::debug!("Starting frame read loop");
    loop {
         match read_frame(recv).await {
             Ok(Some(Frame::Input(event))) => {
                 tracing::trace!("Received input event: {:?}", event);
                 if let Err(e) = sink.inject_event(event) {
                     tracing::error!("Failed to inject input event: {}", e);
                 }
             }
             Ok(Some(Frame::Clipboard(ClipboardEvent::Text(text)))) => {
                 tracing::debug!("Received clipboard text");
                 let clip = DefaultClipboard::new();
                 let _ = clip.set_text(text);
             }
             Ok(Some(_)) => {
                 tracing::warn!("Received unexpected frame");
             },
             Ok(None) => {
                 tracing::info!("Read frame loop: Stream closed");
                 return;
             }
             Err(e) => {
                 tracing::error!("Read frame loop error: {}", e);
                 return;
             }
         }
    }
}
