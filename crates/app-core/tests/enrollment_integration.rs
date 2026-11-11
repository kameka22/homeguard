//! Full loop through a real TCP + TLS connection: an agent-style client
//! (its own generated key + CSR, pinned by CA fingerprint, no cert yet)
//! enrolls against the app's temporary server, and the resulting agent
//! record + initial policy come out of the DB correctly.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use homeguard_app_core::db::{Db, TargetKind};
use homeguard_app_core::enrollment_server::{serve, EnrollmentState};
use homeguard_common::wire::{EnrollRequest, EnrollResponse};
use homeguard_common::Ca;

async fn spawn_enrollment_server(state: Arc<EnrollmentState>) -> (SocketAddr, axum_server::Handle) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener); // serve() rebinds by address; we only needed a free port.

    let handle = axum_server::Handle::new();
    let handle_for_server = handle.clone();
    tokio::spawn(async move {
        serve(addr, state, handle_for_server).await.unwrap();
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
    (addr, handle)
}

async fn enroll_via_http(
    addr: SocketAddr,
    token: &str,
    ca_fingerprint: &str,
    hostname: &str,
    existing_blocked_domains: Vec<String>,
) -> Result<EnrollResponse, reqwest::StatusCode> {
    homeguard_common::install_crypto_provider();
    let (_key_pair, csr_pem) = homeguard_common::generate_agent_identity(&[hostname.to_string()]).unwrap();
    let tls_config = homeguard_common::client_config_pinned_ca(ca_fingerprint);
    let client = reqwest::Client::builder()
        .use_preconfigured_tls((*tls_config).clone())
        .build()
        .unwrap();

    let response = client
        .post(format!("https://{addr}/v1/enroll"))
        .json(&EnrollRequest {
            token: token.to_string(),
            csr_pem,
            hostname: hostname.to_string(),
            os: "linux".to_string(),
            arch: "x86_64".to_string(),
            agent_version: "0.1.0-test".to_string(),
            existing_blocked_domains,
        })
        .send()
        .await
        .unwrap();

    if response.status().is_success() {
        Ok(response.json().await.unwrap())
    } else {
        Err(response.status())
    }
}

#[tokio::test]
async fn enrolling_creates_the_agent_and_returns_its_resolved_initial_policy() {
    let ca = Arc::new(Ca::generate().unwrap());
    let db = Arc::new(Db::open_in_memory().unwrap());

    let group_id = db.create_group("Enfants").unwrap();
    let games_list = db.create_list("Jeux", &["roblox.com".to_string()]).unwrap();
    db.create_assignment(&games_list, TargetKind::Group, &group_id, None).unwrap();

    let state = Arc::new(EnrollmentState::new(ca.clone(), db.clone(), "Europe/Paris"));
    let token = state.issue_token(jiff::SignedDuration::from_secs(1800), Some(group_id.clone()));
    let (addr, handle) = spawn_enrollment_server(state).await;

    let enrolled = enroll_via_http(addr, &token, &ca.fingerprint_sha256(), "PORTABLE-LEO", Vec::new())
        .await
        .expect("enrollment should succeed with a valid token and CA pin");

    // The new agent must already be a member of the group the token
    // carried, and its initial policy must reflect that group's
    // assignment — expanded through the alias catalog.
    let agents = db.list_agents().unwrap();
    assert_eq!(agents.len(), 1);
    assert_eq!(agents[0].id, enrolled.agent_id);
    assert_eq!(agents[0].hostname, "PORTABLE-LEO");

    let group_members = db.agent_group_ids(&enrolled.agent_id).unwrap();
    assert_eq!(group_members, vec![group_id]);

    assert_eq!(enrolled.initial_policy.rules.len(), 1);
    assert!(enrolled.initial_policy.rules[0].domains.contains(&"roblox.com".to_string()));
    assert!(enrolled.initial_policy.rules[0].domains.contains(&"www.roblox.com".to_string()));

    let history = db.recent_history(10).unwrap();
    assert!(history.iter().any(|h| h.kind == "enrolled"));

    handle.shutdown();
}

#[tokio::test]
async fn rejects_expired_or_unknown_token() {
    let ca = Arc::new(Ca::generate().unwrap());
    let db = Arc::new(Db::open_in_memory().unwrap());
    let state = Arc::new(EnrollmentState::new(ca.clone(), db.clone(), "UTC"));
    let expired_token = state.issue_token(jiff::SignedDuration::from_secs(0), None);
    let (addr, handle) = spawn_enrollment_server(state).await;

    tokio::time::sleep(Duration::from_millis(10)).await;

    let status = enroll_via_http(addr, &expired_token, &ca.fingerprint_sha256(), "PC-X", Vec::new())
        .await
        .expect_err("an expired token must be rejected");
    assert_eq!(status, reqwest::StatusCode::FORBIDDEN);

    let status = enroll_via_http(addr, "never-issued", &ca.fingerprint_sha256(), "PC-X", Vec::new())
        .await
        .expect_err("an unknown token must be rejected");
    assert_eq!(status, reqwest::StatusCode::FORBIDDEN);

    assert!(db.list_agents().unwrap().is_empty());
    handle.shutdown();
}

#[tokio::test]
async fn surfaces_domains_the_agent_found_already_blocked_at_enrollment() {
    let ca = Arc::new(Ca::generate().unwrap());
    let db = Arc::new(Db::open_in_memory().unwrap());
    let state = Arc::new(EnrollmentState::new(ca.clone(), db.clone(), "UTC"));
    let token = state.issue_token(jiff::SignedDuration::from_secs(1800), None);
    let (addr, handle) = spawn_enrollment_server(state).await;

    let existing = vec!["old-tracked-site.com".to_string(), "another-old-one.com".to_string()];
    let enrolled = enroll_via_http(addr, &token, &ca.fingerprint_sha256(), "PC-SALON", existing.clone())
        .await
        .expect("enrollment should still succeed");

    let agent = db.get_agent(&enrolled.agent_id).unwrap().unwrap();
    assert_eq!(agent.pending_discovered_domains, existing);

    let history = db.recent_history(10).unwrap();
    assert!(history.iter().any(|h| h.kind == "existing_domains_detected"));

    handle.shutdown();
}

#[tokio::test]
async fn does_not_touch_pending_discovered_domains_when_none_were_reported() {
    let ca = Arc::new(Ca::generate().unwrap());
    let db = Arc::new(Db::open_in_memory().unwrap());
    let state = Arc::new(EnrollmentState::new(ca.clone(), db.clone(), "UTC"));
    let token = state.issue_token(jiff::SignedDuration::from_secs(1800), None);
    let (addr, handle) = spawn_enrollment_server(state).await;

    let enrolled = enroll_via_http(addr, &token, &ca.fingerprint_sha256(), "PC-SALON", Vec::new())
        .await
        .expect("enrollment should succeed");

    let agent = db.get_agent(&enrolled.agent_id).unwrap().unwrap();
    assert!(agent.pending_discovered_domains.is_empty());

    let history = db.recent_history(10).unwrap();
    assert!(!history.iter().any(|h| h.kind == "existing_domains_detected"));

    handle.shutdown();
}
