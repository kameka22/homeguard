//! Disables DNS-over-HTTPS in the major Chromium-based browsers and
//! Firefox via machine-wide policy, so they can't bypass the hosts file.
//! Re-applied on every scheduler pass (see `state::recompute_and_apply`),
//! so a user resetting the policy just gets it reinstated within one
//! heartbeat.

use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum BrowserPolicyError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

/// Apply (or lift) the DNS-over-HTTPS-disabled policy for every browser we
/// know how to manage. `etc_root` is `/etc` in production and a temp
/// directory in tests; ignored on Windows, where policy lives in the
/// registry instead.
pub fn apply(etc_root: &Path, disable_doh: bool) -> Result<(), BrowserPolicyError> {
    platform::apply(etc_root, disable_doh)
}

#[cfg(target_os = "linux")]
mod platform {
    use super::*;
    use std::fs;

    const CHROMIUM_TARGETS: &[&str] = &["opt/chrome", "opt/edge", "brave", "chromium"];
    const MANAGED_POLICY_FILENAME: &str = "homeguard.json";

    pub fn apply(etc_root: &Path, disable_doh: bool) -> Result<(), BrowserPolicyError> {
        for target in CHROMIUM_TARGETS {
            let path = etc_root
                .join(target)
                .join("policies/managed")
                .join(MANAGED_POLICY_FILENAME);
            if disable_doh {
                write_chromium_policy(&path)?;
            } else {
                remove_if_present(&path)?;
            }
        }
        merge_firefox_policy(&etc_root.join("firefox/policies/policies.json"), disable_doh)?;
        Ok(())
    }

    fn write_chromium_policy(path: &Path) -> Result<(), BrowserPolicyError> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let body = serde_json::json!({ "DnsOverHttpsMode": "off" });
        fs::write(path, serde_json::to_string_pretty(&body)?)?;
        Ok(())
    }

    fn remove_if_present(path: &Path) -> Result<(), BrowserPolicyError> {
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    /// Firefox has one shared `policies.json` — unlike the Chromium
    /// browsers we can't own a whole file, so we merge our one key in and
    /// out without touching anything else an admin (or another tool) put
    /// there.
    fn merge_firefox_policy(path: &Path, disable_doh: bool) -> Result<(), BrowserPolicyError> {
        let mut root: serde_json::Value = match fs::read_to_string(path) {
            Ok(content) => serde_json::from_str(&content).unwrap_or_else(|_| serde_json::json!({})),
            // Nothing to write: no file existed and we're only asked to
            // clear our key — don't create a policies.json out of thin
            // air on uninstall.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound && !disable_doh => return Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => serde_json::json!({}),
            Err(e) => return Err(e.into()),
        };

        let root_obj = root.as_object_mut().ok_or_else(|| {
            BrowserPolicyError::Io(std::io::Error::other("policies.json root is not an object"))
        })?;
        let policies = root_obj
            .entry("policies")
            .or_insert_with(|| serde_json::json!({}));
        let policies_obj = policies.as_object_mut().ok_or_else(|| {
            BrowserPolicyError::Io(std::io::Error::other("policies.policies is not an object"))
        })?;

        if disable_doh {
            policies_obj.insert(
                "DNSOverHTTPS".to_string(),
                serde_json::json!({ "Enabled": false, "Locked": true }),
            );
        } else {
            policies_obj.remove("DNSOverHTTPS");
        }

        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::write(path, serde_json::to_string_pretty(&root)?)?;
        Ok(())
    }
}

#[cfg(target_os = "windows")]
mod platform {
    //! UNVERIFIED — same caveat as `service.rs`: cannot compile or run on
    //! this Linux sandbox. Writes `DnsOverHttpsMode` under each browser's
    //! policy key in HKEY_LOCAL_MACHINE, which only an admin/SYSTEM can do
    //! — matching the plan's registry paths.
    use super::*;
    use winreg::enums::*;
    use winreg::RegKey;

    const CHROMIUM_KEYS: &[&str] = &[
        r"SOFTWARE\Policies\Google\Chrome",
        r"SOFTWARE\Policies\Microsoft\Edge",
        r"SOFTWARE\Policies\BraveSoftware\Brave",
        r"SOFTWARE\Policies\Chromium",
    ];

    pub fn apply(_etc_root: &Path, disable_doh: bool) -> Result<(), BrowserPolicyError> {
        let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
        for key_path in CHROMIUM_KEYS {
            if disable_doh {
                let (key, _) = hklm.create_subkey(key_path)?;
                key.set_value("DnsOverHttpsMode", &"off")?;
            } else if let Ok(key) = hklm.open_subkey_with_flags(key_path, KEY_SET_VALUE) {
                let _ = key.delete_value("DnsOverHttpsMode");
            }
        }

        let firefox_key = r"SOFTWARE\Policies\Mozilla\Firefox\DNSOverHTTPS";
        if disable_doh {
            let (key, _) = hklm.create_subkey(firefox_key)?;
            key.set_value("Enabled", &0u32)?;
            key.set_value("Locked", &1u32)?;
        } else if let Ok(key) = hklm.open_subkey_with_flags(firefox_key, KEY_SET_VALUE) {
            let _ = key.delete_value("Enabled");
            let _ = key.delete_value("Locked");
        }

        Ok(())
    }
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
mod platform {
    use super::*;

    pub fn apply(_etc_root: &Path, _disable_doh: bool) -> Result<(), BrowserPolicyError> {
        Ok(())
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn writes_chromium_and_firefox_policy_when_enabled() {
        let dir = tempfile::tempdir().unwrap();
        apply(dir.path(), true).unwrap();

        let chrome_policy =
            std::fs::read_to_string(dir.path().join("opt/chrome/policies/managed/homeguard.json")).unwrap();
        assert!(chrome_policy.contains(r#""DnsOverHttpsMode": "off""#));

        let firefox_policy =
            std::fs::read_to_string(dir.path().join("firefox/policies/policies.json")).unwrap();
        assert!(firefox_policy.contains("DNSOverHTTPS"));
        assert!(firefox_policy.contains(r#""Enabled": false"#));
    }

    #[test]
    fn removes_chromium_file_and_firefox_key_when_disabled() {
        let dir = tempfile::tempdir().unwrap();
        apply(dir.path(), true).unwrap();
        apply(dir.path(), false).unwrap();

        assert!(!dir.path().join("opt/chrome/policies/managed/homeguard.json").exists());
        let firefox_policy =
            std::fs::read_to_string(dir.path().join("firefox/policies/policies.json")).unwrap();
        assert!(!firefox_policy.contains("DNSOverHTTPS"));
    }

    #[test]
    fn preserves_unrelated_firefox_policies_already_present() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("firefox/policies/policies.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            r#"{"policies": {"DisableTelemetry": true}}"#,
        )
        .unwrap();

        apply(dir.path(), true).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("DisableTelemetry"));
        assert!(content.contains("DNSOverHTTPS"));
    }
}
