//! Everything that pushes a policy to a poste: the one-poste sync used by
//! every entry point (the "Synchroniser" buttons, the scan, the automatic
//! push after an edit), the classification of its failures into something
//! a parent can act on, and the debounced automatic push itself.
//!
//! Every attempt — success or failure — is recorded on the poste
//! (`Db::record_sync_result`), so the UI can always say when it last
//! worked and why it didn't, instead of only reacting to the click that
//! happened to be on screen.

use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use homeguard_app_core::client::{connect_to_agent, AgentClient, ClientError, TlsFailure};
use homeguard_app_core::policy_engine::{resolve_policy_for_agent, PolicyEngineError};
use homeguard_common::wire::AgentStatus;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::Notify;

use crate::state::AppState;

pub const AGENT_PORT: u16 = 7878;

/// Coalesces a burst of edits (typing several domains, ticking several
/// schedule cells...) into a single push.
const AUTO_PUSH_DEBOUNCE: Duration = Duration::from_millis(800);

/// Emitted when a push run starts / finishes, so the UI can show a
/// spinner and a result toast whatever triggered it.
pub const EVENT_SYNC_STARTED: &str = "sync-started";
pub const EVENT_SYNC_FINISHED: &str = "sync-finished";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncErrorKind {
    /// Nothing listening / no route: off, asleep, off the network, or the
    /// agent service isn't running.
    Unreachable,
    Timeout,
    /// The TLS handshake failed — almost always a poste still running an
    /// identity this app no longer recognizes (old enrollment, other CA).
    TlsRejected,
    /// Something answered at that address, but it's another enrollment of
    /// the agent (typically: an old service still running after a
    /// re-enrollment).
    WrongIdentity,
    /// Answered, but not in a shape this app understands — an agent too
    /// old (or too new) for this app.
    Incompatible,
    Revoked,
    /// The app has never seen this poste on the network.
    NoAddress,
    Other,
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncFailure {
    pub kind: SyncErrorKind,
    /// Human-readable, French, with what to do about it.
    pub message: String,
}

impl SyncFailure {
    fn new(kind: SyncErrorKind, raw: impl std::fmt::Display) -> Self {
        let message = match kind {
            SyncErrorKind::Unreachable => {
                "Injoignable — le poste est éteint, hors du réseau, ou l'agent ne tourne pas.".to_string()
            }
            SyncErrorKind::Timeout => "Pas de réponse à temps — le poste est peut-être en veille.".to_string(),
            SyncErrorKind::TlsRejected => {
                "Connexion sécurisée refusée — ce poste utilise sans doute un ancien enrôlement : réenrôlez-le."
                    .to_string()
            }
            SyncErrorKind::WrongIdentity => {
                "Un autre agent répond à cette adresse (ancien enrôlement encore actif ?) — réinstallez l'agent sur ce poste."
                    .to_string()
            }
            SyncErrorKind::Incompatible => {
                "Réponse inattendue — l'agent est sans doute d'une version incompatible : mettez-le à jour ou réinstallez-le."
                    .to_string()
            }
            SyncErrorKind::Revoked => "Ce poste a été retiré — il ne reçoit plus de règles.".to_string(),
            SyncErrorKind::NoAddress => {
                "Adresse inconnue — le poste n'a pas encore été vu sur le réseau.".to_string()
            }
            SyncErrorKind::Other => format!("Échec de la synchronisation : {raw}"),
        };
        SyncFailure { kind, message }
    }
}

/// Maps a low-level client error to what it most likely means for the
/// parent looking at the screen.
pub fn classify(e: &ClientError) -> SyncFailure {
    let kind = match e {
        ClientError::AgentRevoked(_) => SyncErrorKind::Revoked,
        ClientError::UnexpectedStatus(_) => SyncErrorKind::Incompatible,
        ClientError::Request(re) => match e.tls_failure() {
            // Checked before `is_connect`: a rejected certificate surfaces
            // either as a connect error (the app refused the agent's) or a
            // send error (the agent refused the app's, via a TLS alert).
            Some(TlsFailure::Rejected) => SyncErrorKind::TlsRejected,
            Some(TlsFailure::NotTls) => SyncErrorKind::Incompatible,
            None if re.is_timeout() => SyncErrorKind::Timeout,
            None if re.is_connect() => SyncErrorKind::Unreachable,
            None if re.is_decode() || re.is_body() => SyncErrorKind::Incompatible,
            None => SyncErrorKind::Other,
        },
        _ => SyncErrorKind::Other,
    };
    SyncFailure::new(kind, e)
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncOutcome {
    pub agent_id: String,
    pub hostname: String,
    pub ok: bool,
    /// Success: a one-line summary. Failure: `error.message`.
    pub detail: String,
    pub error: Option<SyncFailure>,
    pub domains_blocked: Option<u64>,
    pub policy_version: Option<u64>,
    /// `Some(false)` is a real écart: the agent answered but its hosts
    /// file no longer matched what its policy said before this sync.
    /// `None` means we couldn't even ask.
    pub integrity_ok: Option<bool>,
}

struct Pushed {
    pre: AgentStatus,
    post: AgentStatus,
}

/// Connects to `addr`, checks it's really the poste we think it is, pulls
/// its events, pushes a freshly-resolved policy and records the outcome.
/// Never fails as such: a failure is an outcome too.
pub async fn sync_one(state: &AppState, agent_id: &str, addr: SocketAddr) -> SyncOutcome {
    // One push at a time: an automatic push and a manual click landing
    // together would otherwise race on the policy version (If-Match).
    let _guard = state.sync_lock.lock().await;

    let display = state
        .db
        .get_agent(agent_id)
        .ok()
        .flatten()
        .map(|a| a.display_name.filter(|n| !n.is_empty()).unwrap_or(a.hostname))
        .unwrap_or_else(|| agent_id.to_string());

    let result = push_policy(state, agent_id, addr).await;
    let outcome = match result {
        Ok(Pushed { pre, post }) => {
            let mut detail = format!(
                "{} domaine{} bloqué{} · politique v{}",
                post.domains_blocked_now,
                plural(post.domains_blocked_now),
                plural(post.domains_blocked_now),
                post.applied_policy_version
            );
            if !pre.integrity_ok {
                detail.push_str(" — écart corrigé sur le fichier hosts");
            }
            SyncOutcome {
                agent_id: agent_id.to_string(),
                hostname: display,
                ok: true,
                detail,
                error: None,
                domains_blocked: Some(post.domains_blocked_now),
                policy_version: Some(post.applied_policy_version),
                integrity_ok: Some(pre.integrity_ok),
            }
        }
        Err(failure) => SyncOutcome {
            agent_id: agent_id.to_string(),
            hostname: display,
            ok: false,
            detail: failure.message.clone(),
            error: Some(failure),
            domains_blocked: None,
            policy_version: None,
            integrity_ok: None,
        },
    };
    let _ = state.db.record_sync_result(agent_id, outcome.ok, &outcome.detail);
    outcome
}

fn plural(n: u64) -> &'static str {
    if n > 1 { "s" } else { "" }
}

async fn push_policy(state: &AppState, agent_id: &str, addr: SocketAddr) -> Result<Pushed, SyncFailure> {
    let client = connect_to_agent(
        &state.db,
        agent_id,
        addr,
        &state.client_identity.cert_pem,
        &state.client_identity.key_pem,
        state.ca.cert_der(),
    )
    .map_err(|e| classify(&e))?;

    // The certificate only proves "some poste our CA enrolled under this
    // hostname" — after a re-enrollment, the old service can still be the
    // one answering. Catch that explicitly rather than pushing to it.
    let info = client.info().await.map_err(|e| classify(&e))?;
    if info.agent_id != agent_id {
        return Err(SyncFailure::new(SyncErrorKind::WrongIdentity, ""));
    }

    let pre = client.status().await.map_err(|e| classify(&e))?;

    // Pull agent-side events (self-heal, schedule transitions, a manual
    // stop...) into the unified history before pushing a new policy.
    if let Some(agent) = state.db.get_agent(agent_id).map_err(other)? {
        let page = client.events_since(agent.events_cursor).await.map_err(|e| classify(&e))?;
        for event in &page.events {
            state.db.record_agent_event(agent_id, event).map_err(other)?;
        }
        if page.next_seq != agent.events_cursor {
            state.db.set_agent_events_cursor(agent_id, page.next_seq).map_err(other)?;
        }
    }

    let mut current_version = pre.applied_policy_version;
    let mut attempts = 0;
    let version = loop {
        attempts += 1;
        let version = push_once(state, &client, agent_id, current_version).await;
        match version {
            Ok(v) => break v,
            // Someone (a previous run, another app instance) moved the
            // agent on since we read its status: re-read, retry once.
            Err(ClientError::VersionConflict) if attempts < 2 => {
                current_version = client.status().await.map_err(|e| classify(&e))?.applied_policy_version;
            }
            Err(e) => return Err(classify(&e)),
        }
    };
    let _ = state
        .db
        .append_history("app", Some(agent_id), "policy_pushed", &format!("politique v{version} envoyée"));

    let post = client.status().await.map_err(|e| classify(&e))?;
    Ok(Pushed { pre, post })
}

async fn push_once(
    state: &AppState,
    client: &AgentClient,
    agent_id: &str,
    current_version: u64,
) -> Result<u64, ClientError> {
    let timezone = state.timezone();
    let version = state.db.bump_policy_version(agent_id)?;
    // TODO: protection flags (self-heal / anti-DoH) should become a
    // per-agent or per-group setting; both default on for now.
    let policy = resolve_policy_for_agent(&state.db, agent_id, version, &timezone, true, true, state.auto_www())
        .map_err(|PolicyEngineError::Db(e)| ClientError::Db(e))?;
    client.push_policy(&policy, Some(current_version)).await?;
    Ok(version)
}

fn other(e: impl std::fmt::Display) -> SyncFailure {
    SyncFailure::new(SyncErrorKind::Other, e)
}

/// Syncs every known, non-revoked poste not in `skip`, at its last known
/// address — the scan uses this for postes mDNS didn't surface, so a
/// poste that's off still shows up as "injoignable" rather than silently
/// missing from the results.
pub async fn sync_known_agents(state: &AppState, skip: &HashSet<String>) -> Vec<SyncOutcome> {
    let Ok(agents) = state.db.list_agents() else {
        return Vec::new();
    };
    let mut outcomes = Vec::new();
    for agent in agents {
        if agent.revoked || skip.contains(&agent.id) {
            continue;
        }
        let addr = agent
            .last_ip
            .as_deref()
            .and_then(|ip| ip.parse::<std::net::IpAddr>().ok())
            .map(|ip| SocketAddr::new(ip, AGENT_PORT));
        let outcome = match addr {
            Some(addr) => sync_one(state, &agent.id, addr).await,
            None => {
                let failure = SyncFailure::new(SyncErrorKind::NoAddress, "");
                let _ = state.db.record_sync_result(&agent.id, false, &failure.message);
                SyncOutcome {
                    agent_id: agent.id.clone(),
                    hostname: agent.display_name.filter(|n| !n.is_empty()).unwrap_or(agent.hostname),
                    ok: false,
                    detail: failure.message.clone(),
                    error: Some(failure),
                    domains_blocked: None,
                    policy_version: None,
                    integrity_ok: None,
                }
            }
        };
        outcomes.push(outcome);
    }
    outcomes
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncRunEvent {
    /// "auto" (after an edit) — the only one emitted for now; manual runs
    /// return their outcomes directly to the caller.
    pub trigger: &'static str,
    pub outcomes: Vec<SyncOutcome>,
}

/// Handle used by commands to ask for an automatic push.
#[derive(Clone, Default)]
pub struct AutoPush {
    notify: Arc<Notify>,
}

impl AutoPush {
    /// Marks every poste as having pending changes and wakes the push
    /// loop. Cheap and non-blocking — safe to call from any command.
    pub fn request(&self, state: &AppState) {
        let _ = state.db.mark_all_agents_dirty();
        self.notify.notify_one();
    }

    /// Spawns the loop that turns requests into (debounced) pushes.
    pub fn start(&self, app: AppHandle) {
        let notify = self.notify.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                notify.notified().await;
                tokio::time::sleep(AUTO_PUSH_DEBOUNCE).await;
                let state = app.state::<AppState>();
                let _ = app.emit(EVENT_SYNC_STARTED, ());
                let outcomes = sync_known_agents(&state, &HashSet::new()).await;
                let _ = app.emit(EVENT_SYNC_FINISHED, SyncRunEvent { trigger: "auto", outcomes });
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revoked_is_classified_as_such() {
        let f = classify(&ClientError::AgentRevoked("a1".into()));
        assert_eq!(f.kind, SyncErrorKind::Revoked);
    }

    #[test]
    fn unexpected_status_means_incompatible_agent() {
        let f = classify(&ClientError::UnexpectedStatus(reqwest::StatusCode::NOT_FOUND));
        assert_eq!(f.kind, SyncErrorKind::Incompatible);
        assert!(f.message.contains("version"));
    }

    #[tokio::test]
    async fn connection_refused_is_unreachable() {
        // Port 9 on localhost: nothing listens there in practice.
        let e = reqwest::Client::new().get("http://127.0.0.1:9/").send().await.unwrap_err();
        let f = classify(&ClientError::Request(e));
        assert_eq!(f.kind, SyncErrorKind::Unreachable);
    }
}
