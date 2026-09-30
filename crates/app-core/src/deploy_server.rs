//! Serves the one-line install scripts (`install.sh` / `install.ps1`, see
//! `scripts/`) with their placeholders substituted, plus the embedded
//! agent binaries they download. Plain HTTP, not mTLS: this is exactly
//! what PLAN.md section 4 flags as attackable by an on-LAN MITM — the
//! actual trust anchors are the binary's SHA-256 (checked by the script)
//! and the CA fingerprint (checked by `homeguard-agent enroll` itself
//! over a pinned HTTPS connection), not this server's transport.
//!
//! Runs alongside, but separately from, the enrollment HTTPS server
//! ([`crate::enrollment_server`]) — different protocol, different port,
//! same lifecycle (both start when "Ajouter des postes" opens, both stop
//! when it closes).

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;

use axum::extract::{Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use serde::Deserialize;
use std::sync::Arc;

pub const INSTALL_SH_TEMPLATE: &str = include_str!("../../../scripts/install.sh");
pub const INSTALL_PS1_TEMPLATE: &str = include_str!("../../../scripts/install.ps1");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AgentTarget {
    LinuxX86_64,
    LinuxArm64,
    Windows,
}

impl AgentTarget {
    fn route_path(self) -> &'static str {
        match self {
            AgentTarget::LinuxX86_64 => "/agent/linux-x86_64",
            AgentTarget::LinuxArm64 => "/agent/linux-arm64",
            AgentTarget::Windows => "/agent/windows-x86_64.exe",
        }
    }

    /// Picks the binary a given poste needs, from the `os`/`arch` it
    /// reported (`std::env::consts::OS`/`ARCH` — see `db::Agent`). `None`
    /// for any combination no binary is cross-compiled for (see
    /// `scripts/build-agents.sh`) — notably 32-bit or non-x86_64 Windows,
    /// and any non-Linux, non-Windows agent OS.
    pub fn from_os_arch(os: &str, arch: &str) -> Option<Self> {
        match (os, arch) {
            ("linux", "x86_64") => Some(AgentTarget::LinuxX86_64),
            ("linux", "aarch64") => Some(AgentTarget::LinuxArm64),
            ("windows", "x86_64") => Some(AgentTarget::Windows),
            _ => None,
        }
    }
}

struct AgentBinary {
    path: PathBuf,
    sha256_hex: String,
}

pub struct DeployServer {
    /// This server's own base URL (e.g. `http://192.168.1.20:7880`) — used
    /// to build the `__AGENT_URL_*__` placeholders, which point back at
    /// the `/agent/...` routes below.
    base_url: String,
    enroll_url: String,
    ca_fingerprint: String,
    binaries: HashMap<AgentTarget, AgentBinary>,
}

#[derive(Debug, thiserror::Error)]
pub enum DeployServerError {
    #[error("failed to read agent binary at {path}: {source}")]
    ReadBinary {
        path: String,
        #[source]
        source: std::io::Error,
    },
}

impl DeployServer {
    /// `binary_paths` maps each target to the compiled agent binary for
    /// it (e.g. a Tauri-bundled resource). A target with no entry simply
    /// renders a script that reports the download as unavailable instead
    /// of a broken/insecure link — see [`DeployServer::render`].
    pub fn new(
        base_url: String,
        enroll_url: String,
        ca_fingerprint: String,
        binary_paths: HashMap<AgentTarget, PathBuf>,
    ) -> Result<Self, DeployServerError> {
        let mut binaries = HashMap::new();
        for (target, path) in binary_paths {
            let bytes = std::fs::read(&path).map_err(|source| DeployServerError::ReadBinary {
                path: path.display().to_string(),
                source,
            })?;
            let sha256_hex = homeguard_common::sha256_hex(&bytes);
            binaries.insert(target, AgentBinary { path, sha256_hex });
        }
        Ok(DeployServer {
            base_url,
            enroll_url,
            ca_fingerprint,
            binaries,
        })
    }

    fn agent_url(&self, target: AgentTarget) -> String {
        format!("{}{}", self.base_url, target.route_path())
    }

