//! End-to-end proof that the whole loop works over a real TCP + mTLS
//! connection: app-style client -> agent's axum-server -> policy applied
//! to a real hosts file on disk, plus the mTLS enforcement itself (a
//! client without a certificate signed by the right CA must be rejected).

use std::net::SocketAddr;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use homeguard_agent::api;
use homeguard_agent::events::EventLog;
use homeguard_agent::state::{run_scheduler, AppState};
use homeguard_common::policy::{Policy, Protection, Rule};
use homeguard_common::wire::{AgentInfo, AgentStatus, EventsPage};
use homeguard_common::Ca;

struct TestAgent {
    addr: SocketAddr,
    hosts_path: std::path::PathBuf,
    _dir: tempfile::TempDir,
}

async fn spawn_test_agent(ca: &Ca) -> TestAgent {
    let dir = tempfile::tempdir().unwrap();
    let hosts_path = dir.path().join("hosts");
    std::fs::write(&hosts_path, "127.0.0.1 localhost\n").unwrap();

    let server_identity = ca
        .issue_server_identity(&["127.0.0.1".to_string()])
        .unwrap();
    let tls_config =
        homeguard_common::server_config_mtls(&server_identity.cert_pem, &server_identity.key_pem, ca.cert_der())
            .unwrap();

    let events = Arc::new(EventLog::open(dir.path().join("events.jsonl")).unwrap());
    let state = Arc::new(AppState {
        agent_id: "test-agent-id".into(),
        hostname: "test-host".into(),
        os: "linux".into(),
        arch: "x86_64".into(),
        agent_version: "0.1.0-test".into(),
        started_at: Instant::now(),
        hosts_path: hosts_path.clone(),
        policy_path: dir.path().join("policy.json"),
        etc_root: dir.path().join("etc"),
        events,
        policy: RwLock::new(Policy {
            version: 0,
            timezone: "UTC".into(),
            rules: vec![],
            exceptions: vec![],
            protection: Protection::default(),
        }),
        last_applied: RwLock::new(Default::default()),
        policy_changed: tokio::sync::Notify::new(),
    });

    tokio::spawn(run_scheduler(state.clone()));

    let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
    let listener = std::net::TcpListener::bind(addr).unwrap();
    let bound_addr = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();

    let rustls_config = axum_server::tls_rustls::RustlsConfig::from_config(tls_config);
    let router = api::router(state);
    tokio::spawn(async move {
        axum_server::from_tcp_rustls(listener, rustls_config)
            .serve(router.into_make_service())
            .await
            .unwrap();
    });

    // Give the server a moment to start accepting.
    tokio::time::sleep(Duration::from_millis(50)).await;

    TestAgent {
        addr: bound_addr,
        hosts_path,
        _dir: dir,
    }
}

fn mtls_client(ca: &Ca) -> reqwest::Client {
    homeguard_common::install_crypto_provider();
    let client_identity = ca.issue_client_identity("homeguard-app-test").unwrap();
    let tls_config =
        homeguard_common::client_config_mtls(&client_identity.cert_pem, &client_identity.key_pem, ca.cert_der())
            .unwrap();
    reqwest::Client::builder()
        .use_preconfigured_tls((*tls_config).clone())
        .build()
        .unwrap()
}

