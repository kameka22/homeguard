use std::path::{Path, PathBuf};

/// Every file the agent owns lives under one directory, overridable for
/// tests. In production this resolves per-OS (see [`AgentPaths::default_for_os`]).
#[derive(Debug, Clone)]
pub struct AgentPaths {
    pub data_dir: PathBuf,
}

impl AgentPaths {
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        AgentPaths {
            data_dir: data_dir.into(),
        }
    }

    #[cfg(target_os = "windows")]
    pub fn default_for_os() -> Self {
        let base = std::env::var_os("ProgramData")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"));
        AgentPaths::new(base.join("HomeGuard"))
    }

    #[cfg(not(target_os = "windows"))]
    pub fn default_for_os() -> Self {
        AgentPaths::new("/etc/homeguard")
    }

    pub fn ca_cert_path(&self) -> PathBuf {
        self.data_dir.join("ca.pem")
    }

    pub fn cert_path(&self) -> PathBuf {
        self.data_dir.join("cert.pem")
    }

    pub fn key_path(&self) -> PathBuf {
        self.data_dir.join("key.pem")
    }

    pub fn agent_id_path(&self) -> PathBuf {
        self.data_dir.join("agent-id")
    }

    pub fn policy_path(&self) -> PathBuf {
        self.data_dir.join("policy.json")
    }

    pub fn events_path(&self) -> PathBuf {
        self.data_dir.join("events.jsonl")
    }

    /// Real system `hosts` file this agent must manage. Independent from
    /// `data_dir` since it always points at a fixed OS path (tests override
    /// it separately via `AgentConfig::hosts_path`).
    #[cfg(target_os = "windows")]
    pub fn system_hosts_path() -> PathBuf {
        let base = std::env::var_os("SystemRoot")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
        base.join(r"System32\drivers\etc\hosts")
    }

    #[cfg(not(target_os = "windows"))]
    pub fn system_hosts_path() -> PathBuf {
        PathBuf::from("/etc/hosts")
    }

    pub fn ensure_created(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.data_dir)
    }
}

/// Full runtime configuration for `run`, combining on-disk paths with the
/// address the API server listens on.
#[derive(Debug, Clone)]
pub struct AgentConfig {
    pub paths: AgentPaths,
    pub hosts_path: PathBuf,
    pub listen_addr: std::net::SocketAddr,
    pub mdns_enabled: bool,
}

impl AgentConfig {
    pub fn production(paths: AgentPaths) -> Self {
        AgentConfig {
            hosts_path: AgentPaths::system_hosts_path(),
            listen_addr: "0.0.0.0:7878".parse().unwrap(),
            mdns_enabled: true,
            paths,
        }
    }

    pub fn for_tests(data_dir: &Path, hosts_path: &Path, port: u16) -> Self {
        AgentConfig {
            paths: AgentPaths::new(data_dir),
            hosts_path: hosts_path.to_path_buf(),
            listen_addr: ([127, 0, 0, 1], port).into(),
            mdns_enabled: false,
        }
    }
}