    /// Substitute every placeholder in `template` (either
    /// [`INSTALL_SH_TEMPLATE`] or [`INSTALL_PS1_TEMPLATE`]) with this
    /// server's actual values. A target with no embedded binary gets a
    /// URL that 404s and a SHA-256 of all zeros — the script's own
    /// checksum comparison then fails loudly instead of silently
    /// accepting whatever it downloaded (or downloading nothing at all).
    pub fn render(&self, template: &str, token: &str) -> String {
        let url = |target: AgentTarget| {
            self.binaries
                .get(&target)
                .map(|_| self.agent_url(target))
                .unwrap_or_else(|| "about:unavailable".to_string())
        };
        let sha = |target: AgentTarget| {
            self.binaries
                .get(&target)
                .map(|b| b.sha256_hex.clone())
                .unwrap_or_else(|| "0".repeat(64))
        };

        template
            .replace("__AGENT_URL_LINUX_X86_64__", &url(AgentTarget::LinuxX86_64))
            .replace("__AGENT_SHA256_LINUX_X86_64__", &sha(AgentTarget::LinuxX86_64))
            .replace("__AGENT_URL_LINUX_ARM64__", &url(AgentTarget::LinuxArm64))
            .replace("__AGENT_SHA256_LINUX_ARM64__", &sha(AgentTarget::LinuxArm64))
            .replace("__AGENT_URL_WINDOWS__", &url(AgentTarget::Windows))
            .replace("__AGENT_SHA256_WINDOWS__", &sha(AgentTarget::Windows))
            .replace("__ENROLL_URL__", &self.enroll_url)
            .replace("__TOKEN__", token)
            .replace("__CA_SHA256__", &self.ca_fingerprint)
    }

    pub fn has_binary(&self, target: AgentTarget) -> bool {
        self.binaries.contains_key(&target)
    }
}

#[derive(Deserialize)]
struct TokenQuery {
    t: Option<String>,
}

pub fn router(server: Arc<DeployServer>) -> Router {
    Router::new()
        .route("/install.sh", get(get_install_sh))
        .route("/install.ps1", get(get_install_ps1))
        .route("/agent/linux-x86_64", get(get_agent_linux_x86_64))
        .route("/agent/linux-arm64", get(get_agent_linux_arm64))
        .route("/agent/windows-x86_64.exe", get(get_agent_windows))
        .with_state(server)
}

async fn get_install_sh(State(server): State<Arc<DeployServer>>, Query(q): Query<TokenQuery>) -> Response {
    (
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        server.render(INSTALL_SH_TEMPLATE, &q.t.unwrap_or_default()),
    )
        .into_response()
}

async fn get_install_ps1(State(server): State<Arc<DeployServer>>, Query(q): Query<TokenQuery>) -> Response {
    (
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        server.render(INSTALL_PS1_TEMPLATE, &q.t.unwrap_or_default()),
    )
        .into_response()
}

