//! Finds agents on the local network via mDNS. Blocking (uses
//! `Receiver::recv_timeout` under the hood) — call it from
//! `tokio::task::spawn_blocking` in async contexts (e.g. a Tauri command).

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use mdns_sd::{ServiceDaemon, ServiceEvent};
use serde::{Deserialize, Serialize};

pub const SERVICE_TYPE: &str = "_homeguard._tcp.local.";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveredAgent {
    pub agent_id: String,
    pub hostname: String,
    pub os: String,
    /// `std::env::consts::ARCH` as announced by the agent (e.g.
    /// `"x86_64"`, `"aarch64"`) — `"unknown"` for an older agent that
    /// doesn't announce it yet.
    pub arch: String,
    pub agent_version: String,
    pub ca_fingerprint_sha256: String,
    pub addr: SocketAddr,
}

/// Browse for `timeout`, returning every agent that answered. Agents
/// already known (from a previous scan or a manually-added IP) but
/// unreachable this time simply won't appear — the caller decides how to
/// treat that (e.g. mark "offline" rather than delete).
pub fn browse_once(timeout: Duration) -> Result<Vec<DiscoveredAgent>, mdns_sd::Error> {
    let daemon = ServiceDaemon::new()?;
    let receiver = daemon.browse(SERVICE_TYPE)?;

    let mut found = Vec::new();
    let deadline = Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        match receiver.recv_timeout(remaining) {
            Ok(ServiceEvent::ServiceResolved(info)) => {
                let resolved = info.as_resolved_service();
                if let Some(agent) = to_discovered_agent(&resolved) {
                    found.push(agent);
                }
            }
            Ok(_) => continue,
            Err(_) => break, // timeout or channel closed
        }
    }

    let _ = daemon.stop_browse(SERVICE_TYPE);
    let _ = daemon.shutdown();
    Ok(found)
}

fn to_discovered_agent(resolved: &mdns_sd::ResolvedService) -> Option<DiscoveredAgent> {
    if !resolved.is_valid() {
        return None;
    }
    let props = &resolved.txt_properties;
    let agent_id = props.get_property_val_str("id")?.to_string();
    let os = props.get_property_val_str("os").unwrap_or("unknown").to_string();
    let arch = props.get_property_val_str("arch").unwrap_or("unknown").to_string();
    let agent_version = props.get_property_val_str("ver").unwrap_or("unknown").to_string();
    let ca_fingerprint_sha256 = props.get_property_val_str("fp").unwrap_or("").to_string();
    let addr = resolved.addresses.iter().next()?;
    let hostname = resolved.host.trim_end_matches('.').to_string();

    Some(DiscoveredAgent {
        agent_id,
        hostname,
        os,
        arch,
        agent_version,
        ca_fingerprint_sha256,
        addr: SocketAddr::new(*addr, resolved.port),
    })
}
