//! Encrypted backup/migration of the app's full configuration — every
//! poste, group, list, assignment, exception and setting, plus the CA
//! itself. Including the CA is what makes this a genuine *migration*
//! format rather than just a backup: without it, a restored app would
//! mint a fresh CA and every already-enrolled agent would need
//! re-enrolling from scratch. That's also exactly why this is always
//! encrypted (age, passphrase-based) — the file alone is enough to
//! impersonate the app to every agent that trusts it.

use age::secrecy::SecretString;
use serde::{Deserialize, Serialize};
use homeguard_common::policy::TimeWindow;
use homeguard_common::Ca;

use crate::db::{Agent, Assignment, Db, DbError, Exception, Group, List};

const FORMAT_VERSION: u32 = 1;

#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    #[error(transparent)]
    Db(#[from] DbError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Pki(#[from] homeguard_common::PkiError),
    #[error("échec du chiffrement : {0}")]
    Encrypt(String),
    #[error("mot de passe incorrect ou fichier corrompu")]
    Decrypt,
    #[error("format de sauvegarde non reconnu (v{found}, celle-ci ne connaît que v{FORMAT_VERSION})")]
    UnsupportedVersion { found: u32 },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ExportedConfig {
    pub format_version: u32,
    pub ca_cert_pem: String,
    pub ca_key_pem: String,
    pub agents: Vec<Agent>,
    pub groups: Vec<Group>,
    pub group_members: Vec<(String, String)>,
    pub lists: Vec<List>,
    pub assignments: Vec<Assignment>,
    pub exceptions: Vec<Exception>,
    pub settings: Vec<(String, String)>,
}

pub fn build_export(db: &Db, ca: &Ca) -> Result<ExportedConfig, ExportError> {
    Ok(ExportedConfig {
        format_version: FORMAT_VERSION,
        ca_cert_pem: ca.cert_pem().to_string(),
        ca_key_pem: ca.key_pem(),
        agents: db.list_agents()?,
        groups: db.list_groups()?,
        group_members: db.list_group_members()?,
        lists: db.list_lists()?,
        assignments: db.list_assignments()?,
        exceptions: db.list_exceptions()?,
        settings: db.list_settings()?,
    })
}

/// Ascii-armored (plain-text-safe) ciphertext, so the resulting file can be
/// emailed, pasted, or opened in a text editor without surprises.
pub fn encrypt_export(config: &ExportedConfig, passphrase: &str) -> Result<String, ExportError> {
    let plaintext = serde_json::to_vec(config)?;
    let recipient = age::scrypt::Recipient::new(SecretString::from(passphrase.to_string()));
    age::encrypt_and_armor(&recipient, &plaintext).map_err(|e| ExportError::Encrypt(e.to_string()))
}

pub fn decrypt_export(armored: &str, passphrase: &str) -> Result<ExportedConfig, ExportError> {
    let identity = age::scrypt::Identity::new(SecretString::from(passphrase.to_string()));
    let plaintext = age::decrypt(&identity, armored.as_bytes()).map_err(|_| ExportError::Decrypt)?;
    let config: ExportedConfig = serde_json::from_slice(&plaintext)?;
    if config.format_version != FORMAT_VERSION {
        return Err(ExportError::UnsupportedVersion { found: config.format_version });
    }
    Ok(config)
}

/// Reconstructs the CA from an imported config — callers persist this
/// through their own `CaStore` (Keychain on macOS, a file elsewhere); this
/// function only parses it.
pub fn ca_from_export(config: &ExportedConfig) -> Result<Ca, ExportError> {
    Ok(Ca::from_pem(&config.ca_cert_pem, &config.ca_key_pem)?)
}

/// Merges every piece of config back into `db`. Every row is upserted by
/// its original id, so importing the same file twice — or importing over
/// an existing, partially-matching config — is safe and idempotent rather
/// than duplicating anything.
pub fn apply_import(db: &Db, config: &ExportedConfig) -> Result<(), ExportError> {
    for agent in &config.agents {
        db.upsert_agent(
            &agent.id,
            &agent.hostname,
            &agent.os,
            &agent.arch,
            agent.last_ip.as_deref(),
            agent.agent_version.as_deref(),
        )?;
        if let Some(name) = &agent.display_name {
            db.set_agent_display_name(&agent.id, name)?;
        }
        if let Some(serial) = &agent.cert_serial {
            db.set_agent_cert_serial(&agent.id, serial)?;
        }
        if agent.revoked {
            db.revoke_agent(&agent.id)?;
        }
        db.set_agent_events_cursor(&agent.id, agent.events_cursor)?;
    }

    for group in &config.groups {
        db.upsert_group(&group.id, &group.name)?;
    }
    for (group_id, agent_id) in &config.group_members {
        db.add_group_member(group_id, agent_id)?;
    }

    for list in &config.lists {
        db.upsert_list(&list.id, &list.name, &list.domains)?;
    }

    for assignment in &config.assignments {
        let schedule: Option<&[TimeWindow]> = assignment.schedule.as_deref();
        db.upsert_assignment(&assignment.id, &assignment.list_id, assignment.target_kind, &assignment.target_id, schedule)?;
    }

    for exception in &config.exceptions {
        db.upsert_exception(
            &exception.id,
            exception.target_kind,
            &exception.target_id,
            &exception.domains,
            exception.until,
        )?;
    }

    for (key, value) in &config.settings {
        db.set_setting(key, value)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::TargetKind as TK;

    fn sample_db() -> Db {
        let db = Db::open_in_memory().unwrap();
        db.upsert_agent("a1", "PORTABLE-LEO", "windows", "x86_64", Some("192.168.1.9"), Some("0.1.0")).unwrap();
        db.set_agent_display_name("a1", "PC de Léo").unwrap();
        let group_id = db.create_group("Enfants").unwrap();
        db.add_group_member(&group_id, "a1").unwrap();
        let list_id = db.create_list("Jeux", &["roblox.com".into()]).unwrap();
        db.create_assignment(&list_id, TK::Group, &group_id, None).unwrap();
        let until = jiff::Timestamp::now() + jiff::SignedDuration::from_secs(1800);
        db.create_exception(TK::Agent, "a1", &["twitch.tv".into()], until).unwrap();
        db.set_setting("timezone", "Europe/Paris").unwrap();
        db
    }

    #[test]
    fn round_trips_through_encryption() {
        let db = sample_db();
        let ca = Ca::generate().unwrap();
        let config = build_export(&db, &ca).unwrap();

        let armored = encrypt_export(&config, "correct horse battery staple").unwrap();
        assert!(armored.starts_with("-----BEGIN AGE ENCRYPTED FILE-----"));

        let decrypted = decrypt_export(&armored, "correct horse battery staple").unwrap();
        assert_eq!(decrypted.agents.len(), 1);
        assert_eq!(decrypted.ca_cert_pem, ca.cert_pem());
    }

    #[test]
    fn wrong_passphrase_is_rejected() {
        let db = sample_db();
        let ca = Ca::generate().unwrap();
        let config = build_export(&db, &ca).unwrap();
        let armored = encrypt_export(&config, "correct horse battery staple").unwrap();

        let result = decrypt_export(&armored, "wrong passphrase");
        assert!(matches!(result, Err(ExportError::Decrypt)));
    }

    #[test]
    fn import_into_a_fresh_db_recreates_everything_with_the_same_ids() {
        let source = sample_db();
        let ca = Ca::generate().unwrap();
        let config = build_export(&source, &ca).unwrap();

        let target = Db::open_in_memory().unwrap();
        apply_import(&target, &config).unwrap();

        let agents = target.list_agents().unwrap();
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].id, "a1");
        assert_eq!(agents[0].display_name.as_deref(), Some("PC de Léo"));

        assert_eq!(target.list_groups().unwrap().len(), 1);
        assert_eq!(target.list_group_members().unwrap().len(), 1);
        assert_eq!(target.list_lists().unwrap().len(), 1);
        assert_eq!(target.list_assignments().unwrap().len(), 1);
        assert_eq!(target.list_exceptions().unwrap().len(), 1);
        assert_eq!(target.get_setting("timezone").unwrap().as_deref(), Some("Europe/Paris"));

        let restored_ca = ca_from_export(&config).unwrap();
        assert_eq!(restored_ca.fingerprint_sha256(), ca.fingerprint_sha256());
    }

    #[test]
    fn importing_twice_does_not_duplicate_rows() {
        let source = sample_db();
        let ca = Ca::generate().unwrap();
        let config = build_export(&source, &ca).unwrap();

        let target = Db::open_in_memory().unwrap();
        apply_import(&target, &config).unwrap();
        apply_import(&target, &config).unwrap();

        assert_eq!(target.list_agents().unwrap().len(), 1);
        assert_eq!(target.list_groups().unwrap().len(), 1);
        assert_eq!(target.list_lists().unwrap().len(), 1);
        assert_eq!(target.list_assignments().unwrap().len(), 1);
        assert_eq!(target.list_exceptions().unwrap().len(), 1);
    }

    #[test]
    fn rejects_a_future_format_version() {
        let db = sample_db();
        let ca = Ca::generate().unwrap();
        let mut config = build_export(&db, &ca).unwrap();
        config.format_version = FORMAT_VERSION + 1;
        let armored = encrypt_export(&config, "pw").unwrap();

        let result = decrypt_export(&armored, "pw");
        assert!(matches!(result, Err(ExportError::UnsupportedVersion { .. })));
    }
}
