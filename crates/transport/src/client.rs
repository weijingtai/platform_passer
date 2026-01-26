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

    // Set TCP_NODELAY on the underlying TCP stream
    let res = match ws_stream.get_ref() {
        MaybeTlsStream::Plain(s) => s.set_nodelay(true),
        _ => {
            // Potentially handle other TLS streams if they are enabled via features
            // and use nested get_ref() to reach the underlying TcpStream.
            Ok(())
        }
    };
    
    if let Err(e) = res {
        tracing::warn!("Failed to set TCP_NODELAY: {}", e);
    }
    
    tracing::info!("Connected to WebSocket successfully at {}", addr);
    Ok(ws_stream)
}
