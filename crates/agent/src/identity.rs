use std::sync::Arc;

use rustls_pki_types::CertificateDer;
use homeguard_common::tls::TlsError;

use crate::config::AgentPaths;

#[derive(Debug, thiserror::Error)]
pub enum IdentityError {
    #[error("agent is not enrolled yet: {0} is missing — run `homeguard-agent enroll` first")]
    NotEnrolled(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Tls(#[from] TlsError),
    #[error(transparent)]
    Pem(#[from] pem::PemError),
}

/// The agent's own mTLS server identity, plus the CA it trusts for incoming
/// client connections. Loaded once at startup.
pub struct AgentIdentity {
    pub cert_pem: String,
    pub key_pem: String,
    pub ca_cert_pem: String,
    pub ca_cert_der: CertificateDer<'static>,
    pub agent_id: String,
}

impl AgentIdentity {
    pub fn load(paths: &AgentPaths) -> Result<Self, IdentityError> {
        let read = |path: &std::path::Path| -> Result<String, IdentityError> {
            std::fs::read_to_string(path)
                .map_err(|_| IdentityError::NotEnrolled(path.display().to_string()))
        };

        let cert_pem = read(&paths.cert_path())?;
        let key_pem = read(&paths.key_path())?;
        let ca_cert_pem = read(&paths.ca_cert_path())?;
        let agent_id = read(&paths.agent_id_path())?.trim().to_string();
        let ca_cert_der = pem::parse(&ca_cert_pem)?.into_contents().into();

        Ok(AgentIdentity {
            cert_pem,
            key_pem,
            ca_cert_pem,
            ca_cert_der,
            agent_id,
        })
    }

    pub fn save(paths: &AgentPaths, cert_pem: &str, key_pem: &str, ca_cert_pem: &str, agent_id: &str) -> std::io::Result<()> {
        paths.ensure_created()?;
        write_private(&paths.key_path(), key_pem)?;
        std::fs::write(paths.cert_path(), cert_pem)?;
        std::fs::write(paths.ca_cert_path(), ca_cert_pem)?;
        std::fs::write(paths.agent_id_path(), agent_id)?;
        Ok(())
    }

    pub fn server_tls_config(&self) -> Result<Arc<rustls::ServerConfig>, IdentityError> {
        Ok(homeguard_common::server_config_mtls(
            &self.cert_pem,
            &self.key_pem,
            &self.ca_cert_der,
        )?)
    }
}

#[cfg(unix)]
fn write_private(path: &std::path::Path, contents: &str) -> std::io::Result<()> {
    use std::fs::{OpenOptions, Permissions};
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

    // `.mode(0o600)` only applies to a file this call *creates* — if
    // `path` already exists (e.g. a stale file from an earlier install
    // left behind with looser permissions), opening it for truncation
    // does not change its mode. Set it explicitly afterwards so a private
    // key is never left world/group-readable regardless of prior state.
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(contents.as_bytes())?;
    file.set_permissions(Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn write_private(path: &std::path::Path, contents: &str) -> std::io::Result<()> {
    // On Windows the file inherits ACLs from `data_dir`, which the service
    // installer restricts to SYSTEM/Administrators.
    std::fs::write(path, contents)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn tightens_permissions_even_when_the_file_already_existed_with_looser_ones() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("key.pem");
        std::fs::write(&path, "stale content").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();

        write_private(&path, "fresh private key").unwrap();

        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "fresh private key");
    }
}
