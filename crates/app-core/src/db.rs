//! SQLite storage for everything the app owns: agents, groups, lists,
//! assignments, exceptions and history. The agent never sees any of this —
//! it only ever receives the fully-resolved [`homeguard_common::Policy`]
//! the [`crate::policy_engine`] builds from it.

use std::sync::Mutex;

use homeguard_common::AliasEntry;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, DbError>;

pub struct Db {
    conn: Mutex<Connection>,
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS agents (
    id            TEXT PRIMARY KEY,
    display_name  TEXT,
    hostname      TEXT NOT NULL,
    os            TEXT NOT NULL,
    -- std::env::consts::ARCH as reported by the agent (e.g. "x86_64",
    -- "aarch64"). `os` alone can't tell the two Linux agent binaries
    -- apart, which a remote update needs to pick the right one.
    arch          TEXT NOT NULL DEFAULT 'unknown',
    last_ip       TEXT,
    cert_serial   TEXT,
    revoked       INTEGER NOT NULL DEFAULT 0,
    last_seen_at  TEXT,
    agent_version TEXT,
    policy_version INTEGER NOT NULL DEFAULT 0,
    -- Next `seq` to request from this agent's `/v1/events?since=`, so a
    -- sync only pulls what's new since the last one (agents keep their
    -- own journal; nothing is lost if the app was closed in between).
    events_cursor INTEGER NOT NULL DEFAULT 1
);

