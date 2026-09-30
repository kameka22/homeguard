//! The agent's actual server loop: load identity + policy, start the
//! scheduler, announce over mDNS, and serve the mTLS API until told to stop.
//!
//! Lives in the library (not `main.rs`) so both the normal foreground/systemd
//! path and the Windows service dispatcher (`service.rs`) can drive it — the
//! only difference between them is *where the shutdown signal comes from*,
//! which is why `external_shutdown` exists: the Windows SCM delivers a
//! distinct control code for "stop this service" vs. "the machine is
//! shutting down" on its own control-handler thread, so there's no console
//! to send Ctrl+C/SIGTERM to and no OS signal to listen for.

use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::Instant;

use anyhow::Context;

use crate::config::{AgentConfig, AgentPaths};
use crate::events::EventLog;
use crate::identity::AgentIdentity;
use crate::shutdown::ShutdownCause;
use crate::state::{run_scheduler, AppState};
use crate::{api, enroll, mdns_announce, shutdown};
use homeguard_common::wire::EventKind;

/// The parsed `run` command's arguments, captured once so the Windows
/// service dispatcher (which re-enters on a background thread with no
/// argv of its own to parse) can reuse exactly what the CLI parsed.
#[derive(Clone)]
pub struct RunArgs {
    pub data_dir: Option<PathBuf>,
    pub hosts_path: Option<PathBuf>,
    pub port: u16,
    pub mdns_enabled: bool,
}

pub async fn run(
    args: RunArgs,
    external_shutdown: Option<tokio::sync::oneshot::Receiver<ShutdownCause>>,
) -> anyhow::Result<()> {
    let paths = args.data_dir.map(AgentPaths::new).unwrap_or_else(AgentPaths::default_for_os);
    let mut config = AgentConfig::production(paths.clone());
    config.listen_addr = std::net::SocketAddr::from(([0, 0, 0, 0], args.port));
    config.mdns_enabled = args.mdns_enabled;
    if let Some(hosts_path) = args.hosts_path {
        config.hosts_path = hosts_path;
    }

    homeguard_common::install_crypto_provider();

    let identity = AgentIdentity::load(&paths)?;
    let policy = match std::fs::read_to_string(paths.policy_path()) {
        Ok(json) => serde_json::from_str(&json)?,
        // No policy pushed yet (fresh enrollment writes one, but be
        // defensive): start from "block nothing" rather than fail.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => empty_policy(),
        // Any other error (permissions, disk, transient failure) must NOT
        // silently fail open — better to refuse to start (and let the
        // service manager retry) than to run with zero domains blocked.
        Err(e) => return Err(e).context("échec de la lecture de policy.json"),
    };

    let events = Arc::new(EventLog::open(paths.events_path())?);
    let state = Arc::new(AppState {
        agent_id: identity.agent_id.clone(),
        hostname: enroll::local_hostname(),
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        agent_version: env!("CARGO_PKG_VERSION").to_string(),
        started_at: Instant::now(),
        hosts_path: config.hosts_path.clone(),
        policy_path: paths.policy_path(),
        etc_root: PathBuf::from("/etc"),
        events,
        policy: RwLock::new(policy),
        last_applied: RwLock::new(Default::default()),
        policy_changed: tokio::sync::Notify::new(),
    });

    state
        .events
        .append(EventKind::AgentStarted, format!("agent {} démarré", env!("CARGO_PKG_VERSION")))
        .ok();

    tokio::spawn(run_scheduler(state.clone()));

    let _mdns_guard = if config.mdns_enabled {
        let fingerprint = homeguard_common::pki::fingerprint_sha256(&identity.ca_cert_der);
        match mdns_announce::announce(
            &state.agent_id,
            &state.hostname,
            &state.os,
            &state.arch,
            &state.agent_version,
            config.listen_addr.port(),
            &fingerprint,
        ) {
            Ok(daemon) => Some(daemon),
            Err(e) => {
                tracing::warn!("annonce mDNS impossible : {e}");
                None
            }
        }
    } else {
        None
    };

    let tls_config = identity.server_tls_config()?;
    let rustls_config = axum_server::tls_rustls::RustlsConfig::from_config(tls_config);

    let handle = axum_server::Handle::new();
    let shutdown_handle = handle.clone();
    let shutdown_events = state.events.clone();
    tokio::spawn(async move {
        let cause = match external_shutdown {
            Some(rx) => shutdown::wait_for_external_signal(rx).await,
            None => shutdown::wait_for_shutdown_signal().await,
        };
        shutdown::log_shutdown(&shutdown_events, cause);
        // Give in-flight requests (e.g. the app mid-`/v1/status`) a moment
        // to finish instead of dropping their connections outright.
        shutdown_handle.graceful_shutdown(Some(std::time::Duration::from_secs(5)));
    });

    tracing::info!("écoute mTLS sur {}", config.listen_addr);
    axum_server::bind_rustls(config.listen_addr, rustls_config)
        .handle(handle)
        .serve(api::router(state).into_make_service())
        .await?;

    Ok(())
}

fn empty_policy() -> homeguard_common::Policy {
    homeguard_common::Policy {
        version: 0,
        timezone: "UTC".to_string(),
        rules: vec![],
        exceptions: vec![],
        protection: Default::default(),
    }
}
