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
                    client_id: "macos-server".to_string(),
                    capabilities: vec!["input".to_string(), "clipboard".to_string()],
                });
                ws_sink.send(Message::Binary(bincode::serialize(&resp)?)).await?;
            }
            _ => {
                log_error!(&event_tx, "Invalid handshake frame");
                return Err(anyhow::anyhow!("Invalid handshake"));
            }
        }
    }

    log_debug!(&event_tx, "Entering protocol loop...");
    loop {
        tokio::select! {
            // Read from client
            msg = ws_stream.next() => {
                match msg {
                    Some(Ok(Message::Binary(bytes))) => {
                        let frame: Frame = bincode::deserialize(&bytes)?;
                        match frame {
                            Frame::Clipboard(ClipboardEvent::Text(text)) => {
                                log_debug!(&event_tx, "Received clipboard update ({} chars)", text.len());
                                let _ = clip.set_text(text);
                            }
                            Frame::Heartbeat(hb) => {
                                // Echo back
                                ws_sink.send(Message::Binary(bincode::serialize(&Frame::Heartbeat(hb))?)).await?;
                            }
                            _ => {}
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
                        log_warn!(&event_tx, "Input broadcast LAGGED by {} messages.", n);
                        continue;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    }

    log_info!(&event_tx, "Session terminated. Resetting focus.");
    DefaultInputSource::set_remote(false);
    let _ = event_tx.send(SessionEvent::Disconnected).await;
    Ok(())
}
