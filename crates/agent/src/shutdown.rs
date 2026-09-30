//! Distinguishes the agent being stopped on purpose (an admin running
//! `systemctl stop`, `kill`, or Ctrl+C in the foreground) from the machine
//! itself shutting down or rebooting, and logs which one happened before
//! the process exits — so a poste going dark shows up in the history as
//! "someone stopped protection" rather than silently looking identical to
//! "the computer was turned off", which needs no attention at all.
//!
//! Windows gets this almost for free: the Service Control Manager already
//! delivers a *different* control code for an explicit stop
//! (`SERVICE_CONTROL_STOP`) versus the machine shutting down
//! (`SERVICE_CONTROL_SHUTDOWN`/`PRESHUTDOWN`) — see `service.rs`'s service
//! dispatcher, which forwards whichever one it gets through
//! `wait_for_external_signal` below instead of this module's Unix signal
//! heuristic. On Linux/Unix there's no such built-in signal, so
//! `wait_for_shutdown_signal` uses a heuristic instead (see
//! `is_system_shutting_down`).

use homeguard_common::wire::EventKind;

use crate::events::EventLog;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShutdownCause {
    /// Ctrl+C, `systemctl stop`, `kill <pid>` while the system itself
    /// stays up — someone (or something) deliberately stopped just this
    /// agent.
    Manual,
    /// The machine is powering off or rebooting; every process gets
    /// terminated together, this one included. Not worth an alert.
    SystemShutdown,
}

/// Waits for whichever termination signal arrives first, foreground
/// Ctrl+C included, and reports why.
pub async fn wait_for_shutdown_signal() -> ShutdownCause {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};

        // SIGINT (Ctrl+C in a foreground terminal) is unambiguous: a human
        // is right there doing it on purpose.
        let ctrl_c = async {
            let _ = tokio::signal::ctrl_c().await;
        };
        // SIGTERM is what `systemctl stop` *and* a system shutdown both
        // send — telling them apart needs the heuristic below.
        let sigterm = async {
            let Ok(mut term) = signal(SignalKind::terminate()) else {
                std::future::pending::<()>().await;
                unreachable!();
            };
            term.recv().await;
        };

        tokio::select! {
            _ = ctrl_c => ShutdownCause::Manual,
            _ = sigterm => {
                if is_system_shutting_down() {
                    ShutdownCause::SystemShutdown
                } else {
                    ShutdownCause::Manual
                }
            }
        }
    }

    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
        ShutdownCause::Manual
    }
}

/// Used only on the Windows-service path (see `service.rs`): the SCM's
/// control handler already tells a deliberate stop apart from a system
/// shutdown/preshutdown via distinct control codes, so this just forwards
/// whichever `ShutdownCause` it decided on — no heuristic needed. If the
/// sender is dropped without sending (the control handler thread panicked),
/// err toward `Manual` for the same reason `is_system_shutting_down` does:
/// missing a real shutdown just wastes a glance at the history, missing a
/// real manual stop does not.
pub async fn wait_for_external_signal(rx: tokio::sync::oneshot::Receiver<ShutdownCause>) -> ShutdownCause {
    rx.await.unwrap_or(ShutdownCause::Manual)
}

/// Best-effort: `systemctl is-system-running` prints `stopping` while the
/// system is shutting down or rebooting. Anything else — including the
/// command not existing at all (non-systemd Linux, or this binary humoring
/// a non-Linux Unix at compile time) — means we can't confirm a real
/// shutdown, so this errs toward reporting a manual stop instead: missing
/// a real system shutdown just wastes an admin's glance at the history,
/// but missing a real manual stop is the whole point of this module.
#[cfg(unix)]
fn is_system_shutting_down() -> bool {
    std::process::Command::new("systemctl")
        .arg("is-system-running")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "stopping")
        .unwrap_or(false)
}

/// Only a genuine manual stop is worth a history entry — a system
/// shutdown ends every process the same way and needs no attention.
pub fn log_shutdown(events: &EventLog, cause: ShutdownCause) {
    if cause == ShutdownCause::Manual {
        let _ = events.append(EventKind::ManualStopDetected, "agent arrêté manuellement");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Sends a *real* SIGTERM to this test process (safe: once
    // `wait_for_shutdown_signal` registers its listener, tokio owns
    // SIGTERM's disposition for the process, overriding the default
    // terminate action — the process won't die from this). This crate has
    // only one such test; a second one running concurrently would race
    // for the same process-wide signal delivery, so keep it that way
    // rather than copying this pattern elsewhere without also serializing
    // the tests.
    #[cfg(unix)]
    #[tokio::test]
    async fn sigterm_outside_a_system_shutdown_is_reported_as_manual() {
        let handle = tokio::spawn(wait_for_shutdown_signal());
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let pid = std::process::id().to_string();
        std::process::Command::new("kill").args(["-TERM", &pid]).status().unwrap();

        let cause = tokio::time::timeout(std::time::Duration::from_secs(3), handle)
            .await
            .expect("wait_for_shutdown_signal should return promptly after SIGTERM")
            .unwrap();

        // This sandbox has no systemd shutdown actually in progress, so
        // the heuristic correctly falls back to Manual — which also
        // happens to be the literal truth here (we just sent a bare
        // SIGTERM, not a shutdown).
        assert_eq!(cause, ShutdownCause::Manual);
    }

    #[test]
    fn manual_stop_is_logged() {
        let dir = tempfile::tempdir().unwrap();
        let events = EventLog::open(dir.path().join("events.jsonl")).unwrap();

        log_shutdown(&events, ShutdownCause::Manual);

        let (entries, _) = events.since(1).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].kind, EventKind::ManualStopDetected);
    }

    #[test]
    fn system_shutdown_is_not_logged() {
        let dir = tempfile::tempdir().unwrap();
        let events = EventLog::open(dir.path().join("events.jsonl")).unwrap();

        log_shutdown(&events, ShutdownCause::SystemShutdown);

        let (entries, _) = events.since(1).unwrap();
        assert!(entries.is_empty(), "a routine power-off shouldn't need an admin's attention");
    }
}
