use std::collections::BTreeSet;
use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use serde::Deserialize;
use homeguard_common::policy::Policy;
use homeguard_common::wire::{AgentInfo, AgentStatus, EventsPage};

use crate::hosts_writer;
use crate::state::{hosts_block_hash, AppState};

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/v1/info", get(get_info))
        .route("/v1/status", get(get_status))
        .route("/v1/policy", put(put_policy))
        .route("/v1/events", get(get_events))
        .route("/v1/update", post(post_update))
        .route("/v1/uninstall", post(post_uninstall))
        .with_state(state)
}

async fn get_info(State(state): State<Arc<AppState>>) -> Json<AgentInfo> {
    Json(AgentInfo {
        agent_id: state.agent_id.clone(),
        hostname: state.hostname.clone(),
        os: state.os.clone(),
        arch: state.arch.clone(),
        agent_version: state.agent_version.clone(),
        uptime_secs: state.uptime_secs(),
    })
}

async fn get_status(State(state): State<Arc<AppState>>) -> Json<AgentStatus> {
    let policy = state.current_policy();
    let now = jiff::Timestamp::now();
    let desired = policy
        .effective_domains_at(now)
        .unwrap_or_else(|_| BTreeSet::new());

    let current_on_disk: BTreeSet<String> = hosts_writer::read_current_domains(&state.hosts_path)
        .ok()
        .flatten()
        .unwrap_or_default()
        .into_iter()
        .collect();

    Json(AgentStatus {
        applied_policy_version: policy.version,
        domains_blocked_now: desired.len() as u64,
        // Must reflect what's actually on disk, not what the policy says
        // it should be — the app already knows the latter (it built the
        // policy) and relies on this hash to notice when the two diverge.
        hosts_block_hash: hosts_block_hash(&current_on_disk),
        integrity_ok: current_on_disk == desired,
        self_heal_enabled: policy.protection.self_heal,
        disable_doh_enabled: policy.protection.disable_doh,
    })
}

async fn put_policy(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(policy): Json<Policy>,
) -> Response {
    if let Some(if_match) = headers.get("if-match").and_then(|v| v.to_str().ok()) {
        let current_version = state.current_policy().version;
        let expected: Option<u64> = if_match.trim().parse().ok();
        if expected != Some(current_version) {
            return StatusCode::CONFLICT.into_response();
        }
    }

    match state.set_policy(policy) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => {
            tracing::error!("échec de l'enregistrement de la politique : {e}");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

#[derive(Debug, Deserialize)]
struct EventsQuery {
    #[serde(default)]
    since: u64,
}

async fn get_events(
    State(state): State<Arc<AppState>>,
    Query(query): Query<EventsQuery>,
) -> Response {
    match state.events.since(query.since) {
        Ok((events, next_seq)) => Json(EventsPage { events, next_seq }).into_response(),
        Err(e) => {
            tracing::error!("échec de la lecture du journal d'événements : {e}");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

async fn post_uninstall(State(state): State<Arc<AppState>>) -> StatusCode {
    crate::service::trigger_self_uninstall(&state);
    StatusCode::ACCEPTED
}

/// Body is the raw new agent binary; the expected SHA-256 (hex) comes in
/// the `x-sha256` header so the agent never applies a truncated or
/// tampered download.
async fn post_update(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let Some(expected_sha256) = headers.get("x-sha256").and_then(|v| v.to_str().ok()) else {
        return (StatusCode::BAD_REQUEST, "missing x-sha256 header").into_response();
    };

    match crate::update::apply_update(&body, expected_sha256) {
        Ok(()) => {
            state
                .events
                .append(
                    homeguard_common::wire::EventKind::PolicyApplied,
                    "agent mis à jour, redémarrage en cours",
                )
                .ok();
            // Let the response reach the caller before this process exits;
            // the service manager restarts it running the new binary.
            tokio::spawn(async {
                tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                std::process::exit(0);
            });
            StatusCode::ACCEPTED.into_response()
        }
        Err(e) => {
            tracing::error!("échec de la mise à jour de l'agent : {e}");
            (StatusCode::BAD_REQUEST, e.to_string()).into_response()
        }
    }
}
