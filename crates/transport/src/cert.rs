use anyhow::{Context, Result};
use rcgen::generate_simple_self_signed;

pub struct Certificate {
    pub cert_der: Vec<u8>,
    pub priv_key_der: Vec<u8>,
}

pub fn generate_self_signed_cert(subject_alt_names: Vec<String>) -> Result<Certificate> {
    let cert = generate_simple_self_signed(subject_alt_names).context("Failed to generate self-signed cert")?;
    Ok(Certificate {
        cert_der: cert.serialize_der()?,
        priv_key_der: cert.serialize_private_key_der(),
    })
}