#[tokio::test]
async fn app_client_can_read_info_push_policy_and_see_hosts_updated() {
    let ca = Ca::generate().unwrap();
    let agent = spawn_test_agent(&ca).await;
    let client = mtls_client(&ca);
    let base = format!("https://{}", agent.addr);

    let info: AgentInfo = client
        .get(format!("{base}/v1/info"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(info.agent_id, "test-agent-id");

    let policy = Policy {
        version: 1,
        timezone: "UTC".into(),
        rules: vec![Rule {
            id: "social".into(),
            name: "Réseaux sociaux".into(),
            domains: vec!["tiktok.com".into(), "youtube.com".into()],
            schedule: None,
        }],
        exceptions: vec![],
        protection: Protection {
            self_heal: true,
            disable_doh: false,
        },
    };
    let put_status = client
        .put(format!("{base}/v1/policy"))
        .json(&policy)
        .send()
        .await
        .unwrap()
        .status();
    assert_eq!(put_status, 204);

    // The scheduler reacts to the Notify immediately; give it a beat.
    tokio::time::sleep(Duration::from_millis(100)).await;

    let hosts_content = std::fs::read_to_string(&agent.hosts_path).unwrap();
    assert!(hosts_content.contains("0.0.0.0 tiktok.com"));
    assert!(hosts_content.contains("0.0.0.0 youtube.com"));
    assert!(hosts_content.contains("127.0.0.1 localhost"));

    let status: AgentStatus = client
        .get(format!("{base}/v1/status"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(status.applied_policy_version, 1);
    assert_eq!(status.domains_blocked_now, 2);
    assert!(status.integrity_ok);
    assert!(status.self_heal_enabled);

    let events: EventsPage = client
        .get(format!("{base}/v1/events?since=1"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(events.events.iter().any(|e| e.message.contains("politique v1")));
}

#[tokio::test]
async fn hosts_block_hash_reflects_the_actual_file_not_the_policy() {
    let ca = Ca::generate().unwrap();
    let agent = spawn_test_agent(&ca).await;
    let client = mtls_client(&ca);
    let base = format!("https://{}", agent.addr);

    let policy = Policy {
        version: 1,
        timezone: "UTC".into(),
        rules: vec![Rule {
            id: "social".into(),
            name: "Réseaux sociaux".into(),
            domains: vec!["tiktok.com".into()],
            schedule: None,
        }],
        exceptions: vec![],
        // Self-heal off: a manual edit below must stick around long
        // enough to observe.
        protection: Protection {
            self_heal: false,
            disable_doh: false,
        },
    };
    client.put(format!("{base}/v1/policy")).json(&policy).send().await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    let status_before: AgentStatus = client.get(format!("{base}/v1/status")).send().await.unwrap().json().await.unwrap();

    // Tamper with the file directly, bypassing the agent entirely.
    std::fs::write(
        &agent.hosts_path,
        "127.0.0.1 localhost\n\n# BEGIN HOMEGUARD\n0.0.0.0 something-else.com\n# END HOMEGUARD\n",
    )
    .unwrap();

    let status_after: AgentStatus = client.get(format!("{base}/v1/status")).send().await.unwrap().json().await.unwrap();

    // The policy (and therefore domains_blocked_now) hasn't changed, but
    // the hash must follow the real file content, not the policy.
    assert_eq!(status_after.applied_policy_version, status_before.applied_policy_version);
    assert_eq!(status_after.domains_blocked_now, status_before.domains_blocked_now);
    assert_ne!(status_after.hosts_block_hash, status_before.hosts_block_hash);
    assert!(!status_after.integrity_ok);
}

#[tokio::test]
async fn put_policy_conflicts_on_stale_if_match() {
    let ca = Ca::generate().unwrap();
    let agent = spawn_test_agent(&ca).await;
    let client = mtls_client(&ca);
    let base = format!("https://{}", agent.addr);

    let status = client
        .put(format!("{base}/v1/policy"))
        .header("if-match", "999")
        .json(&Policy {
            version: 1,
            timezone: "UTC".into(),
            rules: vec![],
            exceptions: vec![],
            protection: Protection::default(),
        })
        .send()
        .await
        .unwrap()
        .status();

    assert_eq!(status, 409);
}

#[tokio::test]
async fn client_without_a_cert_signed_by_the_right_ca_is_rejected() {
    let ca = Ca::generate().unwrap();
    let agent = spawn_test_agent(&ca).await;

    // A client trusting (and presenting a cert from) a *different* CA must
    // be rejected by the agent's mTLS handshake.
    let rogue_ca = Ca::generate().unwrap();
    let rogue_client = mtls_client(&rogue_ca);

    let result = rogue_client
        .get(format!("https://{}/v1/info", agent.addr))
        .send()
        .await;

    assert!(result.is_err(), "expected the TLS handshake to fail, got {result:?}");
}
