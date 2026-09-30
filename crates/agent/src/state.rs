use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};
use homeguard_common::policy::Policy;
use homeguard_common::wire::EventKind;
use tokio::sync::Notify;

use crate::events::EventLog;
use crate::hosts_writer;

pub const HEARTBEAT: Duration = Duration::from_secs(60);

pub struct AppState {
    pub agent_id: String,
    pub hostname: String,
    pub os: String,
    pub arch: String,
    pub agent_version: String,
    pub started_at: Instant,
    pub hosts_path: PathBuf,
    pub policy_path: PathBuf,
    /// `/etc` in production; a temp dir in tests. Ignored on Windows,
    /// where browser policy lives in the registry instead of files.
    pub etc_root: PathBuf,
    pub events: Arc<EventLog>,
    pub policy: RwLock<Policy>,
    pub last_applied: RwLock<BTreeSet<String>>,
    pub policy_changed: Notify,
}

impl AppState {
    pub fn uptime_secs(&self) -> u64 {
        self.started_at.elapsed().as_secs()
    }

    pub fn set_policy(&self, policy: Policy) -> std::io::Result<()> {
        let json = serde_json::to_string_pretty(&policy).expect("Policy always serializes");
        std::fs::write(&self.policy_path, json)?;
        *self.policy.write().unwrap() = policy;
        self.policy_changed.notify_one();
        Ok(())
    }

    pub fn current_policy(&self) -> Policy {
        self.policy.read().unwrap().clone()
    }
}

/// Recompute the domains the current policy demands right now, and
/// reconcile the hosts file with that set. Also self-heals: even when the
/// desired set hasn't changed, it checks the file still matches what was
/// last written and restores it if a human (or another program) touched
/// it. Returns `Ok(true)` if the hosts file was (re)written.
pub fn recompute_and_apply(state: &AppState) -> anyhow::Result<bool> {
    let policy = state.current_policy();
    let now = jiff::Timestamp::now();
    let desired = policy.effective_domains_at(now)?;

    if let Err(e) = crate::browser_policy::apply(&state.etc_root, policy.protection.disable_doh) {
        tracing::warn!("échec de la mise à jour des politiques navigateurs : {e}");
    }

    let mut last_applied = state.last_applied.write().unwrap();
    let desired_changed = *last_applied != desired;

    let needs_self_heal = policy.protection.self_heal
        && !desired_changed
        && hosts_writer::read_current_domains(&state.hosts_path)
            .ok()
            .flatten()
            .map(|current| current.into_iter().collect::<BTreeSet<_>>())
            .as_ref()
            != Some(&desired);

    if !desired_changed && !needs_self_heal {
        return Ok(false);
    }

    if needs_self_heal {
        state
            .events
            .append(EventKind::IntegrityViolationDetected, "hosts modifié en dehors de l'agent")
            .ok();
    }

    let domains_vec: Vec<String> = desired.iter().cloned().collect();
    hosts_writer::write_domains(&state.hosts_path, &domains_vec)?;
    *last_applied = desired;
    drop(last_applied);

    if needs_self_heal {
        state
            .events
            .append(EventKind::IntegrityRestored, "hosts restauré automatiquement")
            .ok();
    } else {
        state
            .events
            .append(
                EventKind::PolicyApplied,
                format!(
                    "{} domaine(s) bloqué(s) (politique v{})",
                    domains_vec.len(),
                    policy.version
                ),
            )
            .ok();
    }

    Ok(true)
}

