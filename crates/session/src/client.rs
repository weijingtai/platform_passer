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
    log_info!(&event_tx, "Attempting WebSocket connection to {}", server_addr);

    // 1. Setup WebSocket Client
    let ws_stream = connect_ws(server_addr).await?;
    log_info!(&event_tx, "WebSocket connection established with {}", server_addr);
    let _ = event_tx.send(SessionEvent::Connected(server_addr.to_string())).await;

    let (mut ws_sink, mut ws_stream) = ws_stream.split();

    // 2. Setup Input Sink
    let sink = Arc::new(DefaultInputSink::new());

    // 3. Perform Protocol Handshake
    log_debug!(&event_tx, "Performing application handshake...");
    let handshake = Frame::Handshake(Handshake {
        version: 1,
        client_id: "macos-client".to_string(),
        capabilities: vec!["input".to_string(), "clipboard".to_string()],
    });
    ws_sink.send(Message::Binary(bincode::serialize(&handshake)?)).await?;
    
    match ws_stream.next().await {
        Some(Ok(Message::Binary(bytes))) => {
            let resp: Frame = bincode::deserialize(&bytes)?;
            if matches!(resp, Frame::Handshake(_)) {
                log_info!(&event_tx, "Protocol handshake successful.");
            } else {
                return Err(anyhow::anyhow!("Handshake failed: Invalid frame"));
            }
        }
        _ => return Err(anyhow::anyhow!("Handshake failed: No response")),
    }
    
    // 4. Setup Channel for Outbound
    let (tx, mut rx) = mpsc::channel::<Frame>(100);

    // 5. Spawn Read Loop in Background
    let event_tx_read = event_tx.clone();
    let sink_read = sink.clone();
    let tx_read = tx.clone();
    let (err_tx, mut err_rx) = mpsc::channel::<()>(1);
    tokio::spawn(async move {
        while let Some(msg) = ws_stream.next().await {
            match msg {
                Ok(Message::Binary(bytes)) => {
                    if let Ok(frame) = bincode::deserialize::<Frame>(&bytes) {
                        match frame {
                            Frame::Input(event) => {
                                if let platform_passer_core::InputEvent::ScreenSwitch(side) = event {
                                    log_info!(&event_tx_read, "Focus switched to {:?}", side);
                                } else {
                                    let _ = sink_read.inject_event(event);
                                }
                            }
                            Frame::Clipboard(ClipboardEvent::Text(text)) => {
                                log_info!(&event_tx_read, "Clipboard sync from server.");
                                let _ = DefaultClipboard::new().set_text(text);
                            }
                            Frame::Heartbeat(hb) => {
                                let _ = tx_read.send(Frame::Heartbeat(hb)).await;
                            }
                            Frame::FileTransferResponse(resp) => {
                                if resp.accepted {
                                    log_info!(&event_tx_read, "File transfer accepted by server.");
                                } else {
                                    log_warn!(&event_tx_read, "File transfer rejected by server.");
                                }
                                // In a full implementation, we'd use this to start/stop the sender loop.
                            }
                            _ => {}
                        }
                    }
                }
                Ok(Message::Close(_)) => {
                    log_info!(&event_tx_read, "Connection closed by server.");
                    break;
                }
                Err(e) => {
                    let _ = log_error!(&event_tx_read, "Read error: {}. Terminating.", e);
                    break;
                }
                _ => {}
            }
        }
        let _ = err_tx.send(()).await;
        let _ = event_tx_read.send(SessionEvent::Disconnected).await;
    });

    // 6. Start Heartbeat Task
    let tx_hb = tx.clone();
    let hb_tx_log = event_tx.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(5)).await;
            let hb = Frame::Heartbeat(Heartbeat {
                timestamp: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs(),
            });
            if tx_hb.send(hb).await.is_err() { 
                let _ = log_debug!(&hb_tx_log, "Stopping heartbeat task.");
                break; 
            }
        }
    });

    // 7. Clipboard Listener
    let clip = DefaultClipboard::new();
    let tx_clip = tx.clone();
    clip.start_listener(Box::new(move || {
        if let Ok(text) = DefaultClipboard::new().get_text() {
            if !text.is_empty() {
                let _ = tx_clip.blocking_send(Frame::Clipboard(ClipboardEvent::Text(text)));
            }
        }
    }))?;

    // 8. Main Outbound Loop
    loop {
        tokio::select! {
            Some(cmd) = cmd_rx.recv() => {
                match cmd {
                    SessionCommand::Disconnect => {
                        log_info!(&event_tx, "UI requested disconnection.");
                        break;
                    }
                    _ => {
                        log_warn!(&event_tx, "Command {:?} not yet supported over WebSocket fully.", cmd);
                    }
                }
            }
            Some(frame) = rx.recv() => {
                let frame_type = format!("{:?}", frame);
                let bytes = bincode::serialize(&frame)?;
                if let Err(e) = ws_sink.send(Message::Binary(bytes)).await {
                    let _ = log_error!(&event_tx, "Failed to send frame {}: {}. Terminating.", frame_type, e);
                    break;
                }
                if !frame_type.contains("Heartbeat") {
                    let _ = log_debug!(&event_tx, "Successfully sent frame type: {}", frame_type);
                }
            }
            _ = err_rx.recv() => {
                let _ = log_warn!(&event_tx, "Read loop terminated. Closing session.");
                break;
            }
        }
    }
    
    Ok(())
}
