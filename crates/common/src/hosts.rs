//! Parsing and rendering of the managed block inside a `hosts` file.
//!
//! Everything outside `# BEGIN HOMEGUARD` … `# END HOMEGUARD` is left
//! untouched, except lines that would otherwise conflict with a domain we
//! manage — those get commented out so our block stays authoritative.

pub const BEGIN_MARKER: &str = "# BEGIN HOMEGUARD";
pub const END_MARKER: &str = "# END HOMEGUARD";
const CONFLICT_PREFIX: &str = "# [homeguard] désactivé: ";
const BLOCK_IP: &str = "0.0.0.0";

/// Result of applying a domain list to an existing `hosts` file content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplyResult {
    pub content: String,
    /// Lines outside the managed block that were commented out because they
    /// redefined one of our managed domains.
    pub conflicts_disabled: Vec<String>,
    /// Entries from the input that were dropped instead of being written,
    /// because they weren't a well-formed domain (e.g. contained whitespace
    /// or a newline) — accepting them verbatim would let one malformed
    /// entry inject an arbitrary extra line into the hosts file.
    pub rejected: Vec<String>,
}

/// Whether `domain` is safe to embed as-is in a `hosts` file line: only
/// ASCII letters, digits, `.` and `-`. In particular this rejects any
/// whitespace or control character, so a malformed or malicious domain
/// string can never inject a newline (and therefore an extra line) into
/// the file.
pub fn is_valid_domain(domain: &str) -> bool {
    !domain.is_empty()
        && domain.len() <= 253
        && domain
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
}

/// Replace (or append) the managed block in `original` so it contains
/// exactly `domains`, preserving the rest of the file and its line-ending
/// style.
pub fn apply_block(original: &str, domains: &[String]) -> ApplyResult {
    let eol = detect_eol(original);
    let had_trailing_newline = ends_with_newline(original);

    let raw_lines: Vec<&str> = split_lines(original);
    let (before, after, block_was_present) = split_around_block(&raw_lines);

    let mut rejected = Vec::new();
    let managed: std::collections::BTreeSet<String> = domains
        .iter()
        .map(|d| normalize_domain(d))
        .filter(|d| {
            let ok = is_valid_domain(d);
            if !ok {
                rejected.push(d.clone());
            }
            ok
        })
        .collect();

    let mut conflicts_disabled = Vec::new();
    let mut out_lines: Vec<String> = Vec::new();

    for line in before.iter().chain(after.iter()) {
        match disable_if_conflicting(line, &managed) {
            Some(disabled) => {
                conflicts_disabled.push((*line).to_string());
                out_lines.push(disabled);
            }
            None => out_lines.push((*line).to_string()),
        }
    }

    // Re-insert the block: at its original position if it existed, otherwise
    // appended at the end.
    let block_lines = render_block_lines(domains);
    if block_was_present {
        let insert_at = before.len();
        for (offset, line) in block_lines.iter().enumerate() {
            out_lines.insert(insert_at + offset, line.clone());
        }
    } else {
        if !out_lines.is_empty() && !out_lines.last().unwrap().is_empty() {
            out_lines.push(String::new());
        }
        out_lines.extend(block_lines);
    }

    let mut content = out_lines.join(eol);
    if had_trailing_newline || original.is_empty() {
        content.push_str(eol);
    }

    ApplyResult {
        content,
        conflicts_disabled,
        rejected,
    }
}

/// Extract the list of domains currently present in the managed block, if
/// any. Returns `None` when no (or a corrupted) block is found.
pub fn extract_block(content: &str) -> Option<Vec<String>> {
    let raw_lines = split_lines(content);
    let begin_idx = raw_lines.iter().position(|l| l.trim() == BEGIN_MARKER)?;
    let end_idx = raw_lines[begin_idx..]
        .iter()
        .position(|l| l.trim() == END_MARKER)
        .map(|rel| begin_idx + rel)?;

    let domains = raw_lines[begin_idx + 1..end_idx]
        .iter()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            let mut parts = line.split_whitespace();
            let ip = parts.next()?;
            let host = parts.next()?;
            if ip == BLOCK_IP {
                Some(host.to_string())
            } else {
                None
            }
        })
        .collect();

    Some(domains)
}

fn render_block_lines(domains: &[String]) -> Vec<String> {
    let mut lines = Vec::with_capacity(domains.len() + 2);
    lines.push(BEGIN_MARKER.to_string());
    for domain in domains {
        let normalized = normalize_domain(domain);
        // Defense in depth: `apply_block` already filters the `managed`
        // set through `is_valid_domain`, but this function is the actual
        // point where a string is embedded into a hosts-file line, so it
        // must never trust its input either.
        if is_valid_domain(&normalized) {
            lines.push(format!("{BLOCK_IP} {normalized}"));
        }
    }
    lines.push(END_MARKER.to_string());
    lines
}