pub fn hosts_block_hash(domains: &BTreeSet<String>) -> String {
    let joined = domains.iter().cloned().collect::<Vec<_>>().join("\n");
    let digest = Sha256::digest(joined.as_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// Runs forever: recomputes on every policy change notification, and at
/// least once per [`HEARTBEAT`] regardless (schedule transitions and
/// self-heal checks both ride on this).
pub async fn run_scheduler(state: Arc<AppState>) {
    loop {
        if let Err(e) = recompute_and_apply(&state) {
            tracing::error!("échec de l'application de la politique : {e:#}");
        }

        tokio::select! {
            _ = state.policy_changed.notified() => {}
            _ = tokio::time::sleep(HEARTBEAT) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use homeguard_common::policy::{Protection, Rule};

    fn test_state(dir: &std::path::Path) -> AppState {
        let events = Arc::new(EventLog::open(dir.join("events.jsonl")).unwrap());
        AppState {
            agent_id: "test-agent".into(),
            hostname: "test-host".into(),
            os: "linux".into(),
            arch: "x86_64".into(),
            agent_version: env!("CARGO_PKG_VERSION").into(),
            started_at: Instant::now(),
            hosts_path: dir.join("hosts"),
            policy_path: dir.join("policy.json"),
            etc_root: dir.join("etc"),
            events,
            policy: RwLock::new(Policy {
                version: 0,
                timezone: "UTC".into(),
                rules: vec![],
                exceptions: vec![],
                protection: Protection::default(),
            }),
            last_applied: RwLock::new(BTreeSet::new()),
            policy_changed: Notify::new(),
        }
    }

    #[test]
    fn applies_policy_domains_to_hosts_file() {
        let dir = tempfile::tempdir().unwrap();
        let state = test_state(dir.path());
        std::fs::write(&state.hosts_path, "127.0.0.1 localhost\n").unwrap();

        state
            .set_policy(Policy {
                version: 1,
                timezone: "UTC".into(),
                rules: vec![Rule {
                    id: "social".into(),
                    name: "Réseaux sociaux".into(),
                    domains: vec!["tiktok.com".into()],
                    schedule: None,
                }],
                exceptions: vec![],
                protection: Protection::default(),
            })
            .unwrap();

        assert!(recompute_and_apply(&state).unwrap());
        let content = std::fs::read_to_string(&state.hosts_path).unwrap();
        assert!(content.contains("0.0.0.0 tiktok.com"));

        // Recomputing again with nothing changed and self-heal off: no-op.
        assert!(!recompute_and_apply(&state).unwrap());
    }

    #[test]
    fn self_heals_when_enabled_and_hosts_tampered_with() {
        let dir = tempfile::tempdir().unwrap();
        let state = test_state(dir.path());
        state
            .set_policy(Policy {
                version: 1,
                timezone: "UTC".into(),
                rules: vec![Rule {
                    id: "social".into(),
                    name: "Réseaux sociaux".into(),
                    domains: vec!["tiktok.com".into()],
                    schedule: None,
                }],
                exceptions: vec![],
                protection: Protection {
                    self_heal: true,
                    disable_doh: false,
                },
            })
            .unwrap();
        recompute_and_apply(&state).unwrap();

        // Tamper: drop the managed block entirely.
        std::fs::write(&state.hosts_path, "127.0.0.1 localhost\n").unwrap();
        assert!(recompute_and_apply(&state).unwrap());
        let content = std::fs::read_to_string(&state.hosts_path).unwrap();
        assert!(content.contains("0.0.0.0 tiktok.com"));

        let (events, _) = state.events.since(1).unwrap();
        assert!(events
            .iter()
            .any(|e| e.kind == EventKind::IntegrityViolationDetected));
        assert!(events.iter().any(|e| e.kind == EventKind::IntegrityRestored));
    }

    #[test]
    fn does_not_self_heal_when_disabled() {
        let dir = tempfile::tempdir().unwrap();
        let state = test_state(dir.path());
        state
            .set_policy(Policy {
                version: 1,
                timezone: "UTC".into(),
                rules: vec![Rule {
                    id: "social".into(),
                    name: "Réseaux sociaux".into(),
                    domains: vec!["tiktok.com".into()],
                    schedule: None,
                }],
                exceptions: vec![],
                protection: Protection::default(),
            })
            .unwrap();
        recompute_and_apply(&state).unwrap();

        std::fs::write(&state.hosts_path, "127.0.0.1 localhost\n").unwrap();
        assert!(!recompute_and_apply(&state).unwrap());
        let content = std::fs::read_to_string(&state.hosts_path).unwrap();
        assert!(!content.contains("tiktok.com"));
    }
}
