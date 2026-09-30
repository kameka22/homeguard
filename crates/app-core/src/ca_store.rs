//! Where the app keeps its CA. `FileCaStore` (real, tested here) is the
//! fallback and what's used by default; the Tauri shell should instead
//! provide a Keychain-backed impl of [`CaStore`] on macOS (via
//! `security-framework`), per PLAN.md section 3 — the trait exists
//! precisely so that swap is a small, isolated change.

use std::path::PathBuf;

use homeguard_common::Ca;

pub trait CaStore: Send + Sync {
    fn load(&self) -> Result<Option<Ca>, CaStoreError>;
    fn save(&self, ca: &Ca) -> Result<(), CaStoreError>;
}

#[derive(Debug, thiserror::Error)]
pub enum CaStoreError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Pki(#[from] homeguard_common::PkiError),
}

/// Stores `ca-cert.pem` and `ca-key.pem` under a directory. Good enough for
/// Linux/dev use; on macOS the Tauri shell should prefer a Keychain-backed
/// `CaStore` so the private key never touches disk in the clear.
pub struct FileCaStore {
    dir: PathBuf,
}

impl FileCaStore {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        FileCaStore { dir: dir.into() }
    }

    fn cert_path(&self) -> PathBuf {
        self.dir.join("ca-cert.pem")
    }

    fn key_path(&self) -> PathBuf {
        self.dir.join("ca-key.pem")
    }
}

impl CaStore for FileCaStore {
    fn load(&self) -> Result<Option<Ca>, CaStoreError> {
        let (cert_path, key_path) = (self.cert_path(), self.key_path());
        if !cert_path.exists() || !key_path.exists() {
            return Ok(None);
        }
        let cert_pem = std::fs::read_to_string(cert_path)?;
        let key_pem = std::fs::read_to_string(key_path)?;
        Ok(Some(Ca::from_pem(&cert_pem, &key_pem)?))
    }

    fn save(&self, ca: &Ca) -> Result<(), CaStoreError> {
        std::fs::create_dir_all(&self.dir)?;
        std::fs::write(self.cert_path(), ca.cert_pem())?;
        write_private(&self.key_path(), &ca.key_pem())?;
        Ok(())
    }
}

/// Load the existing CA, or generate and persist a new one — the "one
/// click" first-launch flow from PLAN.md section 3.
pub fn load_or_generate(store: &dyn CaStore) -> Result<Ca, CaStoreError> {
    if let Some(ca) = store.load()? {
        return Ok(ca);
    }
    let ca = Ca::generate()?;
    store.save(&ca)?;
    Ok(ca)
}

#[cfg(unix)]
fn write_private(path: &std::path::Path, contents: &str) -> std::io::Result<()> {
    use std::fs::{OpenOptions, Permissions};
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

    // See the identical comment in agent/src/identity.rs: `.mode()` only
    // takes effect on creation, so a pre-existing file's permissions must
    // be tightened explicitly too.
    let mut file = OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(path)?;
    file.write_all(contents.as_bytes())?;
    file.set_permissions(Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn write_private(path: &std::path::Path, contents: &str) -> std::io::Result<()> {
    std::fs::write(path, contents)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_once_then_reloads_the_same_ca() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileCaStore::new(dir.path());

        assert!(store.load().unwrap().is_none());
        let ca = load_or_generate(&store).unwrap();
        let fingerprint = ca.fingerprint_sha256();

        let reloaded = load_or_generate(&store).unwrap();
        assert_eq!(reloaded.fingerprint_sha256(), fingerprint);
    }

    #[cfg(unix)]
    #[test]
    fn tightens_key_permissions_even_when_the_file_already_existed_with_looser_ones() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let store = FileCaStore::new(dir.path());
        let key_path = dir.path().join("ca-key.pem");
        std::fs::write(&key_path, "stale").unwrap();
        std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o644)).unwrap();

        let ca = Ca::generate().unwrap();
        store.save(&ca).unwrap();

        let mode = std::fs::metadata(&key_path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
}
