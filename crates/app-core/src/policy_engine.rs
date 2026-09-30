//! Turns groups + lists + assignments + exceptions stored in the DB into
//! the flat, fully-resolved [`homeguard_common::Policy`] pushed to one
//! agent. The agent never needs to know what a "group" or a "list" is.

use homeguard_common::policy::{Exception as PolicyException, Policy, Protection, Rule};
use homeguard_common::{default_catalog, expand_domains};

use crate::db::{Db, DbError};

const INCLUDE_WWW: bool = true;

#[derive(Debug, thiserror::Error)]
pub enum PolicyEngineError {
    #[error(transparent)]
    Db(#[from] DbError),
}

/// Build the policy `agent_id` should be running, at the version the
/// caller assigns (see [`Db::bump_policy_version`]).
pub fn resolve_policy_for_agent(
    db: &Db,
    agent_id: &str,
    version: u64,
    timezone: &str,
    self_heal: bool,
    disable_doh: bool,
) -> Result<Policy, PolicyEngineError> {
    let group_ids = db.agent_group_ids(agent_id)?;
    let catalog = default_catalog();

    let assignments = db.assignments_for(agent_id, &group_ids)?;
    let mut rules = Vec::with_capacity(assignments.len());
    for assignment in assignments {
        let Some(list) = db.get_list(&assignment.list_id)? else {
            continue;
        };
        rules.push(Rule {
            id: assignment.id,
            name: list.name,
            domains: expand_domains(&list.domains, &catalog, INCLUDE_WWW),
            schedule: assignment.schedule,
        });
    }

    let exceptions = db
        .exceptions_for(agent_id, &group_ids)?
        .into_iter()
        .map(|e| PolicyException {
            domains: e.domains,
            until: e.until,
        })
        .collect();

    Ok(Policy {
        version,
        timezone: timezone.to_string(),
        rules,
        exceptions,
        protection: Protection { self_heal, disable_doh },
    })
}

/// Convenience for a fresh enrollment: same resolution, but always bumps
/// the agent's version counter first so the very first policy it receives
/// is already versioned consistently with everything pushed afterwards.
pub fn resolve_initial_policy_for_agent(
    db: &Db,
    agent_id: &str,
    timezone: &str,
    self_heal: bool,
    disable_doh: bool,
) -> Result<Policy, PolicyEngineError> {
    let version = db.bump_policy_version(agent_id)?;
    resolve_policy_for_agent(db, agent_id, version, timezone, self_heal, disable_doh)
}

pub fn target_agent_and_groups(db: &Db, agent_id: &str) -> Result<(String, Vec<String>), PolicyEngineError> {
    Ok((agent_id.to_string(), db.agent_group_ids(agent_id)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::TargetKind;

    #[test]
    fn resolves_expanded_domains_and_schedule_from_group_assignment() {
        let db = Db::open_in_memory().unwrap();
        db.upsert_agent("a1", "PORTABLE-LEO", "windows", "x86_64", None, None).unwrap();
        let group_id = db.create_group("Enfants").unwrap();
        db.add_group_member(&group_id, "a1").unwrap();

        let games = db.create_list("Jeux", &["roblox.com".to_string()]).unwrap();
        let window = homeguard_common::policy::TimeWindow {
            days: vec![homeguard_common::Day::Mon, homeguard_common::Day::Tue],
            from: jiff::civil::time(8, 0, 0, 0),
            to: jiff::civil::time(18, 0, 0, 0),
        };
        db.create_assignment(&games, TargetKind::Group, &group_id, Some(&[window]))
            .unwrap();

        let policy = resolve_policy_for_agent(&db, "a1", 1, "Europe/Paris", true, true).unwrap();
        assert_eq!(policy.rules.len(), 1);
        assert_eq!(policy.rules[0].domains, vec!["roblox.com".to_string(), "www.roblox.com".to_string()]);
        assert!(policy.rules[0].schedule.is_some());
        assert!(policy.protection.self_heal);
        assert!(policy.protection.disable_doh);
    }

    #[test]
    fn combines_direct_and_group_assignments_and_applies_exceptions() {
        let db = Db::open_in_memory().unwrap();
        db.upsert_agent("a1", "PORTABLE-LEO", "windows", "x86_64", None, None).unwrap();
        let group_id = db.create_group("Enfants").unwrap();
        db.add_group_member(&group_id, "a1").unwrap();

        let social = db.create_list("Réseaux sociaux", &["tiktok.com".to_string()]).unwrap();
        let streaming = db.create_list("Streaming", &["twitch.tv".to_string()]).unwrap();
        db.create_assignment(&social, TargetKind::Group, &group_id, None).unwrap();
        db.create_assignment(&streaming, TargetKind::Agent, "a1", None).unwrap();

        let until = jiff::Timestamp::now() + jiff::SignedDuration::from_secs(1800);
        db.create_exception(TargetKind::Agent, "a1", &["twitch.tv".to_string()], until).unwrap();

        let policy = resolve_policy_for_agent(&db, "a1", 1, "Europe/Paris", false, false).unwrap();
        assert_eq!(policy.rules.len(), 2);
        assert_eq!(policy.exceptions.len(), 1);

        let now = jiff::Timestamp::now();
        let effective = policy.effective_domains_at(now).unwrap();
        assert!(effective.contains("tiktok.com"));
        assert!(!effective.contains("twitch.tv"));
    }

    #[test]
    fn resolve_initial_policy_bumps_version() {
        let db = Db::open_in_memory().unwrap();
        db.upsert_agent("a1", "PORTABLE-LEO", "windows", "x86_64", None, None).unwrap();
        let policy = resolve_initial_policy_for_agent(&db, "a1", "UTC", true, false).unwrap();
        assert_eq!(policy.version, 1);
        let policy2 = resolve_initial_policy_for_agent(&db, "a1", "UTC", true, false).unwrap();
        assert_eq!(policy2.version, 2);
    }
}
