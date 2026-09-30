use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use homeguard_app_core::client::connect_to_agent;
use homeguard_app_core::db::{Agent, Assignment, Exception, Group, HistoryEntry, List, TargetKind};
use homeguard_app_core::discovery::{browse_once, DiscoveredAgent};
use homeguard_app_core::enrollment_server::EnrollmentState;
use homeguard_app_core::policy_engine::resolve_policy_for_agent;
use homeguard_common::policy::TimeWindow;
use homeguard_common::wire::AgentStatus;
use tauri::State;

use crate::state::{AppState, EnrollmentSession};

// Every command's error becomes a plain String on the JS side — good
// enough for an admin tool where the message itself is the useful part.
type CmdResult<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

// --- agents ------------------------------------------------------------

#[tauri::command]
pub fn list_agents(state: State<'_, AppState>) -> CmdResult<Vec<Agent>> {
    state.db.list_agents().map_err(err)
}

#[tauri::command]
pub fn set_agent_display_name(state: State<'_, AppState>, agent_id: String, display_name: String) -> CmdResult<()> {
    state.db.set_agent_display_name(&agent_id, &display_name).map_err(err)
}

/// Marks the agent revoked (the app will refuse to connect to it from now
/// on) and tells the agent itself to restore its hosts file, lift its
/// browser policies and uninstall — best-effort: if it's offline, the
/// revocation still takes effect locally.
#[tauri::command]
pub async fn remove_agent(state: State<'_, AppState>, agent_id: String, addr: Option<String>) -> CmdResult<()> {
    if let Some(addr) = addr {
        let addr: SocketAddr = addr.parse().map_err(err)?;
        if let Ok(client) = connect_to_agent(
            &state.db,
            &agent_id,
            addr,
            &state.client_identity.cert_pem,
            &state.client_identity.key_pem,
            state.ca.cert_der(),
        ) {
            let _ = client.uninstall().await;
        }
    }
    state.db.revoke_agent(&agent_id).map_err(err)?;
    state
        .db
        .append_history("app", Some(&agent_id), "removed", "poste retiré et certificat révoqué")
        .map_err(err)
}

