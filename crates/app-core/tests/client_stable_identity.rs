//! Proves the fix for certificates being pinned to an IP at enrollment
//! time: an agent's certificate SAN only ever contains its stable
//! hostname (`{hostname}.local`), never an IP — so `AgentClient` must be
//! able to reach it over an address that appears nowhere in that
//! certificate, exactly as it would after a DHCP lease change.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use homeguard_app_core::client::connect_to_agent;
use homeguard_app_core::db::Db;
use homeguard_common::wire::AgentInfo;
use homeguard_common::Ca;

async fn info_handler(State(info): State<Arc<AgentInfo>>) -> Json<AgentInfo> {
    Json((*info).clone())
}

async fn spawn_agent_double(ca: &Ca, hostname: &str) -> SocketAddr {
    // Mirrors what `enrollment_server::enroll_handler` actually issues:
    // SAN = "{hostname}.local" only, no IP anywhere.
    let server_identity = ca.issue_server_identity(&[format!("{hostname}.local")]).unwrap();
    let tls_config =
        homeguard_common::server_config_mtls(&server_identity.cert_pem, &server_identity.key_pem, ca.cert_der())
            .unwrap();
    let rustls_config = axum_server::tls_rustls::RustlsConfig::from_config(tls_config);

    let info = Arc::new(AgentInfo {
        agent_id: "a1".to_string(),
        hostname: hostname.to_string(),
        os: "linux".to_string(),
        arch: "x86_64".to_string(),
        agent_version: "0.1.0-test".to_string(),
        uptime_secs: 0,
    });
    let router = Router::new().route("/v1/info", get(info_handler)).with_state(info);

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
async fn agent_client_reaches_the_agent_by_stable_hostname_not_by_the_ip_in_its_cert() {
    homeguard_common::install_crypto_provider();
    let ca = Arc::new(Ca::generate().unwrap());
    let addr = spawn_agent_double(&ca, "portable-leo").await;

    let db = Db::open_in_memory().unwrap();
    db.upsert_agent("a1", "portable-leo", "windows", "x86_64", None, None).unwrap();

    let app_identity = ca.issue_client_identity("homeguard-app-test").unwrap();
    let client = connect_to_agent(&db, "a1", addr, &app_identity.cert_pem, &app_identity.key_pem, ca.cert_der())
        .expect("a non-revoked agent must be connectable");

    // The certificate's only SAN entry is "portable-leo.local" — nothing
    // about `addr`'s actual IP appears in it. If verification were still
    // (wrongly) tied to that IP, or `resolve()` weren't wired correctly,
    // this request would fail the TLS handshake instead of returning info.
    let info = client.info().await.expect("TLS must verify by hostname, not by IP");
    assert_eq!(info.hostname, "portable-leo");
}

#[tokio::test]
async fn connect_to_agent_still_refuses_a_revoked_agent_with_the_new_signature() {
    homeguard_common::install_crypto_provider();
    let ca = Arc::new(Ca::generate().unwrap());
    let db = Db::open_in_memory().unwrap();
    db.upsert_agent("a1", "portable-leo", "windows", "x86_64", None, None).unwrap();
    db.revoke_agent("a1").unwrap();

    let app_identity = ca.issue_client_identity("homeguard-app-test").unwrap();
    let result = connect_to_agent(
        &db,
        "a1",
        "127.0.0.1:1".parse().unwrap(),
        &app_identity.cert_pem,
        &app_identity.key_pem,
        ca.cert_der(),
    );
    assert!(result.is_err());
}