/// Split `raw_lines` into (lines before the block, lines after the block,
/// whether a block was found). When a `BEGIN` marker is found without a
/// matching `END` (corrupted block), everything from `BEGIN` to the end of
/// the file is treated as part of the (discarded) block.
fn split_around_block<'a>(raw_lines: &[&'a str]) -> (Vec<&'a str>, Vec<&'a str>, bool) {
    let Some(begin_idx) = raw_lines.iter().position(|l| l.trim() == BEGIN_MARKER) else {
        return (raw_lines.to_vec(), Vec::new(), false);
    };
    let end_idx = raw_lines[begin_idx..]
        .iter()
        .position(|l| l.trim() == END_MARKER)
        .map(|rel| begin_idx + rel);

    let before = raw_lines[..begin_idx].to_vec();
    let after = match end_idx {
        Some(end_idx) => raw_lines[end_idx + 1..].to_vec(),
        None => Vec::new(),
    };
    (before, after, true)
}

/// If `line` is an active (non-comment) hosts entry that names one of the
/// domains we manage, return a commented-out version of it.
fn disable_if_conflicting(line: &str, managed: &std::collections::BTreeSet<String>) -> Option<String> {
    let trimmed = line.trim_start();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }
    let mut parts = trimmed.split_whitespace();
    let _ip = parts.next()?;
    let conflicts = parts.any(|host| managed.contains(&normalize_domain(host)));
    if conflicts {
        Some(format!("{CONFLICT_PREFIX}{line}"))
    } else {
        None
    }
}

pub fn normalize_domain(domain: &str) -> String {
    domain.trim().trim_end_matches('.').to_ascii_lowercase()
}

fn detect_eol(content: &str) -> &'static str {
    if content.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}

fn ends_with_newline(content: &str) -> bool {
    content.ends_with('\n')
}

