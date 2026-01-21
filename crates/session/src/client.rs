use crate::events::SessionEvent;
use crate::commands::SessionCommand;
use anyhow::Result;
use platform_passer_core::{Frame, InputEvent, ClipboardEvent, FileTransferRequest, FileTransferResponse, write_frame, read_frame};
use platform_passer_transport::{make_client_endpoint};
use platform_passer_input::{InputSource, DefaultInputSource};
use platform_passer_clipboard::{ClipboardProvider, DefaultClipboard};
use std::net::SocketAddr;
use std::path::PathBuf;
use tokio::sync::mpsc::{self, Sender, Receiver};
use tokio::fs::File;

async fn perform_file_send(
    connection: &quinn::Connection, 
    path: PathBuf, 
    send: &mut quinn::SendStream, 
    recv: &mut quinn::RecvStream,
    event_tx: &Sender<SessionEvent>
) -> Result<()> {
    let _ = event_tx.send(SessionEvent::Log(format!("Sending file: {:?}", path))).await;
    if let Ok(metadata) = tokio::fs::metadata(&path).await {
        let filename = path.file_name().unwrap_or_default().to_string_lossy().to_string();
        let size = metadata.len();
        
        let req = Frame::FileTransferRequest(FileTransferRequest {
            id: rand::random(),
            filename,
            file_size: size,
        });
        write_frame(send, &req).await?;
        
        // We need to read the response. 
        // CAUTION: This assumes the next frame is the response. 
        // usage of 'recv' here might race if we are also reading inputs? 
        // For M7 MVP, we assume lock-step for file transfer or mixed frames is handled by the caller not reading?
        // Actually, 'recv' is the global stream recv. If we are in the main loop, we shouldn't steal from it?
        // Protocol flaw: Control stream is shared.
        // Quick Fix: We assume response comes fast.
        
        // Actually, better approach: The main loop reads frames. If it sees FileTransferResponse, it handles it.
        // But here we want to block "send file" action until accepted?
        // Let's rely on the main loop for responses? 
        // No, keep it simple for now: Client initiates transfer, Server responds immediately.
        // But if we are in the main loop, we can't use 'recv' easily if it's being polled by select!.
        
        // Alternative: Open a NEW bi-stream for file transfer negotiation? 
        // Yes, that's cleaner. 
        // M4 Implementation used the MAIN stream.
        // M7 Refactor: Use a dedicated bi-stream for the negotiation AND data?
        
        // Let's stick to the M4 plan which used the main stream but it was lock-step.
        // If we want dynamic, we should open a new stream for the request.
        
        let (mut req_send, mut req_recv) = connection.open_bi().await?;
        write_frame(&mut req_send, &req).await?;
        
        match read_frame(&mut req_recv).await? {
             Some(Frame::FileTransferResponse(resp)) => {
                if resp.accepted {
                     let _ = event_tx.send(SessionEvent::Log("Transfer accepted. Sending data...".into())).await;
                     let mut uni_send = connection.open_uni().await?;
                     let mut file = File::open(&path).await?;
                     tokio::io::copy(&mut file, &mut uni_send).await?;
                     uni_send.finish().await?;
                     let _ = event_tx.send(SessionEvent::Log("File sent successfully.".into())).await;
                } else {
                     let _ = event_tx.send(SessionEvent::Error("Transfer rejected.".into())).await;
                }
             }
             _ => {}
        }
    }
    Ok(())
}

