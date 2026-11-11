//! Wire types exchanged between the app and an agent over the mTLS API.
//! Kept in `common` so both sides (and tests) share the exact same shapes.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentInfo {
    pub agent_id: String,
    pub hostname: String,
    pub os: String,
    /// `std::env::consts::ARCH` (e.g. `"x86_64"`, `"aarch64"`) — `os` alone
    /// doesn't distinguish the two Linux agent binaries, so picking the
    /// right one for a remote update needs this too.
    pub arch: String,
    pub agent_version: String,
    pub uptime_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentStatus {
    /// Policy version currently applied to the hosts file (0 if none yet).
    pub applied_policy_version: u64,
    pub domains_blocked_now: u64,
    /// SHA-256 (lowercase hex) of the sorted, newline-joined list of
    /// currently-blocked domains. Lets the app detect drift without
    /// transferring the full list on every scan.
    pub hosts_block_hash: String,
    /// False when the on-disk hosts file no longer matches what the agent
    /// last wrote (hand edit, another tool, etc).
    pub integrity_ok: bool,
    pub self_heal_enabled: bool,
    pub disable_doh_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventEntry {
    pub seq: u64,
    /// RFC 3339 timestamp.
    pub at: String,
    pub kind: EventKind,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    AgentStarted,
    PolicyApplied,
    ScheduleTransition,
    IntegrityViolationDetected,
    IntegrityRestored,
    ManualStopDetected,
    Enrolled,
    Uninstalled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventsPage {
    pub events: Vec<EventEntry>,
    /// Pass back as `since` to continue reading from where this page ended.
    pub next_seq: u64,
}

/// Body for the enrollment request an agent sends to the app's temporary
/// enrollment server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrollRequest {
    pub token: String,
    pub csr_pem: String,
    pub hostname: String,
    pub os: String,
    pub arch: String,
    pub agent_version: String,
    /// Domains already present in this machine's `hosts` file managed
    /// block — current markers, or a pre-rewrite `SITEBLOCKER` one —
    /// collected *before* enrollment touches anything. Lets the app
    /// surface "this poste already had rules" instead of silently
    /// discarding them. Empty for a poste with no prior block, or an
    /// older agent build that doesn't send this field at all (hence
    /// `#[serde(default)]`, so the app's enrollment server doesn't reject
    /// requests from agents that predate this).
    #[serde(default)]
    pub existing_blocked_domains: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrollResponse {
    pub cert_pem: String,
    pub ca_cert_pem: String,
    pub agent_id: String,
    pub initial_policy: crate::policy::Policy,
}