#[tauri::command]
pub async fn discover_agents(timeout_ms: u64) -> CmdResult<Vec<DiscoveredAgent>> {
    tokio::task::spawn_blocking(move || browse_once(Duration::from_millis(timeout_ms)))
        .await
        .map_err(err)?
        .map_err(err)
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncOutcome {
    pub agent_id: String,
    pub hostname: String,
    pub ok: bool,
    pub detail: String,
    /// `Some(false)` is a real écart: the agent answered but its hosts
    /// file no longer matches what its policy says it should. `None`
    /// means we couldn't even ask (agent unreachable, revoked...).
    pub integrity_ok: Option<bool>,
}

/// The "scan on launch" / "⟳" action: browse mDNS, and for every agent
/// that answers, make sure it has a DB record (creating one if this is
/// the first time we've seen it, e.g. after a fresh install — an agent
/// can only present a certificate our CA actually signed, so trusting a
/// newly-seen `agent_id` here is no different from trusting the mTLS
/// handshake itself) and push it whatever policy currently applies.
/// Revoked agents are recorded but left alone — `sync_agent` (via
/// `connect_to_agent`) refuses to talk to them.
#[tauri::command]
pub async fn discover_and_sync_all(state: State<'_, AppState>, timeout_ms: u64) -> CmdResult<Vec<SyncOutcome>> {
    let discovered = discover_agents(timeout_ms).await?;
    let mut outcomes = Vec::with_capacity(discovered.len());

    for agent in discovered {
        if let Err(e) = state.db.upsert_agent(
            &agent.agent_id,
            &agent.hostname,
            &agent.os,
            &agent.arch,
            Some(&agent.addr.ip().to_string()),
            Some(&agent.agent_version),
        ) {
            outcomes.push(SyncOutcome {
                agent_id: agent.agent_id,
                hostname: agent.hostname,
                ok: false,
                detail: err(e),
                integrity_ok: None,
            });
            continue;
        }

        let result = sync_agent(state.clone(), agent.agent_id.clone(), agent.addr.to_string()).await;
        outcomes.push(match result {
            Ok(sync) => {
                let mut detail = format!(
                    "{} domaine(s) bloqué(s), politique v{}",
                    sync.post.domains_blocked_now, sync.post.applied_policy_version
                );
                if !sync.pre.integrity_ok {
                    detail.push_str(" — écart détecté sur le hosts avant cette synchronisation");
                }
                SyncOutcome {
                    agent_id: agent.agent_id,
                    hostname: agent.hostname,
                    ok: true,
                    detail,
                    integrity_ok: Some(sync.pre.integrity_ok),
                }
            }
            Err(e) => SyncOutcome {
                agent_id: agent.agent_id,
                hostname: agent.hostname,
                ok: false,
                detail: e,
                integrity_ok: None,
            },
        });
    }

    Ok(outcomes)
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncResult {
    /// Status as found *before* this sync pushed anything — this is the
    /// one that actually reflects drift (a hand-edited hosts file, a
    /// stopped self-heal...). Pushing a fresh policy below usually
    /// reconciles it, so checking `post` for that would nearly always
    /// read back "fine".
    pub pre: AgentStatus,
    pub post: AgentStatus,
}

/// Connects to `addr`, fetches status, pushes a freshly-resolved policy
/// (from whatever groups/lists/assignments currently apply), and returns
/// both the status from before and after that push. This is what
/// "Synchroniser" and the auto-sync on launch both call.
#[tauri::command]
pub async fn sync_agent(state: State<'_, AppState>, agent_id: String, addr: String) -> CmdResult<SyncResult> {
    let addr: SocketAddr = addr.parse().map_err(err)?;
    let client = connect_to_agent(
        &state.db,
        &agent_id,
        addr,
        &state.client_identity.cert_pem,
        &state.client_identity.key_pem,
        state.ca.cert_der(),
    )
    .map_err(err)?;

    let current_status = client.status().await.map_err(err)?;

    // Pull any agent-side events (self-heal, schedule transitions, a
    // manual stop...) into the unified history before pushing a new
    // policy — that push will itself emit an event, but we'll only see
    // it on the *next* sync, which is fine.
    if let Some(agent) = state.db.get_agent(&agent_id).map_err(err)? {
        let page = client.events_since(agent.events_cursor).await.map_err(err)?;
        for event in &page.events {
            state.db.record_agent_event(&agent_id, event).map_err(err)?;
        }
        if page.next_seq != agent.events_cursor {
            state.db.set_agent_events_cursor(&agent_id, page.next_seq).map_err(err)?;
        }
    }

    let timezone = state.timezone();
    let version = state.db.bump_policy_version(&agent_id).map_err(err)?;
    // TODO: protection flags (self-heal / anti-DoH) should become a
    // per-agent or per-group setting; both default on for now.
    let policy = resolve_policy_for_agent(&state.db, &agent_id, version, &timezone, true, true).map_err(err)?;

    client
        .push_policy(&policy, Some(current_status.applied_policy_version))
        .await
        .map_err(err)?;
    let _ = state
        .db
        .append_history("app", Some(&agent_id), "policy_pushed", &format!("politique v{version} envoyée"));

    let post = client.status().await.map_err(err)?;
    Ok(SyncResult { pre: current_status, post })
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentLiveStatus {
    pub agent_id: String,
    /// Whether this poste answered a plain `/v1/status` just now. This is
    /// the actual "is it up" signal — unlike `Agent.last_ip` being set,
    /// which only means the app has *once* seen this poste somewhere, not
    /// that it's reachable right now.
    pub online: bool,
}

/// Probes every known, non-revoked poste with a known IP for real
/// reachability, right now — a plain status read, no policy push, no side
/// effect beyond bumping `last_seen_at` on success. Agents are never
/// polled in the background in this app (see PLAN.md), so this is only
/// ever as fresh as the last time it (or `sync_agent` /
/// `discover_and_sync_all`) actually ran — typically on opening the Postes
/// screen.
#[tauri::command]
pub async fn refresh_agents_status(state: State<'_, AppState>) -> CmdResult<Vec<AgentLiveStatus>> {
    let agents = state.db.list_agents().map_err(err)?;
    let mut results = Vec::with_capacity(agents.len());

    for agent in agents {
        if agent.revoked {
            continue;
        }
        let online = probe_agent(&state, &agent).await;
        if online {
            let _ = state.db.touch_agent_last_seen(&agent.id);
        }
        results.push(AgentLiveStatus { agent_id: agent.id, online });
    }
    Ok(results)
}

async fn probe_agent(state: &AppState, agent: &Agent) -> bool {
    let Some(last_ip) = &agent.last_ip else {
        return false;
    };
    let Ok(addr) = format!("{last_ip}:7878").parse::<SocketAddr>() else {
        return false;
    };
    let Ok(client) = connect_to_agent(
        &state.db,
        &agent.id,
        addr,
        &state.client_identity.cert_pem,
        &state.client_identity.key_pem,
        state.ca.cert_der(),
    ) else {
        return false;
    };
    client.status().await.is_ok()
}

/// The agent's own service manager restarts it `RestartSec=5` after it
/// exits post-update (systemd — see `service.rs::unit_file_contents`) or
/// `restart/5000` (Windows SCM — see `service.rs`'s Windows `install_service`),
/// plus the ~300ms it waits before exiting (`api.rs::post_update`) and
/// whatever its own startup takes. Wait a bit past that worst case before
/// the first reconnect attempt, then retry a few times — a poste that's
/// simply slow to boot shouldn't be reported as a failed update.
const UPDATE_RESTART_GRACE: Duration = Duration::from_secs(5);
const UPDATE_POLL_INTERVAL: Duration = Duration::from_secs(2);
const UPDATE_POLL_ATTEMPTS: u32 = 6;

#[derive(Debug, Clone, Serialize)]
pub struct UpdateOutcome {
    /// Whether the poste answered at all after the restart grace period —
    /// `false` means no confirmation either way (could still be slow, or
    /// could genuinely be stuck; `push_update` having succeeded only means
    /// the agent *accepted* the update before this point).
    pub came_back_online: bool,
    /// The version it actually reported, when it did answer.
    pub reported_version: Option<String>,
    /// Whether `reported_version` matches the version that was just sent
    /// — `false` while still matching `came_back_online: true` would mean
    /// it came back but didn't actually change version (update silently
    /// no-opped, or this poste was already current).
    pub matches_expected: bool,
}

/// One-line, UI-ready description of an [`UpdateOutcome`] plus whether it
/// counts as a success — shared between the single-poste and batch update
/// commands so their history entries and result lists read the same way.
fn describe_outcome(outcome: &UpdateOutcome) -> (bool, String) {
    match &outcome.reported_version {
        Some(v) if outcome.matches_expected => (true, format!("poste revenu en ligne sur la version {v}")),
        Some(v) => (false, format!("poste revenu en ligne mais annonce la version {v}, pas celle attendue")),
        None => (false, "le poste n'a pas répondu après la mise à jour — à vérifier manuellement".to_string()),
    }
}

/// Sends this build's bundled agent binary to one poste, tells it to apply
/// it (`POST /v1/update`, checksummed — see
/// `homeguard_agent::update::apply_update`), then waits for it to come
/// back online and confirms which version it's actually running —
/// pushing a new binary only proves the agent *accepted* it, not that the
/// restart that follows (it immediately replaces its own executable and
/// exits, trusting its service manager to relaunch it — `Restart=always`
/// on Linux, `sc failure ... restart` on Windows) actually succeeded.
/// Shared by [`update_agent`] (one poste, called from the UI's "Mettre à
/// jour" button) and [`update_all_outdated_agents`] (every outdated poste
/// at once).
async fn perform_update(app: &tauri::AppHandle, state: &AppState, agent: &Agent, addr: SocketAddr) -> CmdResult<UpdateOutcome> {
    let target = homeguard_app_core::deploy_server::AgentTarget::from_os_arch(&agent.os, &agent.arch)
        .ok_or_else(|| format!("aucun binaire agent compilé pour {} / {}", agent.os, agent.arch))?;

    let binary_path = discover_agent_binaries(app)
        .remove(&target)
        .ok_or_else(|| "aucun binaire agent embarqué dans cette build de l'application".to_string())?;
    let binary = std::fs::read(&binary_path).map_err(err)?;
    let sha256 = homeguard_common::sha256_hex(&binary);
    let expected_version = bundled_agent_version(app.clone());

    let client = connect_to_agent(
        &state.db,
        &agent.id,
        addr,
        &state.client_identity.cert_pem,
        &state.client_identity.key_pem,
        state.ca.cert_der(),
    )
    .map_err(err)?;

    client.push_update(&binary, &sha256).await.map_err(err)?;
    let _ = state
        .db
        .append_history("app", Some(&agent.id), "update_pushed", "mise à jour envoyée au poste");

    let reported_version = wait_for_agent_back_online(state, &agent.id, addr).await;
    let matches_expected = matches!((&reported_version, &expected_version), (Some(v), Some(exp)) if v == exp);
    let outcome = UpdateOutcome {
        came_back_online: reported_version.is_some(),
        reported_version,
        matches_expected,
    };

    let (_, detail) = describe_outcome(&outcome);
    let _ = state.db.append_history(
        "app",
        Some(&agent.id),
        if outcome.came_back_online { "update_confirmed" } else { "update_unconfirmed" },
        &detail,
    );

    // Keep the DB's own record current so the "poste obsolète" badge
    // clears immediately instead of waiting for the next discovery/sync.
    if let Some(v) = &outcome.reported_version {
        let _ = state
            .db
            .upsert_agent(&agent.id, &agent.hostname, &agent.os, &agent.arch, Some(&addr.ip().to_string()), Some(v));
    }

    Ok(outcome)
}

#[tauri::command]
pub async fn update_agent(app: tauri::AppHandle, state: State<'_, AppState>, agent_id: String, addr: String) -> CmdResult<UpdateOutcome> {
    let agent = state
        .db
        .get_agent(&agent_id)
        .map_err(err)?
        .ok_or_else(|| "poste inconnu".to_string())?;
    let addr: SocketAddr = addr.parse().map_err(err)?;
    perform_update(&app, &state, &agent, addr).await
}

#[derive(Debug, Clone, Serialize)]
pub struct BatchUpdateOutcome {
    pub agent_id: String,
    pub hostname: String,
    pub ok: bool,
    pub detail: String,
}

/// Updates every known poste that's outdated (same "outdated" test the
/// Postes screen uses: known, non-"unknown" `agent_version` that differs
/// from the bundled one), one after another — sequential, like
/// `discover_and_sync_all`, since a poste that never comes back otherwise
/// ties up this whole command for its full ~17s polling budget regardless.
/// A poste being skipped (revoked, no known IP, not actually outdated)
/// just doesn't appear in the result; one that was attempted and failed
/// does, with `ok: false` and why — same shape as `discover_and_sync_all`'s
/// `SyncOutcome`, so a failure for one poste never hides the others.
#[tauri::command]
pub async fn update_all_outdated_agents(app: tauri::AppHandle, state: State<'_, AppState>) -> CmdResult<Vec<BatchUpdateOutcome>> {
    let bundled_version =
        bundled_agent_version(app.clone()).ok_or_else(|| "aucune version de binaire embarquée à déployer".to_string())?;

    let agents = state.db.list_agents().map_err(err)?;
    let mut results = Vec::new();

    for agent in agents {
        if agent.revoked {
            continue;
        }
        let Some(last_ip) = &agent.last_ip else { continue };
        let is_outdated = agent
            .agent_version
            .as_deref()
            .is_some_and(|v| v != "unknown" && v != bundled_version);
        if !is_outdated {
            continue;
        }

        let Ok(addr) = format!("{last_ip}:7878").parse::<SocketAddr>() else {
            results.push(BatchUpdateOutcome {
                agent_id: agent.id,
                hostname: agent.hostname,
                ok: false,
                detail: "adresse IP invalide".to_string(),
            });
            continue;
        };

        results.push(match perform_update(&app, &state, &agent, addr).await {
            Ok(outcome) => {
                let (ok, detail) = describe_outcome(&outcome);
                BatchUpdateOutcome { agent_id: agent.id, hostname: agent.hostname, ok, detail }
            }
            Err(detail) => BatchUpdateOutcome { agent_id: agent.id, hostname: agent.hostname, ok: false, detail },
        });
    }

    Ok(results)
}

/// Polls `agent_id` at `addr` until it answers `/v1/info` again or the
/// attempt budget runs out — see `UPDATE_RESTART_GRACE`/`_POLL_*`.
async fn wait_for_agent_back_online(state: &AppState, agent_id: &str, addr: SocketAddr) -> Option<String> {
    tokio::time::sleep(UPDATE_RESTART_GRACE).await;
    for attempt in 0..UPDATE_POLL_ATTEMPTS {
        if let Ok(client) = connect_to_agent(
            &state.db,
            agent_id,
            addr,
            &state.client_identity.cert_pem,
            &state.client_identity.key_pem,
            state.ca.cert_der(),
        ) {
            if let Ok(info) = client.info().await {
                let _ = state.db.touch_agent_last_seen(agent_id);
                return Some(info.agent_version);
            }
        }
        if attempt + 1 < UPDATE_POLL_ATTEMPTS {
            tokio::time::sleep(UPDATE_POLL_INTERVAL).await;
        }
    }
    None
}

// --- groups --------------------------------------------------------------

#[tauri::command]
pub fn list_groups(state: State<'_, AppState>) -> CmdResult<Vec<Group>> {
    state.db.list_groups().map_err(err)
}

#[tauri::command]
pub fn create_group(state: State<'_, AppState>, name: String) -> CmdResult<String> {
    state.db.create_group(&name).map_err(err)
}

#[tauri::command]
pub fn delete_group(state: State<'_, AppState>, group_id: String) -> CmdResult<()> {
    state.db.delete_group(&group_id).map_err(err)
}

#[tauri::command]
pub fn add_group_member(state: State<'_, AppState>, group_id: String, agent_id: String) -> CmdResult<()> {
    state.db.add_group_member(&group_id, &agent_id).map_err(err)
}

#[tauri::command]
pub fn remove_group_member(state: State<'_, AppState>, group_id: String, agent_id: String) -> CmdResult<()> {
    state.db.remove_group_member(&group_id, &agent_id).map_err(err)
}

#[tauri::command]
pub fn agent_group_ids(state: State<'_, AppState>, agent_id: String) -> CmdResult<Vec<String>> {
    state.db.agent_group_ids(&agent_id).map_err(err)
}

// --- lists -----------------------------------------------------------------

#[tauri::command]
pub fn list_lists(state: State<'_, AppState>) -> CmdResult<Vec<List>> {
    state.db.list_lists().map_err(err)
}

#[tauri::command]
pub fn create_list(state: State<'_, AppState>, name: String, domains: Vec<String>) -> CmdResult<String> {
    state.db.create_list(&name, &domains).map_err(err)
}

#[tauri::command]
pub fn update_list_domains(state: State<'_, AppState>, list_id: String, domains: Vec<String>) -> CmdResult<()> {
    state.db.update_list_domains(&list_id, &domains).map_err(err)
}

#[tauri::command]
pub fn delete_list(state: State<'_, AppState>, list_id: String) -> CmdResult<()> {
    state.db.delete_list(&list_id).map_err(err)
}

// --- assignments & exceptions ------------------------------------------

#[tauri::command]
pub fn assignments_for(state: State<'_, AppState>, agent_id: String) -> CmdResult<Vec<Assignment>> {
    let group_ids = state.db.agent_group_ids(&agent_id).map_err(err)?;
    state.db.assignments_for(&agent_id, &group_ids).map_err(err)
}

#[tauri::command]
pub fn create_assignment(
    state: State<'_, AppState>,
    list_id: String,
    target_kind: String,
    target_id: String,
    schedule: Option<Vec<TimeWindow>>,
) -> CmdResult<String> {
    let kind = parse_target_kind(&target_kind)?;
    state
        .db
        .create_assignment(&list_id, kind, &target_id, schedule.as_deref())
        .map_err(err)
}

#[tauri::command]
pub fn delete_assignment(state: State<'_, AppState>, assignment_id: String) -> CmdResult<()> {
    state.db.delete_assignment(&assignment_id).map_err(err)
}

#[tauri::command]
pub fn exceptions_for(state: State<'_, AppState>, agent_id: String) -> CmdResult<Vec<Exception>> {
    let group_ids = state.db.agent_group_ids(&agent_id).map_err(err)?;
    state.db.exceptions_for(&agent_id, &group_ids).map_err(err)
}

#[tauri::command]
pub fn create_exception(
    state: State<'_, AppState>,
    target_kind: String,
    target_id: String,
    domains: Vec<String>,
    until: String,
) -> CmdResult<String> {
    let kind = parse_target_kind(&target_kind)?;
    let until: jiff::Timestamp = until.parse().map_err(err)?;
    state.db.create_exception(kind, &target_id, &domains, until).map_err(err)
}

#[tauri::command]
pub fn delete_exception(state: State<'_, AppState>, exception_id: String) -> CmdResult<()> {
    state.db.delete_exception(&exception_id).map_err(err)
}

fn parse_target_kind(s: &str) -> CmdResult<TargetKind> {
    match s {
        "agent" => Ok(TargetKind::Agent),
        "group" => Ok(TargetKind::Group),
        other => Err(format!("target_kind inconnu : {other}")),
    }
}

// --- history ---------------------------------------------------------------

#[tauri::command]
pub fn recent_history(state: State<'_, AppState>, limit: u32) -> CmdResult<Vec<HistoryEntry>> {
    state.db.recent_history(limit).map_err(err)
}

// --- dashboard -------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct UpcomingTransition {
    /// RFC 3339, in the policy's fixed timezone.
    pub at: String,
    pub list_name: String,
    /// Display label for the target: an agent's name, or "groupe <Nom>".
    pub target_label: String,
    /// `true` if the list starts being blocked at `at`; `false` if it
    /// stops.
    pub becomes_blocked: bool,
}

/// Every upcoming schedule transition across the whole fleet — assignments
/// with no schedule ("toujours") don't produce any, since they never
/// change. Used by the dashboard's "Prochaines transitions" panel.
#[tauri::command]
pub fn upcoming_transitions(state: State<'_, AppState>, limit: usize) -> CmdResult<Vec<UpcomingTransition>> {
    let timezone = state.timezone();
    let now = jiff::Timestamp::now().in_tz(&timezone).map_err(err)?;

    let assignments = state.db.list_assignments().map_err(err)?;
    let lists = state.db.list_lists().map_err(err)?;
    let groups = state.db.list_groups().map_err(err)?;
    let agents = state.db.list_agents().map_err(err)?;

    let mut results: Vec<UpcomingTransition> = Vec::new();
    for assignment in &assignments {
        let Some(schedule) = &assignment.schedule else {
            continue;
        };
        let list_name = lists
            .iter()
            .find(|l| l.id == assignment.list_id)
            .map(|l| l.name.clone())
            .unwrap_or_else(|| assignment.list_id.clone());
        let target_label = match assignment.target_kind {
            TargetKind::Agent => agents
                .iter()
                .find(|a| a.id == assignment.target_id)
                .map(|a| a.display_name.clone().unwrap_or_else(|| a.hostname.clone()))
                .unwrap_or_else(|| assignment.target_id.clone()),
            TargetKind::Group => groups
                .iter()
                .find(|g| g.id == assignment.target_id)
                .map(|g| format!("groupe {}", g.name))
                .unwrap_or_else(|| assignment.target_id.clone()),
        };

        for window in schedule {
            if let Some(next) = window.next_transition(&now) {
                results.push(UpcomingTransition {
                    at: next.timestamp().to_string(),
                    list_name: list_name.clone(),
                    target_label: target_label.clone(),
                    becomes_blocked: window.contains(&next),
                });
            }
        }
    }

    results.sort_by(|a, b| a.at.cmp(&b.at));
    results.truncate(limit);
    Ok(results)
}

// --- enrollment --------------------------------------------------------

/// Fixed ports (see PLAN.md section 4/5): predictable enough to put in a
/// firewall rule, and side-steps a chicken-and-egg problem — the deploy
/// server's rendered scripts need to embed its own address, which is only
/// known once it's actually bound, but binding happens inside `serve()`.
/// An ephemeral port would mean constructing the `DeployServer` (which
/// needs that address) before the bind that produces it.
const ENROLLMENT_PORT: u16 = 7879;
const DEPLOY_PORT: u16 = 7880;

#[derive(Debug, Clone, Serialize)]
pub struct EnrollmentInfo {
    pub token: String,
    pub app_ip: String,
    pub enrollment_port: u16,
    pub deploy_port: u16,
    pub ca_fingerprint: String,
    pub expires_in_secs: i64,
    /// Whether at least one platform's agent binary is embedded — if not,
    /// the one-line install scripts are servable but their download step
    /// will fail (by design: see `DeployServer::render`), so the caller
    /// should fall back to showing the manual `enroll` command instead.
    pub binaries_available: bool,
}

/// Starts (or reuses) the temporary enrollment + deploy servers and
/// issues a fresh token. Safe to call repeatedly while the "Ajouter des
/// postes" dialog is open — each call gets its own token, all valid until
/// `stop_enrollment` tears both servers down.
#[tauri::command]
pub async fn start_enrollment(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    ttl_secs: i64,
    default_group_id: Option<String>,
) -> CmdResult<EnrollmentInfo> {
    let ip = crate::net::local_lan_ip().map_err(err)?;
    let ttl = jiff::SignedDuration::from_secs(ttl_secs);

    // Fast path: reuse an already-running session. Deliberately doesn't
    // hold the lock across the `.await`s below (a std Mutex guard isn't
    // `Send`, and holding a blocking lock across an await is bad practice
    // even when it would compile).
    let existing = {
        let guard = state.enrollment.lock().unwrap();
        guard.as_ref().map(|s| s.enrollment_state.clone())
    };

    let (enrollment_state, binaries_available) = match existing {
        Some(enrollment_state) => (enrollment_state, true), // already validated below on first start
        None => {
            let enrollment_state = Arc::new(EnrollmentState::new(state.ca.clone(), state.db.clone(), state.timezone()));
            let enrollment_addr = SocketAddr::new(ip, ENROLLMENT_PORT);
            let enrollment_handle = axum_server::Handle::new();
            {
                let serve_state = enrollment_state.clone();
                let serve_handle = enrollment_handle.clone();
                tokio::spawn(async move {
                    if let Err(e) = homeguard_app_core::enrollment_server::serve(enrollment_addr, serve_state, serve_handle).await {
                        tracing::error!("le serveur d'enrôlement s'est arrêté en erreur : {e}");
                    }
                });
            }
            let enrollment_listening = enrollment_handle
                .listening()
                .await
                .ok_or_else(|| format!("le serveur d'enrôlement n'a pas pu écouter sur le port {ENROLLMENT_PORT}"))?;

            let binary_paths = discover_agent_binaries(&app);
            let binaries_available = !binary_paths.is_empty();
            let deploy_server = Arc::new(
                homeguard_app_core::deploy_server::DeployServer::new(
                    format!("http://{ip}:{DEPLOY_PORT}"),
                    format!("https://{enrollment_listening}/v1/enroll"),
                    state.ca.fingerprint_sha256(),
                    binary_paths,
                )
                .map_err(err)?,
            );
            let deploy_addr = SocketAddr::new(ip, DEPLOY_PORT);
            let deploy_handle = axum_server::Handle::new();
            {
                let serve_handle = deploy_handle.clone();
                tokio::spawn(async move {
                    if let Err(e) = homeguard_app_core::deploy_server::serve(deploy_addr, deploy_server, serve_handle).await {
                        tracing::error!("le serveur de déploiement s'est arrêté en erreur : {e}");
                    }
                });
            }
            // Only used to confirm the bind actually succeeded — its
            // address is already fixed (`DEPLOY_PORT`) and embedded in the
            // `DeployServer` above.
            let _deploy_listening = deploy_handle
                .listening()
                .await
                .ok_or_else(|| format!("le serveur de déploiement n'a pas pu écouter sur le port {DEPLOY_PORT}"))?;

            // Another concurrent call may have installed a session while
            // we were awaiting the binds above — if so, defer to it and
            // tear ours down instead of leaking two extra servers.
            let mut guard = state.enrollment.lock().unwrap();
            if let Some(session) = guard.as_ref() {
                let winner = session.enrollment_state.clone();
                drop(guard);
                enrollment_handle.shutdown();
                deploy_handle.shutdown();
                (winner, true)
            } else {
                *guard = Some(EnrollmentSession {
                    handle: enrollment_handle,
                    enrollment_state: enrollment_state.clone(),
                    deploy_handle,
                });
                (enrollment_state, binaries_available)
            }
        }
    };

    let token = enrollment_state.issue_token(ttl, default_group_id);

    Ok(EnrollmentInfo {
        token,
        app_ip: ip.to_string(),
        enrollment_port: ENROLLMENT_PORT,
        deploy_port: DEPLOY_PORT,
        ca_fingerprint: state.ca.fingerprint_sha256(),
        expires_in_secs: ttl_secs,
        binaries_available,
    })
}

/// Looks for compiled agent binaries under `<resource_dir>/agents/`,
/// named exactly `homeguard-agent-linux-x86_64`,
/// `homeguard-agent-linux-aarch64` and
/// `homeguard-agent-windows-x86_64.exe` — staged there by
/// `scripts/build-agents.sh` before the app is bundled (see that script
/// and `tauri.conf.json`'s `bundle.resources`). Empty map if none are
/// staged (a fresh checkout before that script has run, or a bundle built
/// without it).
fn discover_agent_binaries(
    app: &tauri::AppHandle,
) -> std::collections::HashMap<homeguard_app_core::deploy_server::AgentTarget, std::path::PathBuf> {
    use homeguard_app_core::deploy_server::AgentTarget;
    use tauri::Manager;

    let mut found = std::collections::HashMap::new();
    let Ok(resource_dir) = app.path().resource_dir() else {
        return found;
    };
    let agents_dir = resource_dir.join("agents");
    for (target, filename) in [
        (AgentTarget::LinuxX86_64, "homeguard-agent-linux-x86_64"),
        (AgentTarget::LinuxArm64, "homeguard-agent-linux-aarch64"),
        (AgentTarget::Windows, "homeguard-agent-windows-x86_64.exe"),
    ] {
        let path = agents_dir.join(filename);
        if path.is_file() {
            found.insert(target, path);
        }
    }
    found
}

/// The version staged in `resources/agents/version.txt` by
/// `scripts/build-agents.sh` at the last cross-compile — what a poste
/// would be updated *to*, if it's behind. `None` when no version file is
/// staged (same "nothing bundled yet" case as `discover_agent_binaries`),
/// which the UI should treat as "can't tell, don't claim anyone's
/// outdated" rather than as an update being available.
#[tauri::command]
pub fn bundled_agent_version(app: tauri::AppHandle) -> Option<String> {
    use tauri::Manager;

    let resource_dir = app.path().resource_dir().ok()?;
    let raw = std::fs::read_to_string(resource_dir.join("agents").join("version.txt")).ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[tauri::command]
pub fn stop_enrollment(state: State<'_, AppState>) -> CmdResult<()> {
    let mut guard = state.enrollment.lock().unwrap();
    if let Some(session) = guard.take() {
        session.handle.shutdown();
        session.deploy_handle.shutdown();
    }
    Ok(())
}

// --- misc ----------------------------------------------------------------

#[tauri::command]
pub fn ca_fingerprint(state: State<'_, AppState>) -> String {
    state.ca.fingerprint_sha256()
}

#[tauri::command]
pub fn get_timezone(state: State<'_, AppState>) -> String {
    state.timezone()
}

#[tauri::command]
pub fn set_timezone(state: State<'_, AppState>, timezone: String) -> CmdResult<()> {
    state.db.set_setting("timezone", &timezone).map_err(err)
}

// --- export / import --------------------------------------------------

/// Writes the full configuration (every poste, group, list, assignment,
/// exception, setting — and the CA) to `path`, encrypted with `passphrase`.
/// Anyone with the file *and* the passphrase can impersonate this app to
/// every agent it manages, so treat it like the credential it is.
#[tauri::command]
pub fn export_config(state: State<'_, AppState>, path: String, passphrase: String) -> CmdResult<()> {
    let config = homeguard_app_core::export::build_export(&state.db, &state.ca).map_err(err)?;
    let armored = homeguard_app_core::export::encrypt_export(&config, &passphrase).map_err(err)?;
    std::fs::write(&path, armored).map_err(err)?;
    let _ = state.db.append_history("app", None, "config_exported", "configuration exportée");
    Ok(())
}

/// Decrypts `path` and merges its configuration into the current database
/// (every row upserted by its original id — safe to import the same file
/// more than once). The CA it carries is written to storage for the
/// *next* launch to pick up; the running app keeps its current identity
/// until restarted, rather than hot-swapping a live TLS identity out from
/// under in-flight connections.
#[tauri::command]
pub async fn import_config(app: tauri::AppHandle, state: State<'_, AppState>, path: String, passphrase: String) -> CmdResult<bool> {
    let armored = std::fs::read_to_string(&path).map_err(err)?;
    let config = homeguard_app_core::export::decrypt_export(&armored, &passphrase).map_err(err)?;
    let imported_ca = homeguard_app_core::export::ca_from_export(&config).map_err(err)?;

    let restart_needed = imported_ca.fingerprint_sha256() != state.ca.fingerprint_sha256();
    if restart_needed {
        use tauri::Manager;
        let data_dir = app.path().app_data_dir().map_err(err)?;
        crate::state::store_ca(&data_dir, &imported_ca).map_err(err)?;
    }

    homeguard_app_core::export::apply_import(&state.db, &config).map_err(err)?;
    let _ = state
        .db
        .append_history("app", None, "config_imported", "configuration importée");

    Ok(restart_needed)
}