CREATE TABLE IF NOT EXISTS groups (
    id   TEXT PRIMARY KEY,
    name TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS group_members (
    group_id TEXT NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
    agent_id TEXT NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
    PRIMARY KEY (group_id, agent_id)
);

CREATE TABLE IF NOT EXISTS lists (
    id           TEXT PRIMARY KEY,
    name         TEXT NOT NULL,
    domains_json TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS assignments (
    id            TEXT PRIMARY KEY,
    list_id       TEXT NOT NULL REFERENCES lists(id) ON DELETE CASCADE,
    target_kind   TEXT NOT NULL CHECK (target_kind IN ('agent', 'group')),
    target_id     TEXT NOT NULL,
    schedule_json TEXT
);

CREATE TABLE IF NOT EXISTS exceptions (
    id           TEXT PRIMARY KEY,
    target_kind  TEXT NOT NULL CHECK (target_kind IN ('agent', 'group')),
    target_id    TEXT NOT NULL,
    domains_json TEXT NOT NULL,
    until        TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS history (
    id       TEXT PRIMARY KEY,
    at       TEXT NOT NULL,
    source   TEXT NOT NULL CHECK (source IN ('app', 'agent')),
    agent_id TEXT,
    kind     TEXT NOT NULL,
    message  TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS aliases (
    base_domain   TEXT PRIMARY KEY,
    variants_json TEXT NOT NULL
);
"#;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Agent {
    pub id: String,
    pub display_name: Option<String>,
    pub hostname: String,
    pub os: String,
    pub arch: String,
    pub last_ip: Option<String>,
    pub cert_serial: Option<String>,
    pub revoked: bool,
    pub last_seen_at: Option<String>,
    pub agent_version: Option<String>,
    pub policy_version: u64,
    pub events_cursor: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Group {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct List {
    pub id: String,
    pub name: String,
    pub domains: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetKind {
    Agent,
    Group,
}

impl TargetKind {
    fn as_str(self) -> &'static str {
        match self {
            TargetKind::Agent => "agent",
            TargetKind::Group => "group",
        }
    }

    fn parse(s: &str) -> Self {
        match s {
            "group" => TargetKind::Group,
            _ => TargetKind::Agent,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Assignment {
    pub id: String,
    pub list_id: String,
    pub target_kind: TargetKind,
    pub target_id: String,
    /// `None` means "always blocked"; `Some` is the list of
    /// [`homeguard_common::policy::TimeWindow`], serialized.
    pub schedule: Option<Vec<homeguard_common::policy::TimeWindow>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Exception {
    pub id: String,
    pub target_kind: TargetKind,
    pub target_id: String,
    pub domains: Vec<String>,
    pub until: jiff::Timestamp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub id: String,
    pub at: String,
    pub source: String,
    pub agent_id: Option<String>,
    pub kind: String,
    pub message: String,
}

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Adds columns to existing tables that `SCHEMA`'s `CREATE TABLE IF NOT
/// EXISTS` can't retrofit on its own. Each check is cheap and safe to run
/// on every `open`: a fresh database already has every column (just
/// created from `SCHEMA` above), so this is a no-op there.
fn migrate_add_missing_columns(conn: &Connection) -> Result<()> {
    let columns: &[(&str, &str, &str)] = &[
        ("agents", "events_cursor", "INTEGER NOT NULL DEFAULT 1"),
        ("agents", "arch", "TEXT NOT NULL DEFAULT 'unknown'"),
    ];
    for (table, column, ddl) in columns {
        let exists: i64 = conn.query_row(
            &format!("SELECT COUNT(*) FROM pragma_table_info('{table}') WHERE name = '{column}'"),
            [],
            |row| row.get(0),
        )?;
        if exists == 0 {
            conn.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN {column} {ddl};"))?;
        }
    }
    Ok(())
}

/// First-run only: loads the built-in catalog into the `aliases` table so
/// it's editable from the UI from day one — once seeded, the compiled
/// `default_catalog()` is never consulted again, the table is the single
/// source of truth. A non-empty table (every later launch) is left alone.
fn seed_default_aliases(conn: &Connection) -> Result<()> {
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM aliases", [], |row| row.get(0))?;
    if count > 0 {
        return Ok(());
    }
    for entry in homeguard_common::default_catalog() {
        let variants_json = serde_json::to_string(&entry.variants)?;
        conn.execute(
            "INSERT INTO aliases (base_domain, variants_json) VALUES (?1, ?2)",
            params![entry.base_domain, variants_json],
        )?;
    }
    Ok(())
}

impl Db {
    pub fn open(path: &std::path::Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        // The schema declares ON DELETE CASCADE (e.g. removing a list
        // should remove its assignments); SQLite only honors that when
        // foreign key enforcement is explicitly turned on per-connection.
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        conn.execute_batch(SCHEMA)?;
        // `CREATE TABLE IF NOT EXISTS` doesn't touch a table that already
        // exists on disk from an older version of the app — columns added
        // to `SCHEMA` since then (like `events_cursor`) need to be patched
        // in explicitly, or every query that mentions them fails with
        // "Invalid column name" against a pre-existing database file.
        migrate_add_missing_columns(&conn)?;
        seed_default_aliases(&conn)?;
        Ok(Db { conn: Mutex::new(conn) })
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        conn.execute_batch(SCHEMA)?;
        seed_default_aliases(&conn)?;
        Ok(Db { conn: Mutex::new(conn) })
    }

    // --- agents ---------------------------------------------------------

    pub fn upsert_agent(
        &self,
        id: &str,
        hostname: &str,
        os: &str,
        arch: &str,
        last_ip: Option<&str>,
        agent_version: Option<&str>,
    ) -> Result<()> {
        let now = jiff::Timestamp::now().to_string();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO agents (id, hostname, os, arch, last_ip, last_seen_at, agent_version)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(id) DO UPDATE SET
                hostname = excluded.hostname,
                os = excluded.os,
                arch = excluded.arch,
                last_ip = excluded.last_ip,
                last_seen_at = excluded.last_seen_at,
                agent_version = excluded.agent_version",
            params![id, hostname, os, arch, last_ip, now, agent_version],
        )?;
        Ok(())
    }

    pub fn set_agent_display_name(&self, id: &str, display_name: &str) -> Result<()> {
        self.conn
            .lock()
            .unwrap()
            .execute("UPDATE agents SET display_name = ?1 WHERE id = ?2", params![display_name, id])?;
        Ok(())
    }

    /// Bumps `last_seen_at` to now — called after any successful live
    /// contact with this agent (a status probe, a sync), independent of
    /// `upsert_agent` which also touches `hostname`/`os`/`arch`/`last_ip`
    /// and is only appropriate when those were actually just reported
    /// (mDNS discovery, enrollment).
    pub fn touch_agent_last_seen(&self, id: &str) -> Result<()> {
        let now = jiff::Timestamp::now().to_string();
        self.conn
            .lock()
            .unwrap()
            .execute("UPDATE agents SET last_seen_at = ?1 WHERE id = ?2", params![now, id])?;
        Ok(())
    }

    /// Records the serial of the mTLS certificate just issued to this
    /// agent, so it can later be identified in a revocation check.
    pub fn set_agent_cert_serial(&self, id: &str, cert_serial: &str) -> Result<()> {
        self.conn
            .lock()
            .unwrap()
            .execute("UPDATE agents SET cert_serial = ?1 WHERE id = ?2", params![cert_serial, id])?;
        Ok(())
    }

    /// Advances this agent's `/v1/events` cursor after a page of events has
    /// been pulled and recorded in `history`.
    pub fn set_agent_events_cursor(&self, id: &str, cursor: u64) -> Result<()> {
        self.conn.lock().unwrap().execute(
            "UPDATE agents SET events_cursor = ?1 WHERE id = ?2",
            params![cursor as i64, id],
        )?;
        Ok(())
    }

    /// Marks an agent's certificate as revoked — e.g. when the poste is
    /// removed, or its identity is suspected compromised. This alone
    /// doesn't invalidate the certificate cryptographically (there is no
    /// CRL/OCSP here); callers that connect to agents must check
    /// [`Db::is_agent_revoked`] themselves before trusting one — see
    /// [`crate::client::connect_to_agent`].
    pub fn revoke_agent(&self, id: &str) -> Result<()> {
        self.conn
            .lock()
            .unwrap()
            .execute("UPDATE agents SET revoked = 1 WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn is_agent_revoked(&self, id: &str) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let revoked: Option<i64> = conn
            .query_row("SELECT revoked FROM agents WHERE id = ?1", params![id], |row| row.get(0))
            .optional()?;
        Ok(revoked.unwrap_or(0) != 0)
    }

    pub fn bump_policy_version(&self, agent_id: &str) -> Result<u64> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE agents SET policy_version = policy_version + 1 WHERE id = ?1",
            params![agent_id],
        )?;
        let version: i64 = conn.query_row(
            "SELECT policy_version FROM agents WHERE id = ?1",
            params![agent_id],
            |row| row.get(0),
        )?;
        Ok(version as u64)
    }

    pub fn get_agent(&self, id: &str) -> Result<Option<Agent>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row("SELECT * FROM agents WHERE id = ?1", params![id], row_to_agent)
            .optional()
            .map_err(Into::into)
    }

    pub fn list_agents(&self) -> Result<Vec<Agent>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT * FROM agents ORDER BY hostname")?;
        let rows = stmt.query_map([], row_to_agent)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
    }

    pub fn agent_group_ids(&self, agent_id: &str) -> Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT group_id FROM group_members WHERE agent_id = ?1")?;
        let rows = stmt.query_map(params![agent_id], |row| row.get::<_, String>(0))?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
    }

    // --- groups ----------------------------------------------------------

    pub fn create_group(&self, name: &str) -> Result<String> {
        let id = new_id();
        self.conn
            .lock()
            .unwrap()
            .execute("INSERT INTO groups (id, name) VALUES (?1, ?2)", params![id, name])?;
        Ok(id)
    }

    /// Like [`Db::create_group`], but with an explicit id instead of
    /// generating one — used by config import, where ids must be
    /// preserved so `group_members`/`assignments` referencing them stay
    /// valid. Safe to call twice with the same id (upsert).
    pub fn upsert_group(&self, id: &str, name: &str) -> Result<()> {
        self.conn.lock().unwrap().execute(
            "INSERT INTO groups (id, name) VALUES (?1, ?2)
             ON CONFLICT(id) DO UPDATE SET name = excluded.name",
            params![id, name],
        )?;
        Ok(())
    }

    /// Every (group_id, agent_id) membership pair in the fleet.
    pub fn list_group_members(&self) -> Result<Vec<(String, String)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT group_id, agent_id FROM group_members")?;
        let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
    }

    pub fn list_groups(&self) -> Result<Vec<Group>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id, name FROM groups ORDER BY name")?;
        let rows = stmt.query_map([], |row| Ok(Group { id: row.get(0)?, name: row.get(1)? }))?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
    }

    /// Also removes this group's memberships (real foreign key cascade —
    /// see the `PRAGMA foreign_keys = ON` set at connection open) and any
    /// assignments/exceptions that targeted it (a polymorphic reference by
    /// id, which SQLite can't cascade on its own).
    pub fn delete_group(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM assignments WHERE target_kind = 'group' AND target_id = ?1",
            params![id],
        )?;
        conn.execute(
            "DELETE FROM exceptions WHERE target_kind = 'group' AND target_id = ?1",
            params![id],
        )?;
        conn.execute("DELETE FROM groups WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn add_group_member(&self, group_id: &str, agent_id: &str) -> Result<()> {
        self.conn.lock().unwrap().execute(
            "INSERT OR IGNORE INTO group_members (group_id, agent_id) VALUES (?1, ?2)",
            params![group_id, agent_id],
        )?;
        Ok(())
    }

    pub fn remove_group_member(&self, group_id: &str, agent_id: &str) -> Result<()> {
        self.conn.lock().unwrap().execute(
            "DELETE FROM group_members WHERE group_id = ?1 AND agent_id = ?2",
            params![group_id, agent_id],
        )?;
        Ok(())
    }

    // --- lists -------------------------------------------------------------

    pub fn create_list(&self, name: &str, domains: &[String]) -> Result<String> {
        let id = new_id();
        let json = serde_json::to_string(domains)?;
        self.conn
            .lock()
            .unwrap()
            .execute("INSERT INTO lists (id, name, domains_json) VALUES (?1, ?2, ?3)", params![id, name, json])?;
        Ok(id)
    }

    /// Like [`Db::create_list`], but with an explicit id — used by config
    /// import to preserve ids that `assignments` reference. Upsert.
    pub fn upsert_list(&self, id: &str, name: &str, domains: &[String]) -> Result<()> {
        let json = serde_json::to_string(domains)?;
        self.conn.lock().unwrap().execute(
            "INSERT INTO lists (id, name, domains_json) VALUES (?1, ?2, ?3)
             ON CONFLICT(id) DO UPDATE SET name = excluded.name, domains_json = excluded.domains_json",
            params![id, name, json],
        )?;
        Ok(())
    }

    pub fn update_list_domains(&self, list_id: &str, domains: &[String]) -> Result<()> {
        let json = serde_json::to_string(domains)?;
        self.conn
            .lock()
            .unwrap()
            .execute("UPDATE lists SET domains_json = ?1 WHERE id = ?2", params![json, list_id])?;
        Ok(())
    }

    pub fn get_list(&self, id: &str) -> Result<Option<List>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row("SELECT id, name, domains_json FROM lists WHERE id = ?1", params![id], row_to_list)
            .optional()
            .map_err(Into::into)
    }

    pub fn list_lists(&self) -> Result<Vec<List>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id, name, domains_json FROM lists ORDER BY name")?;
        let rows = stmt.query_map([], row_to_list)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
    }

    /// Also removes any assignment that used this list (real foreign key
    /// cascade this time — `assignments.list_id` is a proper FK).
    pub fn delete_list(&self, id: &str) -> Result<()> {
        self.conn.lock().unwrap().execute("DELETE FROM lists WHERE id = ?1", params![id])?;
        Ok(())
    }

    // --- assignments ---------------------------------------------------

    pub fn delete_assignment(&self, id: &str) -> Result<()> {
        self.conn.lock().unwrap().execute("DELETE FROM assignments WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// Replaces just an existing assignment's schedule — what the "Horaire…"
    /// editor saves, as opposed to [`Db::create_assignment`] (a new
    /// assignment entirely) or [`Db::upsert_assignment`] (every field, for
    /// import).
    pub fn update_assignment_schedule(
        &self,
        id: &str,
        schedule: Option<&[homeguard_common::policy::TimeWindow]>,
    ) -> Result<()> {
        let schedule_json = schedule.map(serde_json::to_string).transpose()?;
        self.conn
            .lock()
            .unwrap()
            .execute("UPDATE assignments SET schedule_json = ?1 WHERE id = ?2", params![schedule_json, id])?;
        Ok(())
    }

    pub fn create_assignment(
        &self,
        list_id: &str,
        target_kind: TargetKind,
        target_id: &str,
        schedule: Option<&[homeguard_common::policy::TimeWindow]>,
    ) -> Result<String> {
        let id = new_id();
        let schedule_json = schedule.map(serde_json::to_string).transpose()?;
        self.conn.lock().unwrap().execute(
            "INSERT INTO assignments (id, list_id, target_kind, target_id, schedule_json) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, list_id, target_kind.as_str(), target_id, schedule_json],
        )?;
        Ok(id)
    }

    /// Like [`Db::create_assignment`], but with an explicit id — used by
    /// config import. Upsert.
    pub fn upsert_assignment(
        &self,
        id: &str,
        list_id: &str,
        target_kind: TargetKind,
        target_id: &str,
        schedule: Option<&[homeguard_common::policy::TimeWindow]>,
    ) -> Result<()> {
        let schedule_json = schedule.map(serde_json::to_string).transpose()?;
        self.conn.lock().unwrap().execute(
            "INSERT INTO assignments (id, list_id, target_kind, target_id, schedule_json) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(id) DO UPDATE SET
                list_id = excluded.list_id,
                target_kind = excluded.target_kind,
                target_id = excluded.target_id,
                schedule_json = excluded.schedule_json",
            params![id, list_id, target_kind.as_str(), target_id, schedule_json],
        )?;
        Ok(())
    }

    /// Every assignment that applies to `agent_id`, either directly or
    /// through one of `group_ids`.
    pub fn assignments_for(&self, agent_id: &str, group_ids: &[String]) -> Result<Vec<Assignment>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, list_id, target_kind, target_id, schedule_json FROM assignments
             WHERE (target_kind = 'agent' AND target_id = ?1)
                OR (target_kind = 'group' AND target_id IN (SELECT value FROM json_each(?2)))",
        )?;
        let group_ids_json = serde_json::to_string(group_ids)?;
        let rows = stmt.query_map(params![agent_id, group_ids_json], row_to_assignment)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
            .into_iter()
            .map(|r| r.map_err(DbError::Json))
            .collect()
    }

    /// Every assignment in the fleet, regardless of target — used to build
    /// a global view (e.g. the dashboard's "prochaines transitions"),
    /// where per-agent filtering doesn't apply.
    pub fn list_assignments(&self) -> Result<Vec<Assignment>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id, list_id, target_kind, target_id, schedule_json FROM assignments")?;
        let rows = stmt.query_map([], row_to_assignment)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
            .into_iter()
            .map(|r| r.map_err(DbError::Json))
            .collect()
    }

    // --- exceptions ------------------------------------------------------

    pub fn create_exception(&self, target_kind: TargetKind, target_id: &str, domains: &[String], until: jiff::Timestamp) -> Result<String> {
        let id = new_id();
        let domains_json = serde_json::to_string(domains)?;
        self.conn.lock().unwrap().execute(
            "INSERT INTO exceptions (id, target_kind, target_id, domains_json, until) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, target_kind.as_str(), target_id, domains_json, until.to_string()],
        )?;
        Ok(id)
    }

    /// Like [`Db::create_exception`], but with an explicit id — used by
    /// config import. Upsert.
    pub fn upsert_exception(
        &self,
        id: &str,
        target_kind: TargetKind,
        target_id: &str,
        domains: &[String],
        until: jiff::Timestamp,
    ) -> Result<()> {
        let domains_json = serde_json::to_string(domains)?;
        self.conn.lock().unwrap().execute(
            "INSERT INTO exceptions (id, target_kind, target_id, domains_json, until) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(id) DO UPDATE SET
                target_kind = excluded.target_kind,
                target_id = excluded.target_id,
                domains_json = excluded.domains_json,
                until = excluded.until",
            params![id, target_kind.as_str(), target_id, domains_json, until.to_string()],
        )?;
        Ok(())
    }

    /// Every exception in the fleet, regardless of target — used for a
    /// full config export.
    pub fn list_exceptions(&self) -> Result<Vec<Exception>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id, target_kind, target_id, domains_json, until FROM exceptions")?;
        let rows = stmt.query_map([], row_to_exception)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
            .into_iter()
            .collect::<std::result::Result<Vec<_>, DbError>>()
    }

    pub fn delete_exception(&self, id: &str) -> Result<()> {
        self.conn.lock().unwrap().execute("DELETE FROM exceptions WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn exceptions_for(&self, agent_id: &str, group_ids: &[String]) -> Result<Vec<Exception>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, target_kind, target_id, domains_json, until FROM exceptions
             WHERE (target_kind = 'agent' AND target_id = ?1)
                OR (target_kind = 'group' AND target_id IN (SELECT value FROM json_each(?2)))",
        )?;
        let group_ids_json = serde_json::to_string(group_ids)?;
        let rows = stmt.query_map(params![agent_id, group_ids_json], row_to_exception)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
            .into_iter()
            .collect::<std::result::Result<Vec<_>, DbError>>()
    }

    // --- history -----------------------------------------------------------

    pub fn append_history(&self, source: &str, agent_id: Option<&str>, kind: &str, message: &str) -> Result<()> {
        let id = new_id();
        let at = jiff::Timestamp::now().to_string();
        self.conn.lock().unwrap().execute(
            "INSERT INTO history (id, at, source, agent_id, kind, message) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, at, source, agent_id, kind, message],
        )?;
        Ok(())
    }

    /// Records one event pulled from an agent's own journal (`/v1/events`)
    /// into the unified history, using the event's own timestamp rather
    /// than "now" — and a deterministic id (`agent:<agent_id>:<seq>`) so
    /// re-pulling the same event after a partial failure (cursor not
    /// advanced yet) is a safe no-op instead of a duplicate row.
    pub fn record_agent_event(&self, agent_id: &str, event: &homeguard_common::wire::EventEntry) -> Result<()> {
        let id = format!("agent:{agent_id}:{}", event.seq);
        let kind = serde_json::to_value(event.kind)?
            .as_str()
            .unwrap_or("unknown")
            .to_string();
        self.conn.lock().unwrap().execute(
            "INSERT OR IGNORE INTO history (id, at, source, agent_id, kind, message) VALUES (?1, ?2, 'agent', ?3, ?4, ?5)",
            params![id, event.at, agent_id, kind, event.message],
        )?;
        Ok(())
    }

    pub fn recent_history(&self, limit: u32) -> Result<Vec<HistoryEntry>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id, at, source, agent_id, kind, message FROM history ORDER BY at DESC LIMIT ?1")?;
        let rows = stmt.query_map(params![limit], |row| {
            Ok(HistoryEntry {
                id: row.get(0)?,
                at: row.get(1)?,
                source: row.get(2)?,
                agent_id: row.get(3)?,
                kind: row.get(4)?,
                message: row.get(5)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
    }

    // --- settings ----------------------------------------------------------

    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row("SELECT value FROM settings WHERE key = ?1", params![key], |row| row.get(0))
            .optional()
            .map_err(Into::into)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.conn.lock().unwrap().execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    /// Every setting, for a full config export.
    pub fn list_settings(&self) -> Result<Vec<(String, String)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT key, value FROM settings")?;
        let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
    }

    // --- domain alias catalog --------------------------------------------

    pub fn list_aliases(&self) -> Result<Vec<AliasEntry>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT base_domain, variants_json FROM aliases ORDER BY base_domain")?;
        let rows = stmt.query_map([], row_to_alias)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
    }

    /// Insert or replace one base domain's variant list — the add form and
    /// an edit both just call this with the full desired variant set.
    pub fn upsert_alias(&self, base_domain: &str, variants: &[String]) -> Result<()> {
        let variants_json = serde_json::to_string(variants)?;
        self.conn.lock().unwrap().execute(
            "INSERT INTO aliases (base_domain, variants_json) VALUES (?1, ?2)
             ON CONFLICT(base_domain) DO UPDATE SET variants_json = excluded.variants_json",
            params![base_domain, variants_json],
        )?;
        Ok(())
    }

    pub fn delete_alias(&self, base_domain: &str) -> Result<()> {
        self.conn
            .lock()
            .unwrap()
            .execute("DELETE FROM aliases WHERE base_domain = ?1", params![base_domain])?;
        Ok(())
    }
}