async fn serve_binary(server: &DeployServer, target: AgentTarget) -> Response {
    let Some(binary) = server.binaries.get(&target) else {
        return (StatusCode::NOT_FOUND, "binaire non disponible pour cette plateforme").into_response();
    };
    match tokio::fs::read(&binary.path).await {
        Ok(bytes) => ([(header::CONTENT_TYPE, "application/octet-stream")], bytes).into_response(),
        Err(e) => {
            tracing::error!("échec de la lecture du binaire agent {}: {e}", binary.path.display());
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

async fn get_agent_linux_x86_64(State(server): State<Arc<DeployServer>>) -> Response {
    serve_binary(&server, AgentTarget::LinuxX86_64).await
}

async fn get_agent_linux_arm64(State(server): State<Arc<DeployServer>>) -> Response {
    serve_binary(&server, AgentTarget::LinuxArm64).await
}

async fn get_agent_windows(State(server): State<Arc<DeployServer>>) -> Response {
    serve_binary(&server, AgentTarget::Windows).await
}

/// Serve `server`'s routes on `addr` in plain HTTP until
/// `handle.shutdown()` is called elsewhere.
pub async fn serve(addr: SocketAddr, server: Arc<DeployServer>, handle: axum_server::Handle) -> std::io::Result<()> {
    axum_server::bind(addr)
        .handle(handle)
        .serve(router(server).into_make_service())
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLACEHOLDERS: &[&str] = &[
        "__AGENT_URL_LINUX_X86_64__",
        "__AGENT_URL_LINUX_ARM64__",
        "__AGENT_SHA256_LINUX_X86_64__",
        "__AGENT_SHA256_LINUX_ARM64__",
        "__AGENT_URL_WINDOWS__",
        "__AGENT_SHA256_WINDOWS__",
        "__ENROLL_URL__",
        "__TOKEN__",
        "__CA_SHA256__",
    ];

    fn assert_no_placeholders_remain(rendered: &str) {
        for placeholder in PLACEHOLDERS {
            assert!(!rendered.contains(placeholder), "{placeholder} was not substituted: {rendered}");
        }
    }

    #[test]
    fn from_os_arch_maps_every_cross_compiled_target() {
        assert_eq!(AgentTarget::from_os_arch("linux", "x86_64"), Some(AgentTarget::LinuxX86_64));
        assert_eq!(AgentTarget::from_os_arch("linux", "aarch64"), Some(AgentTarget::LinuxArm64));
        assert_eq!(AgentTarget::from_os_arch("windows", "x86_64"), Some(AgentTarget::Windows));
    }

    #[test]
    fn from_os_arch_is_none_for_an_uncompiled_combination() {
        assert_eq!(AgentTarget::from_os_arch("windows", "aarch64"), None);
        assert_eq!(AgentTarget::from_os_arch("macos", "aarch64"), None);
        assert_eq!(AgentTarget::from_os_arch("linux", "unknown"), None);
    }

    #[test]
    fn renders_install_sh_with_all_placeholders_substituted() {
        let mut paths = HashMap::new();
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), b"fake binary").unwrap();
        paths.insert(AgentTarget::LinuxX86_64, tmp.path().to_path_buf());

        let server = DeployServer::new(
            "http://192.168.1.20:7880".into(),
            "https://192.168.1.20:7879/v1/enroll".into(),
            "aa:bb:cc".into(),
            paths,
        )
        .unwrap();

        let rendered = server.render(INSTALL_SH_TEMPLATE, "K7Q2-9XPA");

        assert_no_placeholders_remain(&rendered);
        assert!(rendered.contains("http://192.168.1.20:7880/agent/linux-x86_64"));
        assert!(rendered.contains("https://192.168.1.20:7879/v1/enroll"));
        assert!(rendered.contains("K7Q2-9XPA"));
        assert!(rendered.contains("aa:bb:cc"));
        // arm64 was never embedded — placeholder URL/hash, not blank.
        assert!(rendered.contains("about:unavailable"));
        assert!(rendered.contains(&"0".repeat(64)));
    }

    #[test]
    fn renders_install_ps1_with_all_placeholders_substituted() {
        let server = DeployServer::new(
            "http://192.168.1.20:7880".into(),
            "https://192.168.1.20:7879/v1/enroll".into(),
            "aa:bb:cc".into(),
            HashMap::new(),
        )
        .unwrap();

        let rendered = server.render(INSTALL_PS1_TEMPLATE, "K7Q2-9XPA");
        assert_no_placeholders_remain(&rendered);
        assert!(rendered.contains("K7Q2-9XPA"));
    }

    #[tokio::test]
    async fn serves_rendered_script_and_a_binary_over_real_http() {
        let mut paths = HashMap::new();
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), b"pretend this is the agent binary").unwrap();
        paths.insert(AgentTarget::LinuxX86_64, tmp.path().to_path_buf());

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = Arc::new(
            DeployServer::new(
                format!("http://{addr}"),
                "https://192.168.1.20:7879/v1/enroll".into(),
                "aa:bb:cc".into(),
                paths,
            )
            .unwrap(),
        );

        tokio::spawn(async move {
            axum_server::from_tcp(listener)
                .serve(router(server).into_make_service())
                .await
                .unwrap();
        });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let client = reqwest::Client::new();

        let script = client
            .get(format!("http://{addr}/install.sh?t=K7Q2-9XPA"))
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        assert!(script.contains("K7Q2-9XPA"));
        assert!(script.contains(&format!("http://{addr}/agent/linux-x86_64")));

        let binary = client
            .get(format!("http://{addr}/agent/linux-x86_64"))
            .send()
            .await
            .unwrap()
            .bytes()
            .await
            .unwrap();
        assert_eq!(&binary[..], b"pretend this is the agent binary");

        let missing = client.get(format!("http://{addr}/agent/windows-x86_64.exe")).send().await.unwrap();
        assert_eq!(missing.status(), reqwest::StatusCode::NOT_FOUND);
    }
}
