//! Proves the agent's `enroll` command against a stand-in enrollment
//! server: token check, CA-fingerprint pinning (no client cert yet), CSR
//! signing, and that the resulting identity is then immediately usable to
//! reach the agent's own mTLS API.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use homeguard_common::wire::{EnrollRequest, EnrollResponse};
use homeguard_common::{Ca, Policy, Protection};

const EXPECTED_TOKEN: &str = "K7Q2-9XPA";

async fn enroll_handler(State(ca): State<Arc<Ca>>, Json(req): Json<EnrollRequest>) -> Response {
    if req.token != EXPECTED_TOKEN {
        return StatusCode::FORBIDDEN.into_response();
    }
    let issued = ca
        .sign_agent_csr(&req.csr_pem, &[req.hostname.clone(), "127.0.0.1".to_string()])
        .unwrap();
    Json(EnrollResponse {
        cert_pem: issued.cert_pem,
        ca_cert_pem: ca.cert_pem().to_string(),
        agent_id: "enrolled-agent-id".to_string(),
        initial_policy: Policy {
            version: 1,
            timezone: "UTC".into(),
            rules: vec![],
            exceptions: vec![],
            protection: Protection::default(),
        },
    })
    .into_response()
}

/// Same as `enroll_handler`, except it lies about which CA issued the
/// certificate: the TLS handshake still presents (and gets pinned to) the
/// real CA, but the JSON body carries a *different* CA's certificate.
async fn enroll_handler_lying_about_ca(State(ca): State<Arc<Ca>>, Json(req): Json<EnrollRequest>) -> Response {
    if req.token != EXPECTED_TOKEN {
        return StatusCode::FORBIDDEN.into_response();
    }
    let issued = ca
        .sign_agent_csr(&req.csr_pem, &[req.hostname.clone(), "127.0.0.1".to_string()])
        .unwrap();
    let rogue_ca = Ca::generate().unwrap();
    Json(EnrollResponse {
        cert_pem: issued.cert_pem,
        ca_cert_pem: rogue_ca.cert_pem().to_string(),
        agent_id: "enrolled-agent-id".to_string(),
        initial_policy: Policy {
            version: 1,
            timezone: "UTC".into(),
            rules: vec![],
            exceptions: vec![],
            protection: Protection::default(),
        },
    })
    .into_response()
}

async fn spawn_enrollment_server(ca: Arc<Ca>) -> SocketAddr {
    let server_identity = ca
        .issue_server_identity(&["127.0.0.1".to_string()])
        .unwrap();
    // Present [leaf, ca] so a pinning client can find the CA in the chain.
    let chain_pem = format!("{}\n{}", server_identity.cert_pem, ca.cert_pem());
    let tls_config =
        homeguard_common::server_config_tls_no_client_auth(&chain_pem, &server_identity.key_pem).unwrap();
    let rustls_config = axum_server::tls_rustls::RustlsConfig::from_config(tls_config);

    let router = Router::new()
        .route("/v1/enroll", post(enroll_handler))
        .route("/v1/enroll-lying", post(enroll_handler_lying_about_ca))
        .with_state(ca);

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum_server::from_tcp_rustls(listener, rustls_config)
            .serve(router.into_make_service())
            .await
            .unwrap();
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    addr
}

#[tokio::test]
async fn enroll_rejects_wrong_token() {
    homeguard_common::install_crypto_provider();
    let ca = Arc::new(Ca::generate().unwrap());
    let addr = spawn_enrollment_server(ca.clone()).await;

    let data_dir = tempfile::tempdir().unwrap();
    let paths = homeguard_agent::config::AgentPaths::new(data_dir.path());
    paths.ensure_created().unwrap();

    let result = homeguard_agent::enroll::enroll(
        &paths,
        &format!("https://{addr}/v1/enroll"),
        "wrong-token",
        &ca.fingerprint_sha256(),
    )
    .await;

    assert!(matches!(
        result,
        Err(homeguard_agent::enroll::EnrollError::Rejected { .. })
    ));
}

#[tokio::test]
async fn enroll_rejects_wrong_ca_pin() {
    homeguard_common::install_crypto_provider();
    let ca = Arc::new(Ca::generate().unwrap());
    let addr = spawn_enrollment_server(ca.clone()).await;

    let wrong_ca = Ca::generate().unwrap();
    let data_dir = tempfile::tempdir().unwrap();
    let paths = homeguard_agent::config::AgentPaths::new(data_dir.path());
    paths.ensure_created().unwrap();

    let result = homeguard_agent::enroll::enroll(
        &paths,
        &format!("https://{addr}/v1/enroll"),
        EXPECTED_TOKEN,
        &wrong_ca.fingerprint_sha256(),
    )
    .await;

    assert!(result.is_err(), "expected the pinned CA mismatch to fail the connection");
}

#[tokio::test]
async fn enroll_rejects_a_ca_cert_in_the_body_that_does_not_match_the_pin() {
    homeguard_common::install_crypto_provider();
    let ca = Arc::new(Ca::generate().unwrap());
    let addr = spawn_enrollment_server(ca.clone()).await;

    let data_dir = tempfile::tempdir().unwrap();
    let paths = homeguard_agent::config::AgentPaths::new(data_dir.path());
    paths.ensure_created().unwrap();

    // The TLS handshake pins correctly (it's the real CA), but the
    // response body's ca_cert_pem is a different CA entirely.
    let result = homeguard_agent::enroll::enroll(
        &paths,
        &format!("https://{addr}/v1/enroll-lying"),
        EXPECTED_TOKEN,
        &ca.fingerprint_sha256(),
    )
    .await;

    assert!(matches!(
        result,
        Err(homeguard_agent::enroll::EnrollError::CaFingerprintMismatch { .. })
    ));
    assert!(!paths.cert_path().exists(), "nothing should be persisted on a fingerprint mismatch");
}

#[tokio::test]
async fn enroll_succeeds_and_persists_a_usable_identity() {
    homeguard_common::install_crypto_provider();
    let ca = Arc::new(Ca::generate().unwrap());
    let addr = spawn_enrollment_server(ca.clone()).await;

    let data_dir = tempfile::tempdir().unwrap();
    let paths = homeguard_agent::config::AgentPaths::new(data_dir.path());
    paths.ensure_created().unwrap();

    let enrolled = homeguard_agent::enroll::enroll(
        &paths,
        &format!("https://{addr}/v1/enroll"),
        EXPECTED_TOKEN,
        &ca.fingerprint_sha256(),
    )
    .await
    .unwrap();

    assert_eq!(enrolled.agent_id, "enrolled-agent-id");
    assert!(paths.cert_path().exists());
    assert!(paths.key_path().exists());
    assert!(paths.ca_cert_path().exists());

    let identity = homeguard_agent::identity::AgentIdentity::load(&paths).unwrap();
    assert_eq!(identity.agent_id, "enrolled-agent-id");
    // The identity must build a valid server TLS config (i.e. the issued
    // cert + locally-kept key are actually a matching pair).
    identity.server_tls_config().unwrap();
}