fn row_to_agent(row: &rusqlite::Row) -> rusqlite::Result<Agent> {
    Ok(Agent {
        id: row.get("id")?,
        display_name: row.get("display_name")?,
        hostname: row.get("hostname")?,
        os: row.get("os")?,
        arch: row.get("arch")?,
        last_ip: row.get("last_ip")?,
        cert_serial: row.get("cert_serial")?,
        revoked: row.get::<_, i64>("revoked")? != 0,
        last_seen_at: row.get("last_seen_at")?,
        agent_version: row.get("agent_version")?,
        policy_version: row.get::<_, i64>("policy_version")? as u64,
        events_cursor: row.get::<_, i64>("events_cursor")? as u64,
    })
}

fn row_to_list(row: &rusqlite::Row) -> rusqlite::Result<List> {
    let domains_json: String = row.get(2)?;
    let domains: Vec<String> = serde_json::from_str(&domains_json).unwrap_or_default();
    Ok(List {
        id: row.get(0)?,
        name: row.get(1)?,
        domains,
    })
}

fn row_to_alias(row: &rusqlite::Row) -> rusqlite::Result<AliasEntry> {
    let variants_json: String = row.get(1)?;
    let variants: Vec<String> = serde_json::from_str(&variants_json).unwrap_or_default();
    Ok(AliasEntry {
        base_domain: row.get(0)?,
        variants,
    })
}

