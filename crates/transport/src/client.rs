use anyhow::{Result, Context};
use quinn::{Endpoint, ClientConfig, TransportConfig};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

struct SkipServerVerification;

impl rustls::client::ServerCertVerifier for SkipServerVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::Certificate,
        _intermediates: &[rustls::Certificate],
        _server_name: &rustls::ServerName,
        _scts: &mut dyn Iterator<Item = &[u8]>,
        _ocsp_response: &[u8],
        _now: std::time::SystemTime,
    ) -> Result<rustls::client::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::ServerCertVerified::assertion())
    }
}

pub fn make_client_endpoint(bind_addr: SocketAddr) -> Result<Endpoint> {
    tracing::debug!("Creating client endpoint on {}", bind_addr);
    let client_cfg = configure_client();
    let mut endpoint = Endpoint::client(bind_addr)
        .context(format!("Failed to create client endpoint on {}", bind_addr))?;
    endpoint.set_default_client_config(client_cfg);
    tracing::info!("Client endpoint created successfully on {}", bind_addr);
    Ok(endpoint)
}

fn configure_client() -> ClientConfig {
    let mut crypto = rustls::ClientConfig::builder()
        .with_safe_defaults()
        .with_custom_certificate_verifier(Arc::new(SkipServerVerification))
        .with_no_client_auth();
    
    crypto.alpn_protocols = vec![b"pp/1".to_vec()];
    tracing::debug!("Client ALPN protocols set to: {:?}", crypto.alpn_protocols);
    
    let mut client_config = ClientConfig::new(Arc::new(crypto));

    // Set transport-specific parameters
    let mut transport_config = TransportConfig::default();
    transport_config.max_idle_timeout(Some(Duration::from_secs(300).try_into().unwrap()));
    transport_config.keep_alive_interval(Some(Duration::from_secs(5)));
    client_config.transport_config(Arc::new(transport_config));
    
    client_config
}
