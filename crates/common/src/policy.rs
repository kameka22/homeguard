//! The policy model sent to an agent: fully resolved rules (domains +
//! optional schedule) plus temporary exceptions. The agent only ever
//! evaluates this — it knows nothing about groups, lists or targets.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::hosts::normalize_domain;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Policy {
    pub version: u64,
    /// IANA timezone name (e.g. "Europe/Paris"). Schedules are always
    /// evaluated in this fixed timezone, never the host's system timezone,
    /// so a non-admin user changing their OS timezone can't dodge a
    /// schedule.
    pub timezone: String,
    pub rules: Vec<Rule>,
    #[serde(default)]
    pub exceptions: Vec<Exception>,
    #[serde(default)]
    pub protection: Protection,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rule {
    pub id: String,
    pub name: String,
    pub domains: Vec<String>,
    /// `None` means "always blocked". `Some(windows)` blocks only while at
    /// least one window matches the current day/time.
    #[serde(default)]
    pub schedule: Option<Vec<TimeWindow>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeWindow {
    pub days: Vec<Day>,
    pub from: jiff::civil::Time,
    pub to: jiff::civil::Time,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Exception {
    pub domains: Vec<String>,
    pub until: jiff::Timestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Protection {
    #[serde(default)]
    pub self_heal: bool,
    #[serde(default)]
    pub disable_doh: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Day {
    #[serde(rename = "mon")]
    Mon,
    #[serde(rename = "tue")]
    Tue,
    #[serde(rename = "wed")]
    Wed,
    #[serde(rename = "thu")]
    Thu,
    #[serde(rename = "fri")]
    Fri,
    #[serde(rename = "sat")]
    Sat,
    #[serde(rename = "sun")]
    Sun,
}

impl Day {
    fn from_jiff(weekday: jiff::civil::Weekday) -> Day {
        use jiff::civil::Weekday::*;
        match weekday {
            Monday => Day::Mon,
            Tuesday => Day::Tue,
            Wednesday => Day::Wed,
            Thursday => Day::Thu,
            Friday => Day::Fri,
            Saturday => Day::Sat,
            Sunday => Day::Sun,
        }
    }

    fn previous(self) -> Day {
        match self {
            Day::Mon => Day::Sun,
            Day::Tue => Day::Mon,
            Day::Wed => Day::Tue,
            Day::Thu => Day::Wed,
            Day::Fri => Day::Thu,
            Day::Sat => Day::Fri,
            Day::Sun => Day::Sat,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PolicyError {
    #[error("invalid or unknown timezone {timezone:?}: {source}")]
    InvalidTimezone {
        timezone: String,
        #[source]
        source: jiff::Error,
    },
}

impl TimeWindow {
    /// Whether this window covers `zoned`'s current day and time. Also
    /// tells you which side of a [`TimeWindow::next_transition`] instant
    /// you're looking at: the window's `[from, to)` is half-open, so
    /// evaluated exactly at a `from` instant this is `true` (block just
    /// started) and exactly at a `to` instant it's `false` (block just
    /// lifted).
    pub fn contains(&self, zoned: &jiff::Zoned) -> bool {
        let day = Day::from_jiff(zoned.weekday());
        let time = zoned.time();

        if self.from <= self.to {
            self.days.contains(&day) && time >= self.from && time < self.to
        } else {
            // Overnight window (e.g. 21:00 -> 07:00): matches either the
            // start day after `from`, or the following day before `to`.
            let previous_day = day.previous();
            (self.days.contains(&day) && time >= self.from)
                || (self.days.contains(&previous_day) && time < self.to)
        }
    }

    /// The next instant, strictly after `at`, at which this window starts
    /// or stops covering the timeline — i.e. the next time it would flip
    /// whether a domain it names is blocked. `None` if `days` is empty (the
    /// window can never be active) or on a timezone arithmetic failure.
    ///
    /// Scans from the day before `at` (to catch an overnight window that
    /// started yesterday and ends today) through the next 9 days — more
    /// than enough to always find the next occurrence of any weekday.
    pub fn next_transition(&self, at: &jiff::Zoned) -> Option<jiff::Zoned> {
        if self.days.is_empty() {
            return None;
        }
        let tz = at.time_zone().clone();
        let mut best: Option<jiff::Zoned> = None;
        let mut date = at.date().yesterday().ok()?;

        for _ in 0..10 {
            if self.days.contains(&Day::from_jiff(date.weekday())) {
                let start = date
                    .at(self.from.hour(), self.from.minute(), self.from.second(), 0)
                    .to_zoned(tz.clone())
                    .ok()?;
                let end_date = if self.from <= self.to { date } else { date.tomorrow().ok()? };
                let end = end_date
                    .at(self.to.hour(), self.to.minute(), self.to.second(), 0)
                    .to_zoned(tz.clone())
                    .ok()?;

                for candidate in [start, end] {
                    if &candidate > at {
                        best = Some(match best {
                            Some(b) if b < candidate => b,
                            _ => candidate,
                        });
                    }
                }
            }
            date = date.tomorrow().ok()?;
        }

        best
    }
}

impl Rule {
    fn is_active_at(&self, zoned: &jiff::Zoned) -> bool {
        match &self.schedule {
            None => true,
            Some(windows) => windows.iter().any(|w| w.contains(zoned)),
        }
    }
}

impl Policy {
    /// Resolve the set of domains that must be blocked at `instant`,
    /// evaluated in the policy's fixed timezone.
    pub fn effective_domains_at(&self, instant: jiff::Timestamp) -> Result<BTreeSet<String>, PolicyError> {
        let zoned = instant
            .in_tz(&self.timezone)
            .map_err(|source| PolicyError::InvalidTimezone {
                timezone: self.timezone.clone(),
                source,
            })?;

        let mut domains: BTreeSet<String> = BTreeSet::new();
        for rule in &self.rules {
            if rule.is_active_at(&zoned) {
                domains.extend(rule.domains.iter().map(|d| normalize_domain(d)));
            }
        }

        for exception in &self.exceptions {
            if exception.until > instant {
                for domain in &exception.domains {
                    domains.remove(&normalize_domain(domain));
                }
            }
        }

        Ok(domains)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::time;

    fn always(id: &str, domains: &[&str]) -> Rule {
        Rule {
            id: id.to_string(),
            name: id.to_string(),
            domains: domains.iter().map(|s| s.to_string()).collect(),
            schedule: None,
        }
    }

    fn scheduled(id: &str, domains: &[&str], days: &[Day], from: jiff::civil::Time, to: jiff::civil::Time) -> Rule {
        Rule {
            id: id.to_string(),
            name: id.to_string(),
            domains: domains.iter().map(|s| s.to_string()).collect(),
            schedule: Some(vec![TimeWindow {
                days: days.to_vec(),
                from,
                to,
            }]),
        }
    }

    fn set(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    fn paris_instant(y: i16, m: i8, d: i8, h: i8, min: i8) -> jiff::Timestamp {
        paris_zoned(y, m, d, h, min).timestamp()
    }

    fn paris_zoned(y: i16, m: i8, d: i8, h: i8, min: i8) -> jiff::Zoned {
        jiff::civil::date(y, m, d).at(h, min, 0, 0).in_tz("Europe/Paris").unwrap()
    }

    #[test]
    fn always_rule_is_always_active() {
        let policy = Policy {
            version: 1,
            timezone: "Europe/Paris".into(),
            rules: vec![always("social", &["tiktok.com"])],
            exceptions: vec![],
            protection: Protection::default(),
        };
        let now = paris_instant(2026, 9, 24, 3, 0);
        assert_eq!(
            policy.effective_domains_at(now).unwrap(),
            set(&["tiktok.com"])
        );
    }

    #[test]
    fn weekday_schedule_blocks_only_inside_window() {
        // Games blocked Mon-Fri 08:00-18:00.
        let policy = Policy {
            version: 1,
            timezone: "Europe/Paris".into(),
            rules: vec![scheduled(
                "games",
                &["roblox.com"],
                &[Day::Mon, Day::Tue, Day::Wed, Day::Thu, Day::Fri],
                time(8, 0, 0, 0),
                time(18, 0, 0, 0),
            )],
            exceptions: vec![],
            protection: Protection::default(),
        };

        // Thursday 2026-09-24, 10:00 -> inside window.
        assert_eq!(
            policy.effective_domains_at(paris_instant(2026, 9, 24, 10, 0)).unwrap(),
            set(&["roblox.com"])
        );
        // Thursday 2026-09-24, 20:00 -> outside window.
        assert_eq!(
            policy.effective_domains_at(paris_instant(2026, 9, 24, 20, 0)).unwrap(),
            set(&[])
        );
        // Saturday 2026-09-26, 10:00 -> day not in schedule.
        assert_eq!(
            policy.effective_domains_at(paris_instant(2026, 9, 26, 10, 0)).unwrap(),
            set(&[])
        );
    }

    #[test]
    fn overnight_window_wraps_past_midnight() {
        // Streaming blocked every night 21:00 -> 07:00.
        let policy = Policy {
            version: 1,
            timezone: "Europe/Paris".into(),
            rules: vec![scheduled(
                "streaming",
                &["twitch.tv"],
                &[Day::Mon, Day::Tue, Day::Wed, Day::Thu, Day::Fri, Day::Sat, Day::Sun],
                time(21, 0, 0, 0),
                time(7, 0, 0, 0),
            )],
            exceptions: vec![],
            protection: Protection::default(),
        };

        // 23:30 the same day -> blocked.
        assert_eq!(
            policy.effective_domains_at(paris_instant(2026, 9, 24, 23, 30)).unwrap(),
            set(&["twitch.tv"])
        );
        // 03:00 the next calendar day -> still blocked (window carried over).
        assert_eq!(
            policy.effective_domains_at(paris_instant(2026, 9, 25, 3, 0)).unwrap(),
            set(&["twitch.tv"])
        );
        // 12:00 daytime -> not blocked.
        assert_eq!(
            policy.effective_domains_at(paris_instant(2026, 9, 25, 12, 0)).unwrap(),
            set(&[])
        );
    }

    #[test]
    fn next_transition_finds_the_next_start_or_end_for_a_weekday_window() {
        let window = TimeWindow {
            days: vec![Day::Mon, Day::Tue, Day::Wed, Day::Thu, Day::Fri],
            from: time(8, 0, 0, 0),
            to: time(18, 0, 0, 0),
        };

        // Thursday 10:00, inside the window -> next transition is today's end.
        let next = window.next_transition(&paris_zoned(2026, 9, 24, 10, 0)).unwrap();
        assert_eq!(next, paris_zoned(2026, 9, 24, 18, 0));

        // Thursday 20:00, outside -> next transition is tomorrow's start.
        let next = window.next_transition(&paris_zoned(2026, 9, 24, 20, 0)).unwrap();
        assert_eq!(next, paris_zoned(2026, 9, 25, 8, 0));

        // Saturday -> next transition skips the weekend to Monday's start.
        let next = window.next_transition(&paris_zoned(2026, 9, 26, 10, 0)).unwrap();
        assert_eq!(next, paris_zoned(2026, 9, 28, 8, 0));
    }

    #[test]
    fn next_transition_handles_an_overnight_window() {
        let window = TimeWindow {
            days: vec![Day::Mon, Day::Tue, Day::Wed, Day::Thu, Day::Fri, Day::Sat, Day::Sun],
            from: time(21, 0, 0, 0),
            to: time(7, 0, 0, 0),
        };

        // Inside last night's carried-over window -> next transition is
        // this morning's end.
        let next = window.next_transition(&paris_zoned(2026, 9, 25, 3, 0)).unwrap();
        assert_eq!(next, paris_zoned(2026, 9, 25, 7, 0));

        // Daytime, before tonight's window -> next transition is tonight's
        // start.
        let next = window.next_transition(&paris_zoned(2026, 9, 25, 12, 0)).unwrap();
        assert_eq!(next, paris_zoned(2026, 9, 25, 21, 0));
    }

    #[test]
    fn next_transition_is_none_for_an_empty_day_list() {
        let window = TimeWindow {
            days: vec![],
            from: time(8, 0, 0, 0),
            to: time(18, 0, 0, 0),
        };
        assert!(window.next_transition(&paris_zoned(2026, 9, 24, 10, 0)).is_none());
    }

    #[test]
    fn active_exception_lifts_a_block() {
        let policy = Policy {
            version: 1,
            timezone: "Europe/Paris".into(),
            rules: vec![always("social", &["twitch.tv", "tiktok.com"])],
            exceptions: vec![Exception {
                domains: vec!["twitch.tv".into()],
                until: paris_instant(2026, 9, 24, 20, 15),
            }],
            protection: Protection::default(),
        };

        // Before expiry: twitch.tv unblocked, tiktok.com still blocked.
        assert_eq!(
            policy.effective_domains_at(paris_instant(2026, 9, 24, 19, 0)).unwrap(),
            set(&["tiktok.com"])
        );
        // After expiry: exception no longer applies.
        assert_eq!(
            policy.effective_domains_at(paris_instant(2026, 9, 24, 20, 30)).unwrap(),
            set(&["tiktok.com", "twitch.tv"])
        );
    }

    #[test]
    fn schedule_survives_dst_spring_forward() {
        // France switches to summer time on 2026-03-29 at 02:00 -> 03:00.
        // A schedule expressed in civil time (08:00-18:00, Europe/Paris)
        // must keep meaning "8am to 6pm local time" across the jump.
        let policy = Policy {
            version: 1,
            timezone: "Europe/Paris".into(),
            rules: vec![scheduled(
                "games",
                &["roblox.com"],
                &[Day::Sun],
                time(8, 0, 0, 0),
                time(18, 0, 0, 0),
            )],
            exceptions: vec![],
            protection: Protection::default(),
        };

        // Sunday 2026-03-29, 09:00 local (after the spring-forward jump).
        assert_eq!(
            policy.effective_domains_at(paris_instant(2026, 3, 29, 9, 0)).unwrap(),
            set(&["roblox.com"])
        );
        // Same Sunday, 19:00 local -> outside window.
        assert_eq!(
            policy.effective_domains_at(paris_instant(2026, 3, 29, 19, 0)).unwrap(),
            set(&[])
        );
    }

    #[test]
    fn rejects_unknown_timezone() {
        let policy = Policy {
            version: 1,
            timezone: "Not/AZone".into(),
            rules: vec![],
            exceptions: vec![],
            protection: Protection::default(),
        };
        assert!(policy.effective_domains_at(jiff::Timestamp::now()).is_err());
    }

    #[test]
    fn policy_roundtrips_through_json() {
        let policy = Policy {
            version: 14,
            timezone: "Europe/Paris".into(),
            rules: vec![scheduled(
                "games",
                &["roblox.com"],
                &[Day::Mon, Day::Tue],
                time(8, 0, 0, 0),
                time(18, 0, 0, 0),
            )],
            exceptions: vec![Exception {
                domains: vec!["twitch.tv".into()],
                until: paris_instant(2026, 9, 24, 20, 15),
            }],
            protection: Protection {
                self_heal: true,
                disable_doh: true,
            },
        };
        let json = serde_json::to_string(&policy).unwrap();
        let back: Policy = serde_json::from_str(&json).unwrap();
        assert_eq!(policy, back);
    }
}