pub async fn run_client_session(
    server_addr: SocketAddr, 
    send_file_path: Option<PathBuf>,
    mut cmd_rx: Receiver<SessionCommand>,
    event_tx: Sender<SessionEvent>
) -> Result<()> {
    let _ = event_tx.send(SessionEvent::Log(format!("Connecting to {}", server_addr))).await;

    // 1. Setup QUIC Client
    let endpoint = make_client_endpoint("0.0.0.0:0".parse()?)?;
    let connection = match endpoint.connect(server_addr, "localhost")?.await {
        Ok(c) => c,
        Err(e) => {
            let _ = event_tx.send(SessionEvent::Error(format!("Connection failed: {}", e))).await;
            return Err(e.into());
        }
    };
    
    let _ = event_tx.send(SessionEvent::Connected(connection.remote_address().to_string())).await;
    let _ = event_tx.send(SessionEvent::Log("Connected!".into())).await;

    let (mut send, mut recv) = connection.open_bi().await?;
    let _ = event_tx.send(SessionEvent::Log("Protocol Stream opened".into())).await;
    
    // 2.5 Handle Initial File Send (Legacy M4 CLI support)
    // We can just queue it? Or run it.
    if let Some(path) = send_file_path {
        // reuse perform_file_send but we need streams.
        // For MVP, since we changed protocol to use NEW stream for request (see above), 
        // we can just call it.
        // Wait, perform_file_send creates new stream.
        let mut dummy_send = connection.open_uni().await?; // Hack to satisfy type if we passed refs, but we changed sig.
        // Actually perform_file_send takes connection.
        
        // We don't need 'send'/'recv' args if we open new streams?
        // Let's update perform_file_send to NOT take send/recv streams, but open its own.
        // Correct.
        perform_file_send(&connection, path, &event_tx).await?;
        return Ok(());
    }

    // 3. Setup Channel for Mixed Events
    let (tx, mut rx) = mpsc::channel::<Frame>(100);

    // 4. Start Capture
    let source = DefaultInputSource::new();
    let tx_clone = tx.clone();
    
    let tx_clone = tx.clone();
    source.start_capture(Box::new(move |event| {
        let _ = tx_clone.blocking_send(Frame::Input(event));
    }))?;

    let _ = event_tx.send(SessionEvent::Log("Input capture started.".into())).await;
    
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
            // We aren't reading from 'recv' here? 
            // The Client is sending inputs. Does it receive anything?
            // Clipboard updates? Yes!
            // We need to read 'recv' too.
             _ = read_frame_loop(&mut recv, &event_tx) => {
                 // If this returns, stream closed or error
                 break;
             }
        }
    }
    
    source.stop_capture()?;
    let _ = event_tx.send(SessionEvent::Disconnected).await;
    Ok(())
}

async fn perform_file_send(
    connection: &quinn::Connection, 
    path: PathBuf, 
    event_tx: &Sender<SessionEvent>
) -> Result<()> {
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
        let (mut req_send, mut req_recv) = connection.open_bi().await?;
        write_frame(&mut req_send, &req).await?;
        
        // Wait for response on THIS stream
        match read_frame(&mut req_recv).await? {
             Some(Frame::FileTransferResponse(resp)) => {
                if resp.accepted {
                     let _ = event_tx.send(SessionEvent::Log("Transfer accepted. Sending data...".into())).await;
                     let mut uni_send = connection.open_uni().await?;
                     let mut file = File::open(&path).await?;
                     tokio::io::copy(&mut file, &mut uni_send).await?;
                     uni_send.finish().await?;
                     let _ = event_tx.send(SessionEvent::Log("File sent successfully.".into())).await;
                } else {
                     let _ = event_tx.send(SessionEvent::Error("Transfer rejected.".into())).await;
                }
             }
             _ => {
                 let _ = event_tx.send(SessionEvent::Error("Invalid response for file transfer.".into())).await;
             }
        }
    }
    Ok(())
}

async fn read_frame_loop(recv: &mut quinn::RecvStream, event_tx: &Sender<SessionEvent>) {
    loop {
         match read_frame(recv).await {
             Ok(Some(Frame::Clipboard(ClipboardEvent::Text(text)))) => {
                 // We don't have local clipboard handle here easily without passing it down?
                 // Or we can just set it ourselves since we are in async context?
                 // Wait, WindowsClipboard::new() is cheap unit struct.
                 let clip = DefaultClipboard::new();
                 if let Err(_) = clip.set_text(text) {}
             }
             Ok(Some(_)) => {},
             Ok(None) => return, // Closed
             Err(_) => return, // Error
         }
    }
}