/// Split into lines without the trailing `\r`, regardless of the original
/// line-ending style.
fn split_lines(content: &str) -> Vec<&str> {
    if content.is_empty() {
        return Vec::new();
    }
    content
        .split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .collect::<Vec<_>>()
        .split_last()
        .map(|(last, rest)| {
            // Drop a trailing empty element produced by a final newline.
            if last.is_empty() {
                rest.to_vec()
            } else {
                let mut v = rest.to_vec();
                v.push(last);
                v
            }
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn domains(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn appends_block_to_file_without_one() {
        let original = "127.0.0.1 localhost\n";
        let result = apply_block(original, &domains(&["youtube.com", "tiktok.com"]));
        assert_eq!(
            result.content,
            "127.0.0.1 localhost\n\n# BEGIN HOMEGUARD\n0.0.0.0 youtube.com\n0.0.0.0 tiktok.com\n# END HOMEGUARD\n"
        );
        assert!(result.conflicts_disabled.is_empty());
    }

    #[test]
    fn appends_block_to_empty_file() {
        let result = apply_block("", &domains(&["youtube.com"]));
        assert_eq!(
            result.content,
            "# BEGIN HOMEGUARD\n0.0.0.0 youtube.com\n# END HOMEGUARD\n"
        );
    }

    #[test]
    fn replaces_existing_block_in_place() {
        let original = "127.0.0.1 localhost\n\n# BEGIN HOMEGUARD\n0.0.0.0 old.com\n# END HOMEGUARD\n\n192.168.1.1 router.local\n";
        let result = apply_block(original, &domains(&["new.com"]));
        assert_eq!(
            result.content,
            "127.0.0.1 localhost\n\n# BEGIN HOMEGUARD\n0.0.0.0 new.com\n# END HOMEGUARD\n\n192.168.1.1 router.local\n"
        );
    }

    #[test]
    fn preserves_crlf_line_endings() {
        let original = "127.0.0.1 localhost\r\n";
        let result = apply_block(original, &domains(&["youtube.com"]));
        assert_eq!(
            result.content,
            "127.0.0.1 localhost\r\n\r\n# BEGIN HOMEGUARD\r\n0.0.0.0 youtube.com\r\n# END HOMEGUARD\r\n"
        );
    }

    #[test]
    fn handles_corrupted_block_missing_end_marker() {
        // BEGIN present but END marker missing/removed by hand: everything
        // from BEGIN to EOF is treated as the (discarded) old block.
        let original = "127.0.0.1 localhost\n\n# BEGIN HOMEGUARD\n0.0.0.0 old.com\n";
        let result = apply_block(original, &domains(&["new.com"]));
        assert_eq!(
            result.content,
            "127.0.0.1 localhost\n\n# BEGIN HOMEGUARD\n0.0.0.0 new.com\n# END HOMEGUARD\n"
        );
    }

    #[test]
    fn comments_out_conflicting_lines_outside_block() {
        let original = "1.2.3.4 youtube.com\n127.0.0.1 localhost\n";
        let result = apply_block(original, &domains(&["youtube.com"]));
        assert_eq!(
            result.content,
            "# [homeguard] désactivé: 1.2.3.4 youtube.com\n127.0.0.1 localhost\n\n# BEGIN HOMEGUARD\n0.0.0.0 youtube.com\n# END HOMEGUARD\n"
        );
        assert_eq!(result.conflicts_disabled, vec!["1.2.3.4 youtube.com"]);
    }

    #[test]
    fn preserves_a_legitimate_leading_blank_line() {
        let original = "\n127.0.0.1 localhost\n";
        let result = apply_block(original, &domains(&["youtube.com"]));
        assert_eq!(
            result.content,
            "\n127.0.0.1 localhost\n\n# BEGIN HOMEGUARD\n0.0.0.0 youtube.com\n# END HOMEGUARD\n"
        );
    }

    #[test]
    fn rejects_a_domain_with_an_embedded_newline_instead_of_injecting_a_line() {
        let original = "127.0.0.1 localhost\n";
        let malicious = "evil.com\n0.0.0.0 good-site.com";
        let result = apply_block(original, &domains(&[malicious, "tiktok.com"]));

        // The malicious entry must not appear anywhere in the output...
        assert!(!result.content.contains("good-site.com"));
        assert!(!result.content.contains(malicious));
        // ...only the well-formed domain gets written...
        assert_eq!(
            result.content,
            "127.0.0.1 localhost\n\n# BEGIN HOMEGUARD\n0.0.0.0 tiktok.com\n# END HOMEGUARD\n"
        );
        // ...and the caller can see it was dropped, instead of it
        // silently vanishing.
        assert_eq!(result.rejected, vec![malicious.to_string()]);
    }

    #[test]
    fn rejects_a_domain_with_embedded_whitespace() {
        let result = apply_block("", &domains(&["good.com evil.com"]));
        assert!(!result.content.contains("evil.com"));
        assert_eq!(result.rejected, vec!["good.com evil.com".to_string()]);
    }

    #[test]
    fn is_valid_domain_accepts_ordinary_domains_and_rejects_control_characters() {
        assert!(is_valid_domain("youtube.com"));
        assert!(is_valid_domain("sub.example-site.co.uk"));
        assert!(!is_valid_domain(""));
        assert!(!is_valid_domain("evil.com\n0.0.0.0 good.com"));
        assert!(!is_valid_domain("has space.com"));
        assert!(!is_valid_domain("tab\tcom"));
    }

    #[test]
    fn does_not_double_comment_already_commented_conflicts() {
        let original = "# [homeguard] désactivé: 1.2.3.4 youtube.com\n";
        let result = apply_block(original, &domains(&["youtube.com"]));
        assert!(result.conflicts_disabled.is_empty());
        assert!(result
            .content
            .contains("# [homeguard] désactivé: 1.2.3.4 youtube.com\n"));
    }

    #[test]
    fn extract_block_reads_current_domains() {
        let content = "127.0.0.1 localhost\n\n# BEGIN HOMEGUARD\n0.0.0.0 youtube.com\n0.0.0.0 tiktok.com\n# END HOMEGUARD\n";
        assert_eq!(
            extract_block(content),
            Some(domains(&["youtube.com", "tiktok.com"]))
        );
    }

    #[test]
    fn extract_block_returns_none_without_block() {
        let content = "127.0.0.1 localhost\n";
        assert_eq!(extract_block(content), None);
    }

    #[test]
    fn extract_block_returns_none_when_corrupted() {
        let content = "# BEGIN HOMEGUARD\n0.0.0.0 youtube.com\n";
        assert_eq!(extract_block(content), None);
    }

    #[test]
    fn idempotent_reapplication_yields_same_content() {
        let original = "127.0.0.1 localhost\n";
        let first = apply_block(original, &domains(&["a.com", "b.com"]));
        let second = apply_block(&first.content, &domains(&["a.com", "b.com"]));
        assert_eq!(first.content, second.content);
    }

    #[test]
    fn normalizes_domain_case_and_trailing_dot() {
        assert_eq!(normalize_domain("YouTube.com."), "youtube.com");
    }
}
