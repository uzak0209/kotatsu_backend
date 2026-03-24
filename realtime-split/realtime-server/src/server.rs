use anyhow::Result;
use quinn::{Endpoint, ServerConfig};
use rcgen::generate_simple_self_signed;
use rustls_pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use std::net::SocketAddr;

pub(crate) fn build_quic_server(addr: SocketAddr) -> Result<Endpoint> {
    let cert = generate_simple_self_signed(vec!["localhost".into(), "127.0.0.1".into()])?;
    let cert_der = cert.cert.der().to_vec();
    let key_der = cert.key_pair.serialize_der();

    let certs = vec![CertificateDer::from(cert_der)];
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key_der));
    let server_config = ServerConfig::with_single_cert(certs, key)?;

    Ok(Endpoint::server(server_config, addr)?)
}
