use anyhow::{Result, Context};
use std::net::SocketAddr;
use tokio::net::TcpStream;
use tokio_tungstenite::{connect_async, WebSocketStream, MaybeTlsStream};

pub async fn connect_ws(addr: SocketAddr) -> Result<WebSocketStream<MaybeTlsStream<TcpStream>>> {
    let url = format!("ws://{}", addr);
    tracing::debug!("Connecting to WebSocket at {}", url);
    
    let (ws_stream, _) = connect_async(url)
        .await
        .context(format!("Failed to connect to WebSocket at {}", addr))?;
    
    tracing::info!("Connected to WebSocket successfully at {}", addr);
    Ok(ws_stream)
}
