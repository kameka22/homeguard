use sha2::{Digest, Sha256};

#[derive(Debug, thiserror::Error)]
pub enum UpdateError {
    #[error("checksum mismatch: expected {expected}, got {actual}")]
    ChecksumMismatch { expected: String, actual: String },
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub fn sha256_hex(data: &[u8]) -> String {
    Sha256::digest(data).iter().map(|b| format!("{b:02x}")).collect()
}

/// Write `new_binary` to a temp file and replace the currently running
/// executable with it. The process must exit shortly after this returns
/// (the caller does so once the HTTP response is flushed) so the service
/// manager (`Restart=always` / `sc failure ... restart`) brings it back up
/// running the new binary.
pub fn apply_update(new_binary: &[u8], expected_sha256_hex: &str) -> Result<(), UpdateError> {
    let actual = sha256_hex(new_binary);
    let expected = expected_sha256_hex.trim().to_ascii_lowercase();
    if actual != expected {
        return Err(UpdateError::ChecksumMismatch { expected, actual });
    }

    let tmp_path = std::env::temp_dir().join(format!("homeguard-agent-update-{}", uuid::Uuid::new_v4()));
    std::fs::write(&tmp_path, new_binary)?;
    make_executable(&tmp_path)?;

    self_replace::self_replace(&tmp_path)?;
    let _ = std::fs::remove_file(&tmp_path);
    Ok(())
}

#[cfg(unix)]
fn make_executable(path: &std::path::Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
}

#[cfg(not(unix))]
fn make_executable(_path: &std::path::Path) -> std::io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_a_binary_that_does_not_match_the_expected_checksum() {
        let result = apply_update(b"not the real binary", "0000000000000000000000000000000000000000000000000000000000000000");
        assert!(matches!(result, Err(UpdateError::ChecksumMismatch { .. })));
    }

    #[test]
    fn sha256_hex_is_stable_and_lowercase() {
        let hash = sha256_hex(b"hello");
        assert_eq!(hash.len(), 64);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    }
}
