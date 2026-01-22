use anyhow::{Result, Context};
use quinn::{Endpoint, ServerConfig, TransportConfig};
use std::{net::SocketAddr, sync::Arc, time::Duration};
use crate::cert::Certificate;

pub fn make_server_endpoint(bind_addr: SocketAddr, cert: &Certificate) -> Result<Endpoint> {
    tracing::debug!("Creating server endpoint on {}", bind_addr);
    let (cert_der, priv_key_der) = (cert.cert_der.clone(), cert.priv_key_der.clone());
    let cert_chain = vec![rustls::Certificate(cert_der)];
    let priv_key = rustls::PrivateKey(priv_key_der);

    let mut crypto = rustls::ServerConfig::builder()
        .with_safe_defaults()
        .with_no_client_auth()
        .with_single_cert(cert_chain, priv_key)
        .context("Failed to create server crypto config")?;
    
    crypto.alpn_protocols = vec![b"pp/1".to_vec()];
    tracing::debug!("Server ALPN protocols set to: {:?}", crypto.alpn_protocols);

    let mut server_config = ServerConfig::with_crypto(Arc::new(crypto));
    
    // Set transport-specific parameters
    let mut transport_config = TransportConfig::default();
    transport_config.max_idle_timeout(Some(Duration::from_secs(60).try_into()?));
    transport_config.keep_alive_interval(Some(Duration::from_secs(10)));
    server_config.transport_config(Arc::new(transport_config));

    let endpoint = Endpoint::server(server_config, bind_addr)
        .context(format!("Failed to bind server to {}", bind_addr))?;
    
    tracing::info!("Server endpoint created successfully on {}", bind_addr);
    Ok(endpoint)
}
