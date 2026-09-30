//! mTLS configuration shared by the agent (server role, plus a one-shot
//! enrollment client) and the app (client role, plus a temporary enrollment
//! server).

use std::sync::Arc;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::client::WebPkiServerVerifier;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName, UnixTime};
use rustls::server::WebPkiClientVerifier;
use rustls::{ClientConfig, DigitallySignedStruct, RootCertStore, ServerConfig, SignatureScheme};

use crate::pki::fingerprint_sha256;

#[derive(Debug, thiserror::Error)]
pub enum TlsError {
    #[error("failed to read PEM material: {0}")]
    Io(#[from] std::io::Error),
    #[error("PEM content has no private key")]
    NoPrivateKey,
    #[error(transparent)]
    Rustls(#[from] rustls::Error),
    #[error("{0}")]
    Other(String),
}

/// Must be called once at process start, before any TLS config is built.
/// Safe to call more than once (later calls are ignored).
pub fn install_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

pub fn parse_cert_chain_pem(pem: &str) -> Result<Vec<CertificateDer<'static>>, TlsError> {
    let mut reader = std::io::Cursor::new(pem.as_bytes());
    rustls_pemfile::certs(&mut reader)
        .collect::<Result<Vec<_>, _>>()
        .map_err(TlsError::Io)
}

pub fn parse_private_key_pem(pem: &str) -> Result<PrivateKeyDer<'static>, TlsError> {
    let mut reader = std::io::Cursor::new(pem.as_bytes());
    rustls_pemfile::private_key(&mut reader)?.ok_or(TlsError::NoPrivateKey)
}

/// Server-side mTLS config: presents `cert_pem`/`key_pem` as its identity,
/// and requires every connecting client to present a certificate signed by
/// `ca_cert_der`. Used by the agent's API server.
pub fn server_config_mtls(
    cert_pem: &str,
    key_pem: &str,
    ca_cert_der: &CertificateDer<'static>,
) -> Result<Arc<ServerConfig>, TlsError> {
    let certs = parse_cert_chain_pem(cert_pem)?;
    let key = parse_private_key_pem(key_pem)?;

    let mut roots = RootCertStore::empty();
    roots.add(ca_cert_der.clone())?;
    let client_verifier =
        WebPkiClientVerifier::builder(Arc::new(roots))
            .build()
            .map_err(|e| TlsError::Other(e.to_string()))?;

    let config = ServerConfig::builder()
        .with_client_cert_verifier(client_verifier)
        .with_single_cert(certs, key)?;
    Ok(Arc::new(config))
}

/// Plain server-auth TLS config (no client certificate required): used by
/// the app's temporary enrollment server, which authenticates requests by
/// token instead of by client certificate (the agent doesn't have one yet
/// — that's the whole point of enrolling). `chain_pem` should contain the
/// leaf certificate *and* the CA certificate concatenated, so an agent
/// pinning by CA fingerprint (see [`client_config_pinned_ca`]) can find it
/// among the presented intermediates.
pub fn server_config_tls_no_client_auth(
    chain_pem: &str,
    key_pem: &str,
) -> Result<Arc<ServerConfig>, TlsError> {
    let certs = parse_cert_chain_pem(chain_pem)?;
    let key = parse_private_key_pem(key_pem)?;
    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)?;
    Ok(Arc::new(config))
}

/// Client-side mTLS config: presents `cert_pem`/`key_pem` as its identity,
/// and only trusts servers presenting a certificate chaining to
/// `ca_cert_der`. Used by the app to talk to already-enrolled agents.
pub fn client_config_mtls(
    cert_pem: &str,
    key_pem: &str,
    ca_cert_der: &CertificateDer<'static>,
) -> Result<Arc<ClientConfig>, TlsError> {
    let certs = parse_cert_chain_pem(cert_pem)?;
    let key = parse_private_key_pem(key_pem)?;

    let mut roots = RootCertStore::empty();
    roots.add(ca_cert_der.clone())?;

    let config = ClientConfig::builder()
        .with_root_certificates(roots)
        .with_client_auth_cert(certs, key)
        .map_err(TlsError::Rustls)?;
    Ok(Arc::new(config))
}

/// Enrollment-only client config: no client certificate yet (that's the
/// whole point of enrolling), so trust is established by pinning the CA
/// certificate the server presents against a fingerprint the admin copied
/// from the app (`--ca-sha256`), rather than by an existing trust chain.
/// Once the fingerprint matches, the rest of the chain (and hostname/IP) is
/// still fully verified via normal WebPKI validation against that pinned
/// root — this only replaces "trust anchor is pre-known" with "trust anchor
/// is confirmed by its hash on first use".
pub fn client_config_pinned_ca(expected_ca_fingerprint_sha256: &str) -> Arc<ClientConfig> {
    let verifier = Arc::new(PinnedCaVerifier {
        expected_fingerprint: normalize_fingerprint(expected_ca_fingerprint_sha256),
    });
    let config = ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth();
    Arc::new(config)
}

pub fn normalize_fingerprint(fingerprint: &str) -> String {
    fingerprint
        .chars()
        .filter(|c| c.is_ascii_hexdigit())
        .collect::<String>()
        .to_ascii_lowercase()
}

#[derive(Debug)]
struct PinnedCaVerifier {
    expected_fingerprint: String,
}

impl ServerCertVerifier for PinnedCaVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp_response: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        let ca = intermediates.first().ok_or_else(|| {
            rustls::Error::General(
                "server did not present its CA certificate alongside its own".into(),
            )
        })?;
        if normalize_fingerprint(&fingerprint_sha256(ca)) != self.expected_fingerprint {
            return Err(rustls::Error::General(
                "server's CA certificate does not match the pinned fingerprint".into(),
            ));
        }

        let mut roots = RootCertStore::empty();
        roots
            .add(ca.clone())
            .map_err(|e| rustls::Error::General(e.to_string()))?;
        let verifier = WebPkiServerVerifier::builder(Arc::new(roots))
            .build()
            .map_err(|e| rustls::Error::General(e.to_string()))?;
        verifier.verify_server_cert(end_entity, intermediates, server_name, ocsp_response, now)
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &rustls::crypto::ring::default_provider().signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &rustls::crypto::ring::default_provider().signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        rustls::crypto::ring::default_provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pki::Ca;

    #[test]
    fn builds_mtls_server_and_client_configs() {
        install_crypto_provider();
        let ca = Ca::generate().unwrap();
        let server_id = ca
            .issue_server_identity(&["pc-salon".to_string(), "127.0.0.1".to_string()])
            .unwrap();
        let client_id = ca.issue_client_identity("homeguard-app").unwrap();

        server_config_mtls(&server_id.cert_pem, &server_id.key_pem, ca.cert_der()).unwrap();
        client_config_mtls(&client_id.cert_pem, &client_id.key_pem, ca.cert_der()).unwrap();
    }

    #[test]
    fn normalizes_fingerprint_formatting() {
        assert_eq!(normalize_fingerprint("AB:CD:01"), "abcd01");
        assert_eq!(normalize_fingerprint("ab-cd-01"), "abcd01");
    }
}
