use anyhow::{Result, Context};
use quinn::{Endpoint, ServerConfig};
use std::{net::SocketAddr, sync::Arc};
use crate::cert::Certificate;

pub fn make_server_endpoint(bind_addr: SocketAddr, cert: &Certificate) -> Result<Endpoint> {
    let (cert_der, priv_key_der) = (cert.cert_der.clone(), cert.priv_key_der.clone());
    let cert_chain = vec![rustls::Certificate(cert_der)];
    let priv_key = rustls::PrivateKey(priv_key_der);

    let server_config = ServerConfig::with_single_cert(cert_chain, priv_key)
        .context("Failed to create server config")?;
    
    let endpoint = Endpoint::server(server_config, bind_addr)?;
    Ok(endpoint)
}
