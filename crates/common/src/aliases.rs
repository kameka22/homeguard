//! Automatic domain variants: blocking "youtube.com" should also catch
//! "www.youtube.com", "m.youtube.com", etc. The app expands a list's
//! domains through this before building a policy; the agent never sees the
//! difference between a domain the admin typed and one added as a variant.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::hosts::normalize_domain;

/// One base domain's known variants, e.g. `youtube.com` ->
/// `[www.youtube.com, m.youtube.com, youtu.be, youtube-nocookie.com]`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AliasEntry {
    pub base_domain: String,
    pub variants: Vec<String>,
}

/// The built-in catalog. Kept small and editable — this is exactly the
/// `aliases.toml` referenced in PLAN.md, just embedded as Rust for now so
/// it ships with no extra file to lose track of; loading a user-edited
/// TOML file over this default is a straightforward follow-up.
pub fn default_catalog() -> Vec<AliasEntry> {
    let raw: &[(&str, &[&str])] = &[
        ("youtube.com", &["m.youtube.com", "youtu.be", "youtube-nocookie.com"]),
        ("tiktok.com", &["m.tiktok.com"]),
        ("instagram.com", &["www.instagram.com"]),
        ("facebook.com", &["m.facebook.com", "fb.com"]),
        ("twitter.com", &["mobile.twitter.com"]),
        ("x.com", &["mobile.x.com"]),
        ("twitch.tv", &["m.twitch.tv"]),
        ("roblox.com", &["www.roblox.com"]),
        ("snapchat.com", &["www.snapchat.com"]),
        ("reddit.com", &["m.reddit.com", "old.reddit.com"]),
        ("netflix.com", &["www.netflix.com"]),
        ("discord.com", &["ptb.discord.com", "canary.discord.com"]),
        ("linkedin.com", &["www.linkedin.com", "m.linkedin.com"]),
        ("pinterest.com", &["www.pinterest.com"]),
        ("whatsapp.com", &["www.whatsapp.com", "web.whatsapp.com"]),
        ("telegram.org", &["www.telegram.org", "web.telegram.org"]),
        ("threads.net", &["www.threads.net"]),
        ("bereal.com", &["www.bereal.com"]),
        ("tumblr.com", &["www.tumblr.com"]),
        ("mastodon.social", &["www.mastodon.social"]),
        ("vk.com", &["www.vk.com", "m.vk.com"]),
    ];
    raw.iter()
        .map(|(base, variants)| AliasEntry {
            base_domain: base.to_string(),
            variants: variants.iter().map(|v| v.to_string()).collect(),
        })
        .collect()
}

/// Expand `domains` with `www.<domain>` (when `include_www` is set) and any
/// variants found in `catalog` for a domain in the list. Domains not in the
/// catalog and without a `www.` variant are returned unchanged. The result
/// is de-duplicated and normalized.
pub fn expand_domains(domains: &[String], catalog: &[AliasEntry], include_www: bool) -> Vec<String> {
    let mut result: BTreeSet<String> = BTreeSet::new();
    for domain in domains {
        let normalized = normalize_domain(domain);
        result.insert(normalized.clone());

        if include_www && !normalized.starts_with("www.") {
            result.insert(format!("www.{normalized}"));
        }

        if let Some(entry) = catalog.iter().find(|e| e.base_domain == normalized) {
            for variant in &entry.variants {
                result.insert(normalize_domain(variant));
            }
        }
    }
    result.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn domains(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn expands_known_domain_with_catalog_variants_and_www() {
        let catalog = default_catalog();
        let expanded = expand_domains(&domains(&["youtube.com"]), &catalog, true);
        assert_eq!(
            expanded,
            domains(&[
                "m.youtube.com",
                "www.youtube.com",
                "youtu.be",
                "youtube-nocookie.com",
                "youtube.com",
            ])
        );
    }

    #[test]
    fn leaves_unknown_domain_alone_without_www() {
        let catalog = default_catalog();
        let expanded = expand_domains(&domains(&["example-unknown.org"]), &catalog, false);
        assert_eq!(expanded, domains(&["example-unknown.org"]));
    }

    #[test]
    fn deduplicates_across_multiple_inputs() {
        let catalog = default_catalog();
        let expanded = expand_domains(&domains(&["youtube.com", "www.youtube.com"]), &catalog, true);
        // www.youtube.com is both an explicit input and a derived variant —
        // must appear once.
        assert_eq!(expanded.iter().filter(|d| *d == "www.youtube.com").count(), 1);
    }
}
