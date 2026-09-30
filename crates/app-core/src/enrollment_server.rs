//! The app's temporary "Ajouter des postes" server: agents POST here with
//! a CSR + a short-lived token, and get back a certificate signed by our
//! CA plus their first policy. Torn down as soon as the admin closes the
//! dialog or the token expires — nothing about enrollment stays listening
//! permanently.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use axum::extract::{ConnectInfo, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use rand::distr::{Alphanumeric, SampleString};
use homeguard_common::wire::{EnrollRequest, EnrollResponse};
use homeguard_common::Ca;

use crate::client::stable_agent_name;
use crate::db::Db;
use crate::policy_engine;

#[derive(Debug, Clone)]
pub struct EnrollmentToken {
    pub token: String,
    pub expires_at: jiff::Timestamp,
    pub default_group_id: Option<String>,
}

pub struct EnrollmentState {
    pub ca: Arc<Ca>,
    pub db: Arc<Db>,
    pub tokens: Mutex<Vec<EnrollmentToken>>,
    pub timezone: String,
    pub default_self_heal: bool,
    pub default_disable_doh: bool,
}

impl EnrollmentState {
    pub fn new(ca: Arc<Ca>, db: Arc<Db>, timezone: impl Into<String>) -> Self {
        EnrollmentState {
            ca,
            db,
            tokens: Mutex::new(Vec::new()),
            timezone: timezone.into(),
            default_self_heal: true,
            default_disable_doh: true,
        }
    }

    /// Issue a fresh token valid for `ttl`, usable by any number of agents
    /// until it expires (an admin enrolling several machines shouldn't
    /// have to copy a new command for each one).
    pub fn issue_token(&self, ttl: jiff::SignedDuration, default_group_id: Option<String>) -> String {
        let token = format!(
            "{}-{}",
            Alphanumeric.sample_string(&mut rand::rng(), 4).to_ascii_uppercase(),
            Alphanumeric.sample_string(&mut rand::rng(), 4).to_ascii_uppercase()
        );
        let mut tokens = self.tokens.lock().unwrap();
        tokens.retain(|t| t.expires_at > jiff::Timestamp::now());
        tokens.push(EnrollmentToken {
            token: token.clone(),
            expires_at: jiff::Timestamp::now() + ttl,
            default_group_id,
        });
        token
    }

    fn check_token(&self, token: &str) -> Option<EnrollmentToken> {
        let tokens = self.tokens.lock().unwrap();
        tokens
            .iter()
            .find(|t| t.token == token && t.expires_at > jiff::Timestamp::now())
            .cloned()
    }
}

pub fn router(state: Arc<EnrollmentState>) -> Router {
    Router::new().route("/v1/enroll", post(enroll_handler)).with_state(state)
}

async fn enroll_handler(
    State(state): State<Arc<EnrollmentState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Json(req): Json<EnrollRequest>,
) -> Response {
    let Some(token) = state.check_token(&req.token) else {
        return (StatusCode::FORBIDDEN, "jeton invalide ou expiré").into_response();
    };

    let peer_ip = peer.ip().to_string();
    // Deliberately no IP in the SAN: the app always connects by this
    // stable name (see `AgentClient::new`), resolving it to whatever the
    // agent's current address happens to be. That's what lets the same
    // 2-year certificate survive a DHCP lease change without reissuing.
    let names = vec![req.hostname.clone(), stable_agent_name(&req.hostname)];

    let issued = match state.ca.sign_agent_csr(&req.csr_pem, &names) {
        Ok(issued) => issued,
        Err(e) => {
            tracing::error!("échec de la signature du certificat pour {}: {e}", req.hostname);
            return (StatusCode::BAD_REQUEST, "CSR invalide").into_response();
        }
    };

    let agent_id = uuid::Uuid::new_v4().to_string();
    if let Err(e) = state
        .db
        .upsert_agent(&agent_id, &req.hostname, &req.os, &req.arch, Some(&peer_ip), Some(&req.agent_version))
    {
        tracing::error!("échec de l'enregistrement de l'agent {}: {e}", req.hostname);
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    if let Err(e) = state.db.set_agent_cert_serial(&agent_id, &issued.serial_hex) {
        tracing::error!("échec de l'enregistrement du numéro de série pour {}: {e}", req.hostname);
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    if let Some(group_id) = &token.default_group_id {
        let _ = state.db.add_group_member(group_id, &agent_id);
    }
    let _ = state
        .db
        .append_history("app", Some(&agent_id), "enrolled", &format!("{} enrôlé depuis {peer_ip}", req.hostname));

    let policy = match policy_engine::resolve_initial_policy_for_agent(
        &state.db,
        &agent_id,
        &state.timezone,
        state.default_self_heal,
        state.default_disable_doh,
    ) {
        Ok(policy) => policy,
        Err(e) => {
            tracing::error!("échec du calcul de la politique initiale pour {}: {e}", req.hostname);
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    Json(EnrollResponse {
        cert_pem: issued.cert_pem,
        ca_cert_pem: state.ca.cert_pem().to_string(),
        agent_id,
        initial_policy: policy,
    })
    .into_response()
}

/// Serve the enrollment router on `addr` until `handle.shutdown()` is
/// called elsewhere (e.g. when the admin closes the "Ajouter des postes"
/// dialog, or the last token expires).
pub async fn serve(
    addr: SocketAddr,
    state: Arc<EnrollmentState>,
    handle: axum_server::Handle,
) -> std::io::Result<()> {
    let server_identity = state
        .ca
        .issue_server_identity(&[addr.ip().to_string()])
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    let chain_pem = format!("{}\n{}", server_identity.cert_pem, state.ca.cert_pem());
    let tls_config = homeguard_common::server_config_tls_no_client_auth(&chain_pem, &server_identity.key_pem)
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    let rustls_config = axum_server::tls_rustls::RustlsConfig::from_config(tls_config);

    let router = router(state);
    axum_server::bind_rustls(addr, rustls_config)
        .handle(handle)
        .serve(router.into_make_service_with_connect_info::<SocketAddr>())
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_expire_and_are_pruned_on_next_issue() {
        let ca = Arc::new(Ca::generate().unwrap());
        let db = Arc::new(Db::open_in_memory().unwrap());
        let state = EnrollmentState::new(ca, db, "UTC");

        let expired = state.issue_token(jiff::SignedDuration::from_secs(0), None);
        std::thread::sleep(std::time::Duration::from_millis(5));
        assert!(state.check_token(&expired).is_none());

        let valid = state.issue_token(jiff::SignedDuration::from_secs(1800), None);
        assert!(state.check_token(&valid).is_some());
        assert!(state.check_token("not-a-real-token").is_none());
    }
}