fn row_to_assignment(row: &rusqlite::Row) -> rusqlite::Result<std::result::Result<Assignment, serde_json::Error>> {
    let id: String = row.get(0)?;
    let list_id: String = row.get(1)?;
    let target_kind: String = row.get(2)?;
    let target_id: String = row.get(3)?;
    let schedule_json: Option<String> = row.get(4)?;
    Ok((|| {
        let schedule = schedule_json.map(|s| serde_json::from_str(&s)).transpose()?;
        Ok(Assignment {
            id,
            list_id,
            target_kind: TargetKind::parse(&target_kind),
            target_id,
            schedule,
        })
    })())
}

fn row_to_exception(row: &rusqlite::Row) -> rusqlite::Result<std::result::Result<Exception, DbError>> {
    let id: String = row.get(0)?;
    let target_kind: String = row.get(1)?;
    let target_id: String = row.get(2)?;
    let domains_json: String = row.get(3)?;
    let until_str: String = row.get(4)?;
    Ok((|| {
        let domains: Vec<String> = serde_json::from_str(&domains_json)?;
        let until: jiff::Timestamp = until_str
            .parse()
            .map_err(|e: jiff::Error| DbError::Sqlite(rusqlite::Error::ToSqlConversionFailure(Box::new(e))))?;
        Ok(Exception {
            id,
            target_kind: TargetKind::parse(&target_kind),
            target_id,
            domains,
            until,
        })
    })())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upserts_and_lists_agents() {
        let db = Db::open_in_memory().unwrap();
        db.upsert_agent("a1", "PC-SALON", "windows", "x86_64", Some("192.168.1.34"), Some("0.1.0")).unwrap();
        db.upsert_agent("a1", "PC-SALON", "windows", "x86_64", Some("192.168.1.35"), Some("0.1.1")).unwrap();

        let agents = db.list_agents().unwrap();
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].last_ip.as_deref(), Some("192.168.1.35"));
        assert_eq!(agents[0].agent_version.as_deref(), Some("0.1.1"));
    }

    #[test]
    fn touch_agent_last_seen_updates_only_that_field() {
        let db = Db::open_in_memory().unwrap();
        db.upsert_agent("a1", "PC-SALON", "windows", "x86_64", Some("192.168.1.34"), Some("0.1.0")).unwrap();
        let before = db.get_agent("a1").unwrap().unwrap().last_seen_at;

        std::thread::sleep(std::time::Duration::from_millis(5));
        db.touch_agent_last_seen("a1").unwrap();

        let after = db.get_agent("a1").unwrap().unwrap();
        assert_ne!(after.last_seen_at, before);
        // Nothing else should move.
        assert_eq!(after.hostname, "PC-SALON");
        assert_eq!(after.last_ip.as_deref(), Some("192.168.1.34"));
    }

    #[test]
    fn events_cursor_defaults_to_one_and_can_be_advanced() {
        let db = Db::open_in_memory().unwrap();
        db.upsert_agent("a1", "PC-SALON", "windows", "x86_64", None, None).unwrap();
        assert_eq!(db.get_agent("a1").unwrap().unwrap().events_cursor, 1);

        db.set_agent_events_cursor("a1", 42).unwrap();
        assert_eq!(db.get_agent("a1").unwrap().unwrap().events_cursor, 42);

        // Re-upserting (e.g. rediscovered via mDNS) must not reset it.
        db.upsert_agent("a1", "PC-SALON", "windows", "x86_64", Some("192.168.1.9"), None).unwrap();
        assert_eq!(db.get_agent("a1").unwrap().unwrap().events_cursor, 42);
    }

    #[test]
    fn migrate_add_missing_columns_adds_events_cursor_to_a_pre_existing_agents_table() {
        // Simulates a database file created by an older version of the app,
        // from before `events_cursor` existed in `SCHEMA` — `CREATE TABLE IF
        // NOT EXISTS` wouldn't touch such a table, so opening it without
        // this migration reproduces the "Invalid column name: events_cursor"
        // error seen against a real, previously-enrolled app data directory.
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE agents (
                id TEXT PRIMARY KEY,
                display_name TEXT,
                hostname TEXT NOT NULL,
                os TEXT NOT NULL,
                last_ip TEXT,
                cert_serial TEXT,
                revoked INTEGER NOT NULL DEFAULT 0,
                last_seen_at TEXT,
                agent_version TEXT,
                policy_version INTEGER NOT NULL DEFAULT 0
            );
            INSERT INTO agents (id, hostname, os) VALUES ('a1', 'PC-SALON', 'linux');",
        )
        .unwrap();

        migrate_add_missing_columns(&conn).unwrap();

        let cursor: i64 = conn
            .query_row("SELECT events_cursor FROM agents WHERE id = 'a1'", [], |row| row.get(0))
            .unwrap();
        assert_eq!(cursor, 1);
    }

    #[test]
    fn migrate_add_missing_columns_adds_arch_to_a_pre_existing_agents_table() {
        // Same scenario as above, for the `arch` column added alongside
        // point-1 of the remote-update feature (distinguishing the two
        // Linux agent binaries needs more than `os` alone).
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE agents (
                id TEXT PRIMARY KEY,
                display_name TEXT,
                hostname TEXT NOT NULL,
                os TEXT NOT NULL,
                last_ip TEXT,
                cert_serial TEXT,
                revoked INTEGER NOT NULL DEFAULT 0,
                last_seen_at TEXT,
                agent_version TEXT,
                policy_version INTEGER NOT NULL DEFAULT 0,
                events_cursor INTEGER NOT NULL DEFAULT 1
            );
            INSERT INTO agents (id, hostname, os) VALUES ('a1', 'PC-SALON', 'linux');",
        )
        .unwrap();

        migrate_add_missing_columns(&conn).unwrap();

        let arch: String = conn
            .query_row("SELECT arch FROM agents WHERE id = 'a1'", [], |row| row.get(0))
            .unwrap();
        assert_eq!(arch, "unknown");
    }

    #[test]
    fn records_cert_serial_and_revokes_an_agent() {
        let db = Db::open_in_memory().unwrap();
        db.upsert_agent("a1", "PC-SALON", "windows", "x86_64", None, None).unwrap();
        assert!(!db.is_agent_revoked("a1").unwrap());

        db.set_agent_cert_serial("a1", "aa11bb22").unwrap();
        assert_eq!(db.get_agent("a1").unwrap().unwrap().cert_serial.as_deref(), Some("aa11bb22"));

        db.revoke_agent("a1").unwrap();
        assert!(db.is_agent_revoked("a1").unwrap());
        assert!(db.get_agent("a1").unwrap().unwrap().revoked);
    }

    #[test]
    fn is_agent_revoked_is_false_for_an_unknown_agent() {
        let db = Db::open_in_memory().unwrap();
        assert!(!db.is_agent_revoked("does-not-exist").unwrap());
    }

    #[test]
    fn bumps_policy_version_monotonically() {
        let db = Db::open_in_memory().unwrap();
        db.upsert_agent("a1", "PC-SALON", "windows", "x86_64", None, None).unwrap();
        assert_eq!(db.bump_policy_version("a1").unwrap(), 1);
        assert_eq!(db.bump_policy_version("a1").unwrap(), 2);
    }

    #[test]
    fn resolves_assignments_via_direct_and_group_targets() {
        let db = Db::open_in_memory().unwrap();
        db.upsert_agent("a1", "PORTABLE-LEO", "windows", "x86_64", None, None).unwrap();
        let group_id = db.create_group("Enfants").unwrap();
        db.add_group_member(&group_id, "a1").unwrap();

        let games_list = db.create_list("Jeux", &["roblox.com".into()]).unwrap();
        let streaming_list = db.create_list("Streaming", &["twitch.tv".into()]).unwrap();

        db.create_assignment(&games_list, TargetKind::Group, &group_id, None).unwrap();
        db.create_assignment(&streaming_list, TargetKind::Agent, "a1", None).unwrap();

        let group_ids = db.agent_group_ids("a1").unwrap();
        assert_eq!(group_ids, vec![group_id.clone()]);

        let assignments = db.assignments_for("a1", &group_ids).unwrap();
        assert_eq!(assignments.len(), 2);
    }

    #[test]
    fn update_assignment_schedule_replaces_only_the_schedule() {
        let db = Db::open_in_memory().unwrap();
        db.upsert_agent("a1", "PORTABLE-LEO", "windows", "x86_64", None, None).unwrap();
        let list_id = db.create_list("Jeux", &["roblox.com".into()]).unwrap();
        let assignment_id = db.create_assignment(&list_id, TargetKind::Agent, "a1", None).unwrap();

        let window = homeguard_common::policy::TimeWindow {
            days: vec![homeguard_common::Day::Mon, homeguard_common::Day::Tue],
            from: jiff::civil::time(8, 0, 0, 0),
            to: jiff::civil::time(18, 0, 0, 0),
        };
        db.update_assignment_schedule(&assignment_id, Some(&[window])).unwrap();

        let assignments = db.list_assignments().unwrap();
        assert_eq!(assignments.len(), 1);
        assert_eq!(assignments[0].list_id, list_id);
        assert!(assignments[0].schedule.is_some());

        db.update_assignment_schedule(&assignment_id, None).unwrap();
        assert!(db.list_assignments().unwrap()[0].schedule.is_none());
    }

    #[test]
    fn list_assignments_returns_every_assignment_regardless_of_target() {
        let db = Db::open_in_memory().unwrap();
        db.upsert_agent("a1", "PORTABLE-LEO", "windows", "x86_64", None, None).unwrap();
        let group_id = db.create_group("Enfants").unwrap();
        let list1 = db.create_list("Jeux", &["roblox.com".into()]).unwrap();
        let list2 = db.create_list("Streaming", &["twitch.tv".into()]).unwrap();
        db.create_assignment(&list1, TargetKind::Agent, "a1", None).unwrap();
        db.create_assignment(&list2, TargetKind::Group, &group_id, None).unwrap();

        assert_eq!(db.list_assignments().unwrap().len(), 2);
    }

    #[test]
    fn deleting_a_list_cascades_its_assignments() {
        let db = Db::open_in_memory().unwrap();
        db.upsert_agent("a1", "PORTABLE-LEO", "windows", "x86_64", None, None).unwrap();
        let list_id = db.create_list("Jeux", &["roblox.com".into()]).unwrap();
        db.create_assignment(&list_id, TargetKind::Agent, "a1", None).unwrap();
        assert_eq!(db.assignments_for("a1", &[]).unwrap().len(), 1);

        db.delete_list(&list_id).unwrap();
        assert_eq!(db.assignments_for("a1", &[]).unwrap().len(), 0);
        assert!(db.get_list(&list_id).unwrap().is_none());
    }

    #[test]
    fn deleting_a_group_removes_its_memberships_and_assignments() {
        let db = Db::open_in_memory().unwrap();
        db.upsert_agent("a1", "PORTABLE-LEO", "windows", "x86_64", None, None).unwrap();
        let group_id = db.create_group("Enfants").unwrap();
        db.add_group_member(&group_id, "a1").unwrap();
        let list_id = db.create_list("Jeux", &["roblox.com".into()]).unwrap();
        db.create_assignment(&list_id, TargetKind::Group, &group_id, None).unwrap();

        db.delete_group(&group_id).unwrap();

        assert!(db.list_groups().unwrap().is_empty());
        assert!(db.agent_group_ids("a1").unwrap().is_empty());
        // The list itself survives; only the assignment referencing the
        // now-gone group does not.
        assert!(db.get_list(&list_id).unwrap().is_some());
        assert_eq!(db.assignments_for("a1", &[group_id]).unwrap().len(), 0);
    }

    #[test]
    fn settings_roundtrip_and_default_to_none() {
        let db = Db::open_in_memory().unwrap();
        assert_eq!(db.get_setting("timezone").unwrap(), None);
        db.set_setting("timezone", "Europe/Paris").unwrap();
        assert_eq!(db.get_setting("timezone").unwrap(), Some("Europe/Paris".to_string()));
        db.set_setting("timezone", "UTC").unwrap();
        assert_eq!(db.get_setting("timezone").unwrap(), Some("UTC".to_string()));
    }

    #[test]
    fn stores_and_reads_back_exceptions() {
        let db = Db::open_in_memory().unwrap();
        db.upsert_agent("a1", "PORTABLE-LEO", "windows", "x86_64", None, None).unwrap();
        let until = jiff::Timestamp::now() + jiff::SignedDuration::from_secs(1800);
        db.create_exception(TargetKind::Agent, "a1", &["twitch.tv".into()], until).unwrap();

        let exceptions = db.exceptions_for("a1", &[]).unwrap();
        assert_eq!(exceptions.len(), 1);
        assert_eq!(exceptions[0].domains, vec!["twitch.tv".to_string()]);
    }

    #[test]
    fn records_and_reads_history() {
        let db = Db::open_in_memory().unwrap();
        db.append_history("app", Some("a1"), "policy_applied", "politique v1 appliquée").unwrap();
        let history = db.recent_history(10).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].message, "politique v1 appliquée");
    }

    #[test]
    fn record_agent_event_uses_the_event_timestamp_and_is_idempotent() {
        use homeguard_common::wire::{EventEntry, EventKind};

        let db = Db::open_in_memory().unwrap();
        let event = EventEntry {
            seq: 7,
            at: "2026-01-01T00:00:00Z".to_string(),
            kind: EventKind::IntegrityRestored,
            message: "hosts restauré automatiquement".to_string(),
        };
        db.record_agent_event("a1", &event).unwrap();
        // Re-pulling the same seq (e.g. cursor update failed after insert)
        // must not create a duplicate row.
        db.record_agent_event("a1", &event).unwrap();

        let history = db.recent_history(10).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].at, "2026-01-01T00:00:00Z");
        assert_eq!(history[0].source, "agent");
        assert_eq!(history[0].kind, "integrity_restored");
        assert_eq!(history[0].agent_id.as_deref(), Some("a1"));
    }

    #[test]
    fn fresh_db_is_seeded_with_the_builtin_alias_catalog() {
        let db = Db::open_in_memory().unwrap();
        let aliases = db.list_aliases().unwrap();
        assert_eq!(aliases.len(), homeguard_common::default_catalog().len());
        assert!(aliases.iter().any(|a| a.base_domain == "youtube.com"));
    }

    #[test]
    fn alias_catalog_is_editable() {
        let db = Db::open_in_memory().unwrap();

        db.upsert_alias("example.org", &["www.example.org".to_string()]).unwrap();
        let aliases = db.list_aliases().unwrap();
        assert!(aliases.iter().any(|a| a.base_domain == "example.org" && a.variants == vec!["www.example.org"]));

        // Editing a built-in entry replaces its variants rather than adding
        // a second row for the same base domain.
        db.upsert_alias("youtube.com", &["kids.youtube.com".to_string()]).unwrap();
        let aliases = db.list_aliases().unwrap();
        let youtube = aliases.iter().filter(|a| a.base_domain == "youtube.com").count();
        assert_eq!(youtube, 1);
        assert_eq!(
            aliases.iter().find(|a| a.base_domain == "youtube.com").unwrap().variants,
            vec!["kids.youtube.com".to_string()]
        );

        db.delete_alias("example.org").unwrap();
        assert!(!db.list_aliases().unwrap().iter().any(|a| a.base_domain == "example.org"));
    }
}
