use anyhow::{Result, Context};
use quinn::{Endpoint, ClientConfig};
use std::net::SocketAddr;
use std::sync::Arc;

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
    
    ClientConfig::new(Arc::new(crypto))
}
