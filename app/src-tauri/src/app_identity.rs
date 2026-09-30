//! The app's own permanent mTLS client identity — separate from the CA
//! (see `homeguard_app_core::ca_store`). Generated once on first launch
//! and reused forever afterwards; this is what every already-enrolled
//! agent verifies before accepting a request.

use std::path::{Path, PathBuf};

use homeguard_common::Ca;

pub struct AppIdentity {
    pub cert_pem: String,
    pub key_pem: String,
}

impl AppIdentity {
    pub fn load_or_generate(dir: &Path, ca: &Ca) -> std::io::Result<Self> {
        let cert_path = dir.join("app-client-cert.pem");
        let key_path = dir.join("app-client-key.pem");

        if cert_path.exists() && key_path.exists() {
            return Ok(AppIdentity {
                cert_pem: std::fs::read_to_string(&cert_path)?,
                key_pem: std::fs::read_to_string(&key_path)?,
            });
        }

        let identity = ca
            .issue_client_identity("homeguard-app")
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        std::fs::create_dir_all(dir)?;
        std::fs::write(&cert_path, &identity.cert_pem)?;
        write_private(&key_path, &identity.key_pem)?;
        Ok(AppIdentity {
            cert_pem: identity.cert_pem,
            key_pem: identity.key_pem,
        })
    }
}

#[cfg(unix)]
fn write_private(path: &PathBuf, contents: &str) -> std::io::Result<()> {
    use std::fs::{OpenOptions, Permissions};
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

    let mut file = OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(path)?;
    file.write_all(contents.as_bytes())?;
    file.set_permissions(Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn write_private(path: &PathBuf, contents: &str) -> std::io::Result<()> {
    std::fs::write(path, contents)
}
