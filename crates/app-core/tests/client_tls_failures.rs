//! Proves `ClientError::tls_failure` recognizes real rustls rejections
//! (as produced by an actual handshake against an actual server) — and
//! does *not* mistake a plain network failure for one.

use std::net::SocketAddr;

use axum::routing::get;
use axum::Router;
use homeguard_app_core::client::{AgentClient, ClientError, TlsFailure};
use homeguard_common::Ca;

/// An mTLS server presenting a certificate from `server_ca` and accepting
/// client certificates from `client_ca` — what an agent enrolled by
/// another app (or another database) looks like from here.
async fn spawn_tls_server(server_ca: &Ca, client_ca: &Ca) -> SocketAddr {
    let identity = server_ca
        .issue_server_identity(&["poste.local".to_string()])
        .unwrap();
    let config = homeguard_common::server_config_mtls(
        &identity.cert_pem,
        &identity.key_pem,
        client_ca.cert_der(),
    )
    .unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let router = Router::new().route("/v1/info", get(|| async { "{}" }));
    tokio::spawn(async move {
        axum_server::from_tcp_rustls(
            listener,
            axum_server::tls_rustls::RustlsConfig::from_config(config),
        )
        .serve(router.into_make_service())
        .await
        .unwrap();
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    addr
}

async fn info_error(app_ca: &Ca, addr: SocketAddr) -> ClientError {
    homeguard_common::install_crypto_provider();
    let identity = app_ca.issue_client_identity("homeguard-app-test").unwrap();
    // A poste name containing every word the old keyword match looked
    // for: the classification must not depend on the URL.
    let client = AgentClient::new(
        "tls-alert-certificate-handshake",
        addr,
        &identity.cert_pem,
        &identity.key_pem,
        app_ca.cert_der(),
    )
    .unwrap();
    client.info().await.unwrap_err()
}

#[tokio::test]
async fn agent_certificate_from_another_ca_is_a_tls_rejection() {
    let app_ca = Ca::generate().unwrap();
    let other_ca = Ca::generate().unwrap();
    let addr = spawn_tls_server(&other_ca, &other_ca).await;
    assert_eq!(
        info_error(&app_ca, addr).await.tls_failure(),
        Some(TlsFailure::Rejected)
    );
}

#[tokio::test]
async fn agent_refusing_our_client_certificate_is_a_tls_rejection() {
    let app_ca = Ca::generate().unwrap();
    let other_ca = Ca::generate().unwrap();
    // Server certificate is fine, but the agent only trusts clients from
    // another CA: it answers with a fatal alert after the handshake.
    let addr = spawn_tls_server(&app_ca, &other_ca).await;
    assert_eq!(
        info_error(&app_ca, addr).await.tls_failure(),
        Some(TlsFailure::Rejected)
    );
}

#[tokio::test]
async fn a_connection_cut_during_the_handshake_is_not_a_tls_rejection() {
    let app_ca = Ca::generate().unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((socket, _)) = listener.accept().await else {
                return;
            };
            drop(socket);
        }
    });
    let e = info_error(&app_ca, addr).await;
    assert_eq!(e.tls_failure(), None);
    assert!(matches!(&e, ClientError::Request(re) if re.is_connect()));
}

#[tokio::test]
async fn a_refused_connection_is_not_a_tls_rejection() {
    let app_ca = Ca::generate().unwrap();
    // Port 9 on localhost: nothing listens there in practice.
    let e = info_error(&app_ca, "127.0.0.1:9".parse().unwrap()).await;
    assert_eq!(e.tls_failure(), None);
}

#[tokio::test]
async fn a_plain_http_server_is_reported_as_not_speaking_tls() {
    let app_ca = Ca::generate().unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum_server::from_tcp(listener)
            .serve(
                Router::new()
                    .route("/v1/info", get(|| async { "{}" }))
                    .into_make_service(),
            )
            .await
            .unwrap();
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert_eq!(
        info_error(&app_ca, addr).await.tls_failure(),
        Some(TlsFailure::NotTls)
    );
}
