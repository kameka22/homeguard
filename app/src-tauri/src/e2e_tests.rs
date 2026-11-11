//! End-to-end checks of the push path against a *real* agent binary:
//! enrollment through the app's own enrollment server, automatic-push
//! semantics (dirty flag), temporary unblocks, error classification and
//! the "no false manual-stop alert across a re-install" guarantee.
//!
//! Ignored by default: they spawn processes and bind 127.0.0.1:7878 (the
//! fixed agent port `sync_known_agents` dials). Run with:
//!
//! ```sh
//! cargo build --release -p homeguard-agent
//! HOMEGUARD_AGENT_BIN=$PWD/target/release/homeguard-agent \
//!   cargo test --manifest-path app/src-tauri/Cargo.toml e2e -- --ignored --test-threads=1
//! ```

use std::collections::HashSet;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::Arc;
use std::time::{Duration, Instant};

use homeguard_app_core::db::TargetKind;
use homeguard_app_core::enrollment_server::{self, EnrollmentState};

use crate::state::AppState;
use crate::sync::{self, SyncErrorKind, AGENT_PORT};

const BLOCKED: &str = "e2e-blocked.example";

struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "homeguard-e2e-{tag}-{}-{}",
            std::process::id(),
            jiff::Timestamp::now().as_nanosecond()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Agent {
    bin: PathBuf,
    data_dir: PathBuf,
    hosts: PathBuf,
    child: Option<Child>,
}

impl Agent {
    fn new(bin: &Path, root: &Path) -> Self {
        let hosts = root.join("hosts");
        std::fs::write(&hosts, "127.0.0.1 localhost\n").unwrap();
        Agent {
            bin: bin.to_path_buf(),
            data_dir: root.join("agent"),
            hosts,
            child: None,
        }
    }

    async fn enroll(&self, state: &AppState, enroll_port: u16) -> String {
        let enrollment = Arc::new(EnrollmentState::new(
            state.ca.clone(),
            state.db.clone(),
            state.timezone(),
        ));
        let token = enrollment.issue_token(jiff::SignedDuration::from_secs(300), None);
        let handle = axum_server::Handle::new();
        let addr: SocketAddr = ([127, 0, 0, 1], enroll_port).into();
        let server = tokio::spawn(enrollment_server::serve(addr, enrollment, handle.clone()));
        tokio::time::sleep(Duration::from_millis(300)).await;

        let before: HashSet<String> = state
            .db
            .list_agents()
            .unwrap()
            .into_iter()
            .map(|a| a.id)
            .collect();
        let bin = self.bin.clone();
        let args = [
            "enroll".to_string(),
            "--url".into(),
            format!("https://127.0.0.1:{enroll_port}/v1/enroll"),
            "--token".into(),
            token,
            "--ca-sha256".into(),
            state.ca.fingerprint_sha256(),
            "--data-dir".into(),
            self.data_dir.display().to_string(),
            "--hosts-path".into(),
            self.hosts.display().to_string(),
        ];
        let status = tokio::task::spawn_blocking(move || Command::new(bin).args(args).status())
            .await
            .unwrap()
            .unwrap();
        assert!(status.success(), "enroll failed");
        handle.shutdown();
        let _ = server.await;

        state
            .db
            .list_agents()
            .unwrap()
            .into_iter()
            .map(|a| a.id)
            .find(|id| !before.contains(id))
            .expect("enrollment should have registered a new agent")
    }

