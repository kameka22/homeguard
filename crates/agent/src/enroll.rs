use homeguard_common::wire::{EnrollRequest, EnrollResponse};

use crate::config::AgentPaths;
use crate::identity::AgentIdentity;

#[derive(Debug, thiserror::Error)]
pub enum EnrollError {
    #[error(transparent)]
    Pki(#[from] homeguard_common::PkiError),
    #[error("request to the enrollment server failed: {0}")]
    Request(#[from] reqwest::Error),
    #[error("enrollment server rejected the request: {status} {body}")]
    Rejected {
        status: reqwest::StatusCode,
        body: String,
    },
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// The CA certificate returned in the JSON response body doesn't match
    /// the fingerprint that was pinned during the TLS handshake — the
    /// transport was authenticated, but the payload it carried wasn't.
    #[error("l'application a renvoyé un certificat de CA différent de celui épinglé (attendu {expected}, reçu {actual})")]
    CaFingerprintMismatch { expected: String, actual: String },
}

pub fn local_hostname() -> String {
    gethostname::gethostname()
        .into_string()
        .unwrap_or_else(|_| "unknown-host".to_string())
}

/// Generate this agent's own key pair and request a signed certificate
/// from the app's temporary enrollment server, pinning it by the CA
/// fingerprint the admin copied from the app. On success, persists the
/// identity and the initial policy under `paths` — after this, `run` can
/// start normally.
pub async fn enroll(
    paths: &AgentPaths,
    enroll_url: &str,
    token: &str,
    ca_fingerprint_sha256: &str,
) -> Result<EnrollResponse, EnrollError> {
    let hostname = local_hostname();
    let (key_pair, csr_pem) =
        homeguard_common::generate_agent_identity(std::slice::from_ref(&hostname))?;

    homeguard_common::install_crypto_provider();
    let tls_config = homeguard_common::client_config_pinned_ca(ca_fingerprint_sha256);
    let client = reqwest::Client::builder()
        .use_preconfigured_tls((*tls_config).clone())
        .build()?;

    let request = EnrollRequest {
        token: token.to_string(),
        csr_pem,
        hostname: hostname.clone(),
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        agent_version: env!("CARGO_PKG_VERSION").to_string(),
    };

    let response = client.post(enroll_url).json(&request).send().await?;
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(EnrollError::Rejected { status, body });
    }
    let enrolled: EnrollResponse = response.json().await?;

    // The pinned-CA verifier only checked the certificate presented at the
    // TLS layer. `ca_cert_pem` is separate, application-layer data — it
    // must be checked against the same pin before we trust it forever.
    let actual = homeguard_common::fingerprint_of_pem(&enrolled.ca_cert_pem)?;
    if homeguard_common::tls::normalize_fingerprint(&actual)
        != homeguard_common::tls::normalize_fingerprint(ca_fingerprint_sha256)
    {
        return Err(EnrollError::CaFingerprintMismatch {
            expected: ca_fingerprint_sha256.to_string(),
            actual,
        });
    }

    AgentIdentity::save(
        paths,
        &enrolled.cert_pem,
        &key_pair.serialize_pem(),
        &enrolled.ca_cert_pem,
        &enrolled.agent_id,
    )?;
    let policy_json = serde_json::to_string_pretty(&enrolled.initial_policy)
        .expect("Policy always serializes");
    std::fs::write(paths.policy_path(), policy_json)?;

    Ok(enrolled)
}
