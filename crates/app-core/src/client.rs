//! mTLS client for talking to one already-enrolled agent. The app is
//! always the one initiating — agents never call out.

use std::net::SocketAddr;
use std::time::Duration;

use rustls_pki_types::CertificateDer;
use homeguard_common::policy::Policy;
use homeguard_common::wire::{AgentInfo, AgentStatus, EventsPage};

use crate::db::{Db, DbError};

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error(transparent)]
    Tls(#[from] homeguard_common::TlsError),
    #[error(transparent)]
    Request(#[from] reqwest::Error),
    /// The app's cached policy version is stale — someone else (or a
    /// previous run) already pushed a newer one. Caller should re-read
    /// `/v1/status` and retry.
    #[error("policy version conflict — the agent is already on a different version")]
    VersionConflict,
    #[error("unexpected response status {0}")]
    UnexpectedStatus(reqwest::StatusCode),
    /// This agent's certificate was revoked (removed poste, suspected
    /// compromise) — refused *before* opening a connection to it. There is
    /// no CRL/OCSP in this system, so this app-level check is the only
    /// thing standing between "revoked" and "still fully trusted".
    #[error("agent {0} is revoked — refusing to connect")]
    AgentRevoked(String),
    #[error(transparent)]
    Db(#[from] DbError),
}

/// What went wrong at the TLS layer, when a request failed there — see
/// [`ClientError::tls_failure`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TlsFailure {
    /// The handshake ran and one side refused the other: the agent's
    /// certificate isn't signed by this app's CA, or the agent refused this
    /// app's client certificate (it answers with a fatal alert). In
    /// practice: a poste still running an identity from another
    /// enrollment.
    Rejected,
    /// Whatever answered doesn't speak TLS at all (plain HTTP, another
    /// service on that port).
    NotTls,
}

impl ClientError {
    /// Classifies a failed request by the actual `rustls::Error` it carries
    /// — never by the error's text, which also contains the URL (and so the
    /// poste's name). `None` when the failure isn't a TLS one (refused,
    /// reset, timed out...).
    pub fn tls_failure(&self) -> Option<TlsFailure> {
        let ClientError::Request(e) = self else {
            return None;
        };
        find_rustls_error(e).map(|tls| match tls {
            rustls::Error::InvalidMessage(_)
            | rustls::Error::InappropriateMessage { .. }
            | rustls::Error::InappropriateHandshakeMessage { .. } => TlsFailure::NotTls,
            _ => TlsFailure::Rejected,
        })
    }
}

/// Walks an error's source chain looking for a `rustls::Error`. hyper and
/// tokio-rustls wrap it in (possibly nested) `std::io::Error`s, whose
/// `source()` skips the wrapped error itself, so each one's inner error
/// has to be inspected explicitly.
fn find_rustls_error<'a>(e: &'a (dyn std::error::Error + 'static)) -> Option<&'a rustls::Error> {
    let mut current = Some(e);
    while let Some(err) = current {
        if let Some(tls) = err.downcast_ref::<rustls::Error>() {
            return Some(tls);
        }
        if let Some(inner) = err.downcast_ref::<std::io::Error>().and_then(|io| io.get_ref()) {
            if let Some(tls) = find_rustls_error(inner) {
                return Some(tls);
            }
        }
        current = err.source();
    }
    None
}

/// The stable name an agent's certificate is issued for (see
/// `enrollment_server::enroll_handler`), independent of whatever IP it
/// happens to have right now. Connecting always goes through this name —
/// see [`AgentClient::new`] — so a DHCP lease change doesn't invalidate
/// the 2-year certificate the way pinning the SAN to an IP would.
///
/// Idempotent: on some systems the OS hostname itself already ends in
/// `.local` (some Linux mDNS/Avahi setups report it that way), and mDNS
/// discovery's own `hostname` field always has it appended too (see
/// `discovery::to_discovered_agent`) — appending it unconditionally would
/// double it into `foo.local.local`, which then simply fails to connect
/// (nothing resolves that name to anything).
pub fn stable_agent_name(hostname: &str) -> String {
    if hostname.to_ascii_lowercase().ends_with(".local") {
        hostname.to_string()
    } else {
        format!("{hostname}.local")
    }
}

/// Build a client for `agent_id`, refusing to connect if the app has
/// marked it revoked. Prefer this over [`AgentClient::new`] directly
/// whenever an `agent_id` (and therefore a DB record) is available — it
/// also supplies the stable hostname `AgentClient::new` needs from the
/// agent's own record, so the caller doesn't have to track it separately.
pub fn connect_to_agent(
    db: &Db,
    agent_id: &str,
    addr: SocketAddr,
    client_cert_pem: &str,
    client_key_pem: &str,
    ca_cert_der: &CertificateDer<'static>,
) -> Result<AgentClient, ClientError> {
    let agent = db.get_agent(agent_id)?.ok_or_else(|| ClientError::AgentRevoked(agent_id.to_string()))?;
    if agent.revoked {
        return Err(ClientError::AgentRevoked(agent_id.to_string()));
    }
    AgentClient::new(&agent.hostname, addr, client_cert_pem, client_key_pem, ca_cert_der)
}

pub struct AgentClient {
    http: reqwest::Client,
    base_url: String,
}

impl AgentClient {
    /// `hostname` is the agent's plain hostname (e.g. `"PC-SALON"`, as
    /// stored in the DB / reported at enrollment) — its certificate's SAN
    /// covers `{hostname}.local`, which is what's actually used for TLS
    /// verification. `addr` is only ever used to dial the TCP connection
    /// (via a per-client DNS override), never to decide whether the
    /// certificate is trusted, so a changed IP doesn't break anything: the
    /// next scan just supplies a fresh `addr` for the same stable name.
    pub fn new(
        hostname: &str,
        addr: SocketAddr,
        client_cert_pem: &str,
        client_key_pem: &str,
        ca_cert_der: &CertificateDer<'static>,
    ) -> Result<Self, ClientError> {
        homeguard_common::install_crypto_provider();
        let tls_config = homeguard_common::client_config_mtls(client_cert_pem, client_key_pem, ca_cert_der)?;
        let server_name = stable_agent_name(hostname);
        let http = reqwest::Client::builder()
            .use_preconfigured_tls((*tls_config).clone())
            .resolve(&server_name, addr)
            .timeout(Duration::from_secs(5))
            .build()?;
        Ok(AgentClient {
            http,
            base_url: format!("https://{server_name}:{}", addr.port()),
        })
    }

    pub async fn info(&self) -> Result<AgentInfo, ClientError> {
        Ok(self.http.get(format!("{}/v1/info", self.base_url)).send().await?.json().await?)
    }

    pub async fn status(&self) -> Result<AgentStatus, ClientError> {
        Ok(self.http.get(format!("{}/v1/status", self.base_url)).send().await?.json().await?)
    }

    /// `expected_current_version`, when set, is sent as `If-Match` so the
    /// agent rejects the push (409) if it's not actually on that version
    /// — guards against clobbering a change applied since the app last
    /// read this agent's status.
    pub async fn push_policy(&self, policy: &Policy, expected_current_version: Option<u64>) -> Result<(), ClientError> {
        let mut request = self.http.put(format!("{}/v1/policy", self.base_url)).json(policy);
        if let Some(version) = expected_current_version {
            request = request.header("if-match", version.to_string());
        }
        let response = request.send().await?;
        match response.status() {
            reqwest::StatusCode::NO_CONTENT => Ok(()),
            reqwest::StatusCode::CONFLICT => Err(ClientError::VersionConflict),
            other => Err(ClientError::UnexpectedStatus(other)),
        }
    }

    pub async fn events_since(&self, since: u64) -> Result<EventsPage, ClientError> {
        Ok(self
            .http
            .get(format!("{}/v1/events?since={since}", self.base_url))
            .send()
            .await?
            .json()
            .await?)
    }

    pub async fn uninstall(&self) -> Result<(), ClientError> {
        let response = self.http.post(format!("{}/v1/uninstall", self.base_url)).send().await?;
        if response.status().is_success() {
            Ok(())
        } else {
            Err(ClientError::UnexpectedStatus(response.status()))
        }
    }

    pub async fn push_update(&self, new_binary: &[u8], expected_sha256_hex: &str) -> Result<(), ClientError> {
        let response = self
            .http
            .post(format!("{}/v1/update", self.base_url))
            .header("x-sha256", expected_sha256_hex)
            .body(new_binary.to_vec())
            .send()
            .await?;
        if response.status().is_success() {
            Ok(())
        } else {
            Err(ClientError::UnexpectedStatus(response.status()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_agent_name_appends_dot_local_once() {
        assert_eq!(stable_agent_name("PC-SALON"), "PC-SALON.local");
    }

    #[test]
    fn stable_agent_name_does_not_double_an_already_present_dot_local() {
        // Some Linux mDNS/Avahi setups report the OS hostname itself as
        // already ending in `.local` — appending it again produced
        // `foo.local.local`, which then simply failed to connect.
        assert_eq!(stable_agent_name("laurent-ThinkPad-L460.local"), "laurent-ThinkPad-L460.local");
        assert_eq!(stable_agent_name("foo.LOCAL"), "foo.LOCAL");
    }

    #[test]
    fn connect_to_agent_refuses_a_revoked_agent_before_touching_the_network() {
        let db = Db::open_in_memory().unwrap();
        db.upsert_agent("a1", "PC-SALON", "windows", "x86_64", None, None).unwrap();
        db.revoke_agent("a1").unwrap();

        let ca = homeguard_common::Ca::generate().unwrap();
        // Deliberately garbage cert/key PEMs: if the revocation check did
        // not short-circuit first, this would fail differently (a TLS
        // config error), which the assertion below would catch.
        let result = connect_to_agent(
            &db,
            "a1",
            "127.0.0.1:7878".parse().unwrap(),
            "not a real cert",
            "not a real key",
            ca.cert_der(),
        );

        assert!(matches!(result, Err(ClientError::AgentRevoked(id)) if id == "a1"));
    }

    #[test]
    fn connect_to_agent_proceeds_normally_for_a_non_revoked_agent() {
        let db = Db::open_in_memory().unwrap();
        db.upsert_agent("a1", "PC-SALON", "windows", "x86_64", None, None).unwrap();

        let ca = homeguard_common::Ca::generate().unwrap();
        let identity = ca.issue_client_identity("test-app").unwrap();
        let result = connect_to_agent(
            &db,
            "a1",
            "127.0.0.1:7878".parse().unwrap(),
            &identity.cert_pem,
            &identity.key_pem,
            ca.cert_der(),
        );

        assert!(result.is_ok());
    }
}