    async fn start(&mut self) {
        let child = Command::new(&self.bin)
            .args([
                "run",
                "--port",
                &AGENT_PORT.to_string(),
                "--no-mdns",
                "--data-dir",
            ])
            .arg(&self.data_dir)
            .arg("--hosts-path")
            .arg(&self.hosts)
            .spawn()
            .unwrap();
        self.child = Some(child);
        for _ in 0..50 {
            if tokio::net::TcpStream::connect(("127.0.0.1", AGENT_PORT))
                .await
                .is_ok()
            {
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        panic!("agent did not start listening");
    }

    fn kill(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    /// What an installer does: announce the restart, then SIGTERM — which
    /// the agent otherwise reads as a manual stop.
    fn stop_like_an_installer(&mut self) {
        homeguard_agent_marker(&self.data_dir);
        if let Some(mut child) = self.child.take() {
            Command::new("kill")
                .args(["-TERM", &child.id().to_string()])
                .status()
                .unwrap();
            let _ = child.wait();
        }
    }

    fn stop_by_hand(&mut self) {
        if let Some(mut child) = self.child.take() {
            Command::new("kill")
                .args(["-TERM", &child.id().to_string()])
                .status()
                .unwrap();
            let _ = child.wait();
        }
    }

    fn hosts_contains(&self, domain: &str) -> bool {
        std::fs::read_to_string(&self.hosts)
            .unwrap()
            .contains(domain)
    }
}

impl Drop for Agent {
    fn drop(&mut self) {
        self.kill();
    }
}

/// Same file `install` / install.ps1 write (`config::restart_expected_path`).
fn homeguard_agent_marker(data_dir: &Path) {
    std::fs::write(data_dir.join("restart-expected"), "e2e").unwrap();
}

fn agent_bin() -> PathBuf {
    PathBuf::from(
        std::env::var("HOMEGUARD_AGENT_BIN")
            .expect("set HOMEGUARD_AGENT_BIN to a built homeguard-agent"),
    )
}

fn local() -> SocketAddr {
    ([127, 0, 0, 1], AGENT_PORT).into()
}

fn manual_stops(state: &AppState, agent_id: &str) -> usize {
    state
        .db
        .recent_history(500)
        .unwrap()
        .into_iter()
        .filter(|h| h.agent_id.as_deref() == Some(agent_id) && h.kind == "manual_stop_detected")
        .count()
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "spawns the real agent binary; see module docs"]
async fn e2e_push_unblock_offline_and_restart() {
    let scratch = Scratch::new("push");
    let state = AppState::init(&scratch.0.join("app")).unwrap();
    let mut agent = Agent::new(&agent_bin(), &scratch.0);
    let agent_id = agent.enroll(&state, 17879).await;
    agent.start().await;

    // A rule change reaches the hosts file.
    let list = state.db.create_list("E2E", &[BLOCKED.to_string()]).unwrap();
    state
        .db
        .create_assignment(&list, TargetKind::Agent, &agent_id, None)
        .unwrap();
    state.db.mark_all_agents_dirty().unwrap();
    let outcome = sync::sync_one(&state, &agent_id, local()).await;
    assert!(outcome.ok, "{outcome:?}");
    assert!(agent.hosts_contains(BLOCKED));
    assert!(!state.db.get_agent(&agent_id).unwrap().unwrap().policy_dirty);

    // A temporary unblock reaches the poste too.
    let until = jiff::Timestamp::now() + jiff::SignedDuration::from_secs(600);
    state
        .db
        .create_exception(TargetKind::Agent, &agent_id, &[BLOCKED.to_string()], until)
        .unwrap();
    let outcome = sync::sync_one(&state, &agent_id, local()).await;
    assert!(outcome.ok, "{outcome:?}");
    assert!(
        !agent.hosts_contains(BLOCKED),
        "the exception should lift the block"
    );

    // Poste off: classified as unreachable, and the change stays pending.
    agent.kill();
    state.db.mark_all_agents_dirty().unwrap();
    let outcome = sync::sync_one(&state, &agent_id, local()).await;
    assert_eq!(
        outcome.error.as_ref().map(|e| e.kind),
        Some(SyncErrorKind::Unreachable),
        "{outcome:?}"
    );
    assert!(state.db.get_agent(&agent_id).unwrap().unwrap().policy_dirty);

    // Back on: "Tout synchroniser" (known postes at their last IP) regularizes.
    agent.start().await;
    let outcomes = sync::sync_known_agents(&state, &HashSet::new()).await;
    assert!(outcomes.iter().all(|o| o.ok), "{outcomes:?}");
    assert!(!state.db.get_agent(&agent_id).unwrap().unwrap().policy_dirty);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "spawns the real agent binary; see module docs"]
async fn e2e_reinstall_wrong_identity_and_no_false_manual_stop() {
    let scratch = Scratch::new("reinstall");
    let state = AppState::init(&scratch.0.join("app")).unwrap();
    let mut agent = Agent::new(&agent_bin(), &scratch.0);
    let first_id = agent.enroll(&state, 17880).await;
    agent.start().await;
    assert!(sync::sync_one(&state, &first_id, local()).await.ok);

    // Re-enroll while the old service still runs: it answers for the old
    // identity, which must be reported as such, not pushed to.
    let second_id = agent.enroll(&state, 17881).await;
    let outcome = sync::sync_one(&state, &second_id, local()).await;
    assert_eq!(
        outcome.error.as_ref().map(|e| e.kind),
        Some(SyncErrorKind::WrongIdentity),
        "{outcome:?}"
    );

    // The installer then restarts the service: that must not surface as
    // "arrêt manuel détecté" on either identity.
    agent.stop_like_an_installer();
    agent.start().await;
    let outcome = sync::sync_one(&state, &second_id, local()).await;
    assert!(outcome.ok, "{outcome:?}");
    assert_eq!(
        manual_stops(&state, &first_id) + manual_stops(&state, &second_id),
        0
    );

    // A real manual stop is still reported.
    agent.stop_by_hand();
    agent.start().await;
    assert!(sync::sync_one(&state, &second_id, local()).await.ok);
    assert_eq!(manual_stops(&state, &second_id), 1);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "spawns the real agent binary; see module docs"]
async fn e2e_poste_from_another_app_is_tls_rejected() {
    let scratch = Scratch::new("tls");
    let owner = AppState::init(&scratch.0.join("owner")).unwrap();
    let stranger = AppState::init(&scratch.0.join("stranger")).unwrap();
    let mut agent = Agent::new(&agent_bin(), &scratch.0);
    let agent_id = agent.enroll(&owner, 17882).await;
    agent.start().await;

    let row = owner.db.get_agent(&agent_id).unwrap().unwrap();
    stranger
        .db
        .upsert_agent(
            &agent_id,
            &row.hostname,
            &row.os,
            &row.arch,
            Some("127.0.0.1"),
            None,
        )
        .unwrap();
    let outcome = sync::sync_one(&stranger, &agent_id, local()).await;
    assert_eq!(
        outcome.error.as_ref().map(|e| e.kind),
        Some(SyncErrorKind::TlsRejected),
        "{outcome:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "waits on network timeouts; see module docs"]
async fn e2e_offline_postes_are_tried_in_parallel() {
    let scratch = Scratch::new("parallel");
    let state = AppState::init(&scratch.0.join("app")).unwrap();
    // Unroutable addresses: each one costs the full client timeout.
    for i in 1..=4 {
        state
            .db
            .upsert_agent(
                &format!("off-{i}"),
                &format!("off-{i}"),
                "linux",
                "x86_64",
                Some(&format!("10.255.255.{i}")),
                None,
            )
            .unwrap();
    }
    let started = Instant::now();
    let outcomes = sync::sync_known_agents(&state, &HashSet::new()).await;
    let elapsed = started.elapsed();
    assert_eq!(outcomes.len(), 4);
    assert!(outcomes.iter().all(|o| !o.ok));
    assert!(
        elapsed < Duration::from_secs(9),
        "pushes ran one after the other ({elapsed:?})"
    );
}
