use mdns_sd::{ServiceDaemon, ServiceInfo};

const SERVICE_TYPE: &str = "_homeguard._tcp.local.";

/// Announce this agent on the local network so the app can find it without
/// the admin typing an IP. Returns the daemon; drop it (or let the process
/// exit) to stop announcing.
pub fn announce(
    agent_id: &str,
    hostname: &str,
    os: &str,
    arch: &str,
    agent_version: &str,
    port: u16,
    ca_fingerprint_sha256: &str,
) -> Result<ServiceDaemon, mdns_sd::Error> {
    let daemon = ServiceDaemon::new()?;

    let instance_name = format!("{hostname}-{}", &agent_id[..agent_id.len().min(8)]);
    let host_name = format!("{hostname}.local.");

    let mut properties = std::collections::HashMap::new();
    properties.insert("id".to_string(), agent_id.to_string());
    properties.insert("os".to_string(), os.to_string());
    properties.insert("arch".to_string(), arch.to_string());
    properties.insert("ver".to_string(), agent_version.to_string());
    properties.insert("fp".to_string(), ca_fingerprint_sha256.to_string());

    let service = ServiceInfo::new(
        SERVICE_TYPE,
        &instance_name,
        &host_name,
        (),
        port,
        properties,
    )?
    .enable_addr_auto();

    daemon.register(service)?;
    Ok(daemon)
}
