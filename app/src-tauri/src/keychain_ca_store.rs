//! Keychain-backed [`CaStore`]: the CA certificate and private key are
//! stored as a single generic-password item (folded from an earlier
//! two-item layout — see `load`'s migration). This is the production
//! `CaStore` on macOS — `ca_store::FileCaStore` in `app-core` remains the
//! fallback used elsewhere (tests, and any non-macOS build of this crate).
//!
//! Deliberately *not* gated behind Touch ID
//! (`AccessControlOptions::BIOMETRY_ANY`, via `PasswordOptions::set_access_control_options`):
//! `SecItemAdd` with a `kSecAttrAccessControl` policy fails with
//! `errSecMissingEntitlement` ("A required entitlement isn't present")
//! unless the app is code-signed with the `keychain-access-groups`
//! entitlement under a real Apple Developer Team ID — plain ad-hoc/unsigned
//! builds (`cargo tauri dev`, and `tauri build` without a signing identity)
//! can't create such an item at all, confirmed by a real crash on this
//! Mac. Revisit once the app has proper code signing (see PLAN.md's
//! packaging/notarization item) — the merge into one item here already
//! halves the number of Keychain confirmation prompts in the meantime.

use security_framework::base::Error as SfError;
use security_framework::passwords::{delete_generic_password, generic_password, set_generic_password_options, PasswordOptions};
use serde::{Deserialize, Serialize};
use homeguard_app_core::ca_store::{CaStore, CaStoreError};
use homeguard_common::Ca;

const SERVICE: &str = "com.homeguard.desktop.ca";
const ACCOUNT_BUNDLE: &str = "ca-bundle";

// Pre-Touch-ID layout: cert and key as two separate, non-access-controlled
// items. `load` migrates away from this the first time it runs on an app
// data directory that still has them; not used for new writes.
const ACCOUNT_CERT: &str = "ca-cert-pem";
const ACCOUNT_KEY: &str = "ca-key-pem";

// security_framework_sys::base::errSecItemNotFound, inlined to avoid an
// extra direct dependency on the (already transitively pulled in) -sys
// crate just for one constant.
const ERR_SEC_ITEM_NOT_FOUND: i32 = -25300;

#[derive(Serialize, Deserialize)]
struct CaBundle {
    cert_pem: String,
    key_pem: String,
}

pub struct KeychainCaStore;

impl KeychainCaStore {
    pub fn new() -> Self {
        KeychainCaStore
    }
}

impl Default for KeychainCaStore {
    fn default() -> Self {
        Self::new()
    }
}

impl CaStore for KeychainCaStore {
    fn load(&self) -> Result<Option<Ca>, CaStoreError> {
        if let Some(bundle) = read_bundle()? {
            return Ok(Some(Ca::from_pem(&bundle.cert_pem, &bundle.key_pem)?));
        }

        // One-time migration from the pre-Touch-ID layout: fold the two
        // separate items into the single biometry-gated bundle and remove
        // them, so the CA's private key doesn't keep sitting in an entry
        // that only needs a typed password to read.
        let Some(cert_pem) = read_legacy_item(ACCOUNT_CERT)? else {
            return Ok(None);
        };
        let Some(key_pem) = read_legacy_item(ACCOUNT_KEY)? else {
            return Ok(None);
        };
        let ca = Ca::from_pem(&cert_pem, &key_pem)?;
        write_bundle(&CaBundle { cert_pem, key_pem })?;
        let _ = delete_generic_password(SERVICE, ACCOUNT_CERT);
        let _ = delete_generic_password(SERVICE, ACCOUNT_KEY);
        Ok(Some(ca))
    }

    fn save(&self, ca: &Ca) -> Result<(), CaStoreError> {
        write_bundle(&CaBundle {
            cert_pem: ca.cert_pem().to_string(),
            key_pem: ca.key_pem(),
        })
    }
}

fn read_bundle() -> Result<Option<CaBundle>, CaStoreError> {
    match generic_password(PasswordOptions::new_generic_password(SERVICE, ACCOUNT_BUNDLE)) {
        Ok(bytes) => {
            let bundle: CaBundle = serde_json::from_slice(&bytes).map_err(|e| {
                CaStoreError::Io(std::io::Error::other(format!(
                    "élément Trousseau {ACCOUNT_BUNDLE} illisible : {e}"
                )))
            })?;
            Ok(Some(bundle))
        }
        Err(e) if is_not_found(&e) => Ok(None),
        Err(e) => Err(CaStoreError::Io(std::io::Error::other(format!(
            "échec de la lecture du CA dans le Trousseau : {e}"
        )))),
    }
}

fn write_bundle(bundle: &CaBundle) -> Result<(), CaStoreError> {
    let bytes = serde_json::to_vec(bundle).map_err(|e| {
        CaStoreError::Io(std::io::Error::other(format!("échec de sérialisation du CA : {e}")))
    })?;

    // `set_generic_password_options` already falls back to `SecItemUpdate`
    // on a duplicate, but deleting first keeps this on the plain-create
    // path unconditionally — one fewer case to reason about, and cheap
    // since a missing item is a normal, silent outcome here.
    let _ = delete_generic_password(SERVICE, ACCOUNT_BUNDLE);

    let options = PasswordOptions::new_generic_password(SERVICE, ACCOUNT_BUNDLE);
    set_generic_password_options(&bytes, options).map_err(|e| {
        CaStoreError::Io(std::io::Error::other(format!("échec de l'écriture du CA dans le Trousseau : {e}")))
    })
}

fn read_legacy_item(account: &str) -> Result<Option<String>, CaStoreError> {
    match generic_password(PasswordOptions::new_generic_password(SERVICE, account)) {
        Ok(bytes) => {
            let text = String::from_utf8(bytes)
                .map_err(|e| CaStoreError::Io(std::io::Error::other(format!("Keychain item {account} is not valid UTF-8: {e}"))))?;
            Ok(Some(text))
        }
        Err(e) if is_not_found(&e) => Ok(None),
        Err(e) => Err(CaStoreError::Io(std::io::Error::other(format!(
            "échec de la lecture de {account} dans le Trousseau : {e}"
        )))),
    }
}

fn is_not_found(e: &SfError) -> bool {
    e.code() == ERR_SEC_ITEM_NOT_FOUND
}
