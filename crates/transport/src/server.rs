use anyhow::{Result, Context};
use std::net::SocketAddr;
use tokio::net::TcpListener;

pub async fn make_ws_listener(bind_addr: SocketAddr) -> Result<TcpListener> {
    tracing::debug!("Creating WebSocket TCP listener on {}", bind_addr);
    let listener = TcpListener::bind(bind_addr)
        .await
        .context(format!("Failed to bind TCP listener to {}", bind_addr))?;
    
    tracing::info!("WebSocket server listener created successfully on {}", bind_addr);
    Ok(listener)
}
