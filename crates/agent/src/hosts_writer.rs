use std::path::Path;

use homeguard_common::hosts;

#[derive(Debug, thiserror::Error)]
pub enum HostsWriteError {
    #[error("failed to read {path}: {source}")]
    Read {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to write {path}: {source}")]
    Write {
        path: String,
        #[source]
        source: std::io::Error,
    },
}

/// Apply `domains` to the managed block in the hosts file at `path`,
/// atomically (write to a temp file in the same directory, then rename) and
/// with a one-time backup of the pre-agent file.
pub fn write_domains(path: &Path, domains: &[String]) -> Result<hosts::ApplyResult, HostsWriteError> {
    let original = match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(source) => {
            return Err(HostsWriteError::Read {
                path: path.display().to_string(),
                source,
            })
        }
    };

    backup_original_once(path, &original)?;

    let result = hosts::apply_block(&original, domains);
    atomic_write(path, &result.content)?;
    flush_dns_cache();
    Ok(result)
}

/// Read back the domains currently in the managed block, without writing
/// anything. Used by `/v1/status` to detect drift.
pub fn read_current_domains(path: &Path) -> Result<Option<Vec<String>>, HostsWriteError> {
    match std::fs::read_to_string(path) {
        Ok(content) => Ok(hosts::extract_block(&content)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(HostsWriteError::Read {
            path: path.display().to_string(),
            source,
        }),
    }
}

fn backup_original_once(path: &Path, original: &str) -> Result<(), HostsWriteError> {
    let backup_path = backup_path_for(path);
    if backup_path.exists() {
        return Ok(());
    }
    // Only worth backing up a file that doesn't already carry our block
    // (i.e. the pristine, pre-agent state).
    if hosts::extract_block(original).is_some() {
        return Ok(());
    }
    std::fs::write(&backup_path, original).map_err(|source| HostsWriteError::Write {
        path: backup_path.display().to_string(),
        source,
    })
}

fn backup_path_for(path: &Path) -> std::path::PathBuf {
    let mut name = path
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    name.push(".homeguard.bak");
    path.with_file_name(name)
}

fn atomic_write(path: &Path, content: &str) -> Result<(), HostsWriteError> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let tmp_path = dir.join(format!(
        ".{}.homeguard.tmp",
        path.file_name().and_then(|n| n.to_str()).unwrap_or("hosts")
    ));

    std::fs::write(&tmp_path, content).map_err(|source| HostsWriteError::Write {
        path: tmp_path.display().to_string(),
        source,
    })?;

    std::fs::rename(&tmp_path, path).map_err(|source| HostsWriteError::Write {
        path: path.display().to_string(),
        source,
    })
}

#[cfg(target_os = "windows")]
fn flush_dns_cache() {
    let _ = std::process::Command::new("ipconfig")
        .arg("/flushdns")
        .status();
}

#[cfg(target_os = "linux")]
fn flush_dns_cache() {
    let _ = std::process::Command::new("resolvectl")
        .arg("flush-caches")
        .status();
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
fn flush_dns_cache() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_domains_and_backs_up_original_once() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hosts");
        std::fs::write(&path, "127.0.0.1 localhost\n").unwrap();

        write_domains(&path, &["youtube.com".to_string()]).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("0.0.0.0 youtube.com"));

        let backup = std::fs::read_to_string(backup_path_for(&path)).unwrap();
        assert_eq!(backup, "127.0.0.1 localhost\n");

        // A second write must not clobber the backup with the now-managed
        // content.
        write_domains(&path, &["tiktok.com".to_string()]).unwrap();
        let backup_after = std::fs::read_to_string(backup_path_for(&path)).unwrap();
        assert_eq!(backup_after, "127.0.0.1 localhost\n");
    }

    #[test]
    fn read_current_domains_reflects_last_write() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hosts");
        write_domains(&path, &["youtube.com".to_string(), "tiktok.com".to_string()]).unwrap();

        let current = read_current_domains(&path).unwrap().unwrap();
        assert_eq!(current, vec!["youtube.com".to_string(), "tiktok.com".to_string()]);
    }

    #[test]
    fn read_current_domains_none_when_file_absent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hosts");
        assert_eq!(read_current_domains(&path).unwrap(), None);
    }
}
