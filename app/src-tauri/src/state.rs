use std::path::Path;
use std::sync::{Arc, Mutex};

use homeguard_app_core::ca_store::{CaStore, FileCaStore};
use homeguard_app_core::db::Db;
use homeguard_app_core::enrollment_server::EnrollmentState;
use homeguard_common::Ca;

use crate::app_identity::AppIdentity;

const DEFAULT_TIMEZONE: &str = "Europe/Paris";

pub struct EnrollmentSession {
    /// HTTPS enrollment server (mTLS-pinned) — where the agent's own
    /// `enroll` command actually posts its CSR. Always bound on
    /// `commands::ENROLLMENT_PORT`.
    pub handle: axum_server::Handle,
    pub enrollment_state: Arc<EnrollmentState>,
    /// Plain-HTTP script/binary server for the one-line installers — a
    /// separate port (`commands::DEPLOY_PORT`), separate trust model (see
    /// `deploy_server`'s module doc).
    pub deploy_handle: axum_server::Handle,
}

pub struct AppState {
    pub db: Arc<Db>,
    pub ca: Arc<Ca>,
    pub client_identity: AppIdentity,
    pub enrollment: Mutex<Option<EnrollmentSession>>,
}

impl AppState {
    pub fn init(app_data_dir: &Path) -> anyhow::Result<Self> {
        homeguard_common::install_crypto_provider();
        std::fs::create_dir_all(app_data_dir)?;

        let ca = Arc::new(resolve_ca(app_data_dir)?);
        let client_identity = AppIdentity::load_or_generate(&app_data_dir.join("identity"), &ca)?;
        let db = Arc::new(Db::open(&app_data_dir.join("homeguard.sqlite3"))?);

        if db.get_setting("timezone")?.is_none() {
            db.set_setting("timezone", DEFAULT_TIMEZONE)?;
        }

        Ok(AppState {
            db,
            ca,
            client_identity,
            enrollment: Mutex::new(None),
        })
    }

    pub fn timezone(&self) -> String {
        self.db
            .get_setting("timezone")
            .ok()
            .flatten()
            .unwrap_or_else(|| DEFAULT_TIMEZONE.to_string())
    }
}

/// The CA is the app's most sensitive secret (it can mint a certificate
/// any agent will trust), so on macOS it lives in the Keychain rather
/// than as a plain file — see `keychain_ca_store`. Any other build target
/// of this crate falls back to `FileCaStore` (matching the one used in
/// `app-core`'s own tests).
#[cfg(target_os = "macos")]
fn resolve_ca(app_data_dir: &Path) -> anyhow::Result<Ca> {
    let keychain_store = crate::keychain_ca_store::KeychainCaStore::new();
    if let Some(ca) = keychain_store.load()? {
        return Ok(ca);
    }

    // One-time migration: an earlier build of this app (before Keychain
    // storage) may have left a file-based CA here. Reuse it instead of
    // silently minting a new one, which would orphan every agent already
    // enrolled against the old CA.
    let legacy_dir = app_data_dir.join("ca");
    let legacy_store = FileCaStore::new(&legacy_dir);
    if let Some(ca) = legacy_store.load()? {
        keychain_store.save(&ca)?;
        let _ = std::fs::remove_dir_all(&legacy_dir);
        return Ok(ca);
    }

    let ca = Ca::generate()?;
    keychain_store.save(&ca)?;
    Ok(ca)
}

#[cfg(not(target_os = "macos"))]
fn resolve_ca(app_data_dir: &Path) -> anyhow::Result<Ca> {
    let ca_store = FileCaStore::new(app_data_dir.join("ca"));
    Ok(homeguard_app_core::ca_store::load_or_generate(&ca_store)?)
}

/// Persists an imported CA through the same store `resolve_ca` reads from,
/// so it's picked up automatically the next time the app starts. Doesn't
/// touch the *running* app's identity (`AppState.ca`) — swapping a live
/// TLS identity out from under in-flight connections isn't worth the risk
/// when "ask for a restart" is simple and safe instead.
#[cfg(target_os = "macos")]
pub fn store_ca(_app_data_dir: &Path, ca: &Ca) -> anyhow::Result<()> {
    crate::keychain_ca_store::KeychainCaStore::new().save(ca)?;
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub fn store_ca(app_data_dir: &Path, ca: &Ca) -> anyhow::Result<()> {
    FileCaStore::new(app_data_dir.join("ca")).save(ca)?;
    Ok(())
}
