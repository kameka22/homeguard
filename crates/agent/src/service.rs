//! OS service (de)registration. Real and testable on Linux (systemd unit
//! file rendering); the Windows Service Control Manager glue follows the
//! `windows-service` crate's documented pattern but cannot be compiled or
//! exercised from this Linux sandbox — it needs verification on a real
//! Windows machine.

use std::sync::Arc;

use crate::state::AppState;

pub const SERVICE_NAME: &str = "homeguard-agent";

#[derive(Debug, thiserror::Error)]
pub enum ServiceError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("command {0:?} exited with status {1}")]
    CommandFailed(String, std::process::ExitStatus),
}

/// Restore the hosts file to its pre-agent state (if we have a backup) and
/// lift the DNS-over-HTTPS-disabled browser policies, then remove the
/// service registration and this agent's own data directory. Called both
/// by `homeguard-agent uninstall` (run locally by an admin) and,
/// indirectly, by `POST /v1/uninstall` (triggered remotely by the app).
pub fn perform_uninstall(
    paths: &crate::config::AgentPaths,
    hosts_path: &std::path::Path,
    etc_root: &std::path::Path,
) -> Result<(), ServiceError> {
    restore_hosts_backup(hosts_path)?;
    if let Err(e) = crate::browser_policy::apply(etc_root, false) {
        // Best-effort: an agent being uninstalled shouldn't get stuck over
        // this, but it's worth knowing about.
        tracing::warn!("échec de la restauration des politiques navigateurs : {e}");
    }
    platform::uninstall_service()?;
    let _ = std::fs::remove_dir_all(&paths.data_dir);
    Ok(())
}

fn restore_hosts_backup(hosts_path: &std::path::Path) -> Result<(), ServiceError> {
    let mut backup_name = hosts_path
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    backup_name.push(".homeguard.bak");
    let backup_path = hosts_path.with_file_name(backup_name);

    if backup_path.exists() {
        std::fs::copy(&backup_path, hosts_path)?;
    }
    Ok(())
}

/// `POST /v1/uninstall` can't safely tear down the very service that's
/// handling the request from inside the request handler — instead it
/// spawns a detached copy of itself running `uninstall`, then replies. The
/// spawned process does the real work (including stopping/removing the
/// service, which will terminate this process) a moment later.
pub fn trigger_self_uninstall(state: &Arc<AppState>) {
    state
        .events
        .append(
            homeguard_common::wire::EventKind::Uninstalled,
            "désinstallation déclenchée à distance",
        )
        .ok();

    let Ok(current_exe) = std::env::current_exe() else {
        tracing::error!("impossible de déterminer le chemin de l'exécutable pour se désinstaller");
        return;
    };
    if let Err(e) = std::process::Command::new(current_exe).arg("uninstall").spawn() {
        tracing::error!("échec du lancement de la désinstallation : {e}");
    }
}

pub fn is_installed() -> bool {
    platform::is_installed()
}

pub fn install(exe_path: &std::path::Path, data_dir: &std::path::Path) -> Result<(), ServiceError> {
    platform::install_service(exe_path, data_dir)
}

pub fn uninstall_service_only() -> Result<(), ServiceError> {
    platform::uninstall_service()
}

/// Attempts to run as an OS-managed service using `args`, blocking until the
/// service is told to stop. Returns `None` when this process wasn't actually
/// launched by the service control manager (e.g. an admin ran
/// `homeguard-agent run` by hand, or this platform has no such concept),
/// in which case the caller should fall back to running `run::run` directly
/// in the foreground. Only meaningfully different from `None` on Windows —
/// see that platform module for why.
pub fn try_run_as_service(args: crate::run::RunArgs) -> Option<Result<(), ServiceError>> {
    platform::try_run_as_service(args)
}

#[cfg(target_os = "linux")]
mod platform {
    use super::ServiceError;

    const UNIT_PATH: &str = "/etc/systemd/system/homeguard-agent.service";

    pub fn unit_file_contents(exe_path: &std::path::Path, data_dir: &std::path::Path) -> String {
        format!(
            "[Unit]\n\
Description=HomeGuard Agent\n\
After=network-online.target\n\
Wants=network-online.target\n\
\n\
[Service]\n\
Type=simple\n\
User=root\n\
ExecStart={exe} run --data-dir {data}\n\
Restart=always\n\
RestartSec=5\n\
\n\
[Install]\n\
WantedBy=multi-user.target\n",
            exe = exe_path.display(),
            data = data_dir.display(),
        )
    }

    pub fn install_service(exe_path: &std::path::Path, data_dir: &std::path::Path) -> Result<(), ServiceError> {
        std::fs::write(UNIT_PATH, unit_file_contents(exe_path, data_dir))?;
        run_systemctl(&["daemon-reload"])?;
        run_systemctl(&["enable", "--now", super::SERVICE_NAME])?;
        Ok(())
    }

    pub fn uninstall_service() -> Result<(), ServiceError> {
        // Best-effort: a service that was never installed (agent run by
        // hand during development) shouldn't make uninstall fail.
        let _ = run_systemctl(&["disable", "--now", super::SERVICE_NAME]);
        let _ = std::fs::remove_file(UNIT_PATH);
        let _ = run_systemctl(&["daemon-reload"]);
        Ok(())
    }

    pub fn is_installed() -> bool {
        std::path::Path::new(UNIT_PATH).exists()
    }

    /// systemd starts the unit as an ordinary process (`ExecStart=`) with no
    /// SCM-style dispatch handshake to attempt — the foreground path in
    /// `run.rs` already *is* what systemd runs, signals and all.
    pub fn try_run_as_service(_args: crate::run::RunArgs) -> Option<Result<(), ServiceError>> {
        None
    }

    fn run_systemctl(args: &[&str]) -> Result<(), ServiceError> {
        let status = std::process::Command::new("systemctl").args(args).status()?;
        if !status.success() {
            return Err(ServiceError::CommandFailed(format!("systemctl {args:?}"), status));
        }
        Ok(())
    }
}

#[cfg(target_os = "windows")]
mod platform {
    //! UNVERIFIED — written against the `windows-service` crate's
    //! documented install/dispatch pattern, but this Linux sandbox cannot
    //! compile or run Windows code (it can only cross-compile it — see
    //! `scripts/build-agents.sh`). Test on a real Windows VM before relying
    //! on it; see PLAN.md.

    use super::ServiceError;
    use std::ffi::OsString;
    use std::sync::OnceLock;
    use std::time::Duration;
    use windows_service::service::{
        ServiceAccess, ServiceControl, ServiceControlAccept, ServiceErrorControl, ServiceExitCode,
        ServiceInfo as WinServiceInfo, ServiceStartType, ServiceState, ServiceStatus, ServiceType,
    };
    use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
    use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};
    use windows_service::{define_windows_service, service_dispatcher};

    const SERVICE_TYPE: ServiceType = ServiceType::OWN_PROCESS;

    /// `service_dispatcher::start` hands control to a system-generated
    /// thread that calls `my_service_main(Vec<OsString>)` with no way to
    /// close over local state (it has to be a plain `extern "system" fn").
    /// The SCM's own per-start arguments are normally empty anyway — the
    /// real `--data-dir`/`--port`/etc. are already baked into the service's
    /// `ImagePath` and so arrive via ordinary `std::env::args()`, which is
    /// how `main.rs` parsed `args` in the first place — so we stash that
    /// already-parsed value here rather than re-parsing argv a second time.
    static RUN_ARGS: OnceLock<crate::run::RunArgs> = OnceLock::new();

    /// Set by `my_service_main` just before it returns and read back by
    /// `try_run_as_service` right after the blocking dispatch call
    /// completes, since `run_service`'s `Result` can't cross the
    /// `extern "system" fn` boundary either. `service_dispatcher::start`
    /// only returns once `my_service_main` has already returned (see its
    /// doc comment), so this is set by the time it's read — no extra
    /// synchronization needed beyond the `Mutex` itself.
    static RUN_RESULT: std::sync::Mutex<Option<Result<(), ServiceError>>> = std::sync::Mutex::new(None);

    define_windows_service!(ffi_service_main, my_service_main);

    /// Attempts the SCM handshake; `Err(Winapi(e))` with
    /// `ERROR_FAILED_SERVICE_CONTROLLER_CONNECT` (1063) means this process
    /// wasn't started by the service control manager at all (an admin ran
    /// it directly, or it's `cargo run`'d during development) — that's not
    /// a real failure, just "not applicable here", so the caller falls back
    /// to the ordinary foreground path instead.
    pub fn try_run_as_service(args: crate::run::RunArgs) -> Option<Result<(), ServiceError>> {
        const ERROR_FAILED_SERVICE_CONTROLLER_CONNECT: i32 = 1063;

        // Only ever set once per process — a second `run` invocation in the
        // same process doesn't happen in practice (the service exits the
        // process when it stops), so a stale value from an earlier attempt
        // is not a concern.
        let _ = RUN_ARGS.set(args);

        match service_dispatcher::start(super::SERVICE_NAME, ffi_service_main) {
            Ok(()) => Some(RUN_RESULT.lock().unwrap().take().unwrap_or(Ok(()))),
            Err(windows_service::Error::Winapi(e))
                if e.raw_os_error() == Some(ERROR_FAILED_SERVICE_CONTROLLER_CONNECT) =>
            {
                None
            }
            Err(e) => Some(Err(to_io(e))),
        }
    }

    fn my_service_main(_arguments: Vec<OsString>) {
        let result = run_service();
        if let Err(e) = &result {
            tracing::error!("échec du service Windows : {e}");
        }
        *RUN_RESULT.lock().unwrap() = Some(result);
    }

    fn run_service() -> Result<(), ServiceError> {
        let args = RUN_ARGS
            .get()
            .cloned()
            .expect("try_run_as_service sets RUN_ARGS before dispatching");

        // A fresh single-threaded runtime: this function runs on a plain OS
        // thread the SCM spawned, entirely outside of `main`'s own tokio
        // runtime, so there's no "runtime within a runtime" concern.
        let runtime = tokio::runtime::Runtime::new().map_err(ServiceError::Io)?;

        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<crate::shutdown::ShutdownCause>();
        let shutdown_tx = std::sync::Mutex::new(Some(shutdown_tx));

        // The SCM delivers a distinct control code for an explicit stop
        // versus the machine shutting down — unlike the Unix signal path in
        // `shutdown.rs`, no heuristic is needed, just forward whichever one
        // arrives.
        let event_handler = move |control_event| -> ServiceControlHandlerResult {
            let cause = match control_event {
                ServiceControl::Interrogate => return ServiceControlHandlerResult::NoError,
                ServiceControl::Stop => crate::shutdown::ShutdownCause::Manual,
                ServiceControl::Shutdown | ServiceControl::Preshutdown => {
                    crate::shutdown::ShutdownCause::SystemShutdown
                }
                _ => return ServiceControlHandlerResult::NotImplemented,
            };
            if let Ok(mut slot) = shutdown_tx.lock() {
                if let Some(tx) = slot.take() {
                    let _ = tx.send(cause);
                }
            }
            ServiceControlHandlerResult::NoError
        };

        let status_handle =
            service_control_handler::register(super::SERVICE_NAME, event_handler).map_err(to_io)?;

        status_handle
            .set_service_status(ServiceStatus {
                service_type: SERVICE_TYPE,
                current_state: ServiceState::Running,
                controls_accepted: ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN,
                exit_code: ServiceExitCode::Win32(0),
                checkpoint: 0,
                wait_hint: Duration::default(),
                process_id: None,
            })
            .map_err(to_io)?;

        let run_result = runtime.block_on(crate::run::run(args, Some(shutdown_rx)));

        status_handle
            .set_service_status(ServiceStatus {
                service_type: SERVICE_TYPE,
                current_state: ServiceState::Stopped,
                controls_accepted: ServiceControlAccept::empty(),
                exit_code: ServiceExitCode::Win32(if run_result.is_ok() { 0 } else { 1 }),
                checkpoint: 0,
                wait_hint: Duration::default(),
                process_id: None,
            })
            .map_err(to_io)?;

        run_result.map_err(|e| ServiceError::Io(std::io::Error::other(e.to_string())))
    }

    pub fn install_service(exe_path: &std::path::Path, data_dir: &std::path::Path) -> Result<(), ServiceError> {
        let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CREATE_SERVICE)
            .map_err(to_io)?;

        let service_info = WinServiceInfo {
            name: OsString::from(super::SERVICE_NAME),
            display_name: OsString::from("HomeGuard Agent"),
            service_type: ServiceType::OWN_PROCESS,
            start_type: ServiceStartType::AutoStart,
            error_control: ServiceErrorControl::Normal,
            executable_path: exe_path.to_path_buf(),
            launch_arguments: vec![
                OsString::from("run"),
                OsString::from("--data-dir"),
                OsString::from(data_dir),
            ],
            dependencies: vec![],
            account_name: None, // LocalSystem
            account_password: None,
        };

        let service = manager
            .create_service(&service_info, ServiceAccess::CHANGE_CONFIG | ServiceAccess::START)
            .map_err(to_io)?;
        service.start::<&std::ffi::OsStr>(&[]).map_err(to_io)?;

        // Auto-restart on failure, 5s later, indefinitely.
        let _ = std::process::Command::new("sc")
            .args(["failure", super::SERVICE_NAME, "reset=", "86400", "actions=", "restart/5000"])
            .status();

        Ok(())
    }

    pub fn uninstall_service() -> Result<(), ServiceError> {
        let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)
            .map_err(to_io)?;
        let service = manager
            .open_service(super::SERVICE_NAME, ServiceAccess::STOP | ServiceAccess::DELETE)
            .map_err(to_io)?;
        let _ = service.stop();
        service.delete().map_err(to_io)?;
        Ok(())
    }

    pub fn is_installed() -> bool {
        let Ok(manager) = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT) else {
            return false;
        };
        manager
            .open_service(super::SERVICE_NAME, ServiceAccess::QUERY_STATUS)
            .is_ok()
    }

    fn to_io(e: windows_service::Error) -> ServiceError {
        ServiceError::Io(std::io::Error::other(e.to_string()))
    }
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
mod platform {
    use super::ServiceError;

    pub fn install_service(_exe_path: &std::path::Path, _data_dir: &std::path::Path) -> Result<(), ServiceError> {
        Err(ServiceError::Io(std::io::Error::other(
            "service installation is only supported on Windows and Linux",
        )))
    }

    pub fn uninstall_service() -> Result<(), ServiceError> {
        Ok(())
    }

    pub fn is_installed() -> bool {
        false
    }

    pub fn try_run_as_service(_args: crate::run::RunArgs) -> Option<Result<(), ServiceError>> {
        None
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::platform::unit_file_contents;
    use super::*;
    use crate::config::AgentPaths;

    #[test]
    fn renders_a_systemd_unit_with_the_right_exec_start() {
        let unit = unit_file_contents(
            std::path::Path::new("/usr/local/bin/homeguard-agent"),
            std::path::Path::new("/etc/homeguard"),
        );
        assert!(unit.contains("ExecStart=/usr/local/bin/homeguard-agent run --data-dir /etc/homeguard"));
        assert!(unit.contains("Restart=always"));
        assert!(unit.contains("WantedBy=multi-user.target"));
    }

    #[test]
    fn uninstall_restores_hosts_and_lifts_browser_policy() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AgentPaths::new(dir.path().join("data"));
        paths.ensure_created().unwrap();
        let hosts_path = dir.path().join("hosts");
        let etc_root = dir.path().join("etc");

        // Simulate the agent's usual footprint: a backed-up pristine
        // hosts, a currently-managed hosts, and an active DoH-disabling
        // browser policy.
        std::fs::write(dir.path().join("hosts.homeguard.bak"), "127.0.0.1 localhost\n").unwrap();
        std::fs::write(&hosts_path, "127.0.0.1 localhost\n\n# BEGIN HOMEGUARD\n0.0.0.0 tiktok.com\n# END HOMEGUARD\n").unwrap();
        crate::browser_policy::apply(&etc_root, true).unwrap();
        assert!(etc_root.join("opt/chrome/policies/managed/homeguard.json").exists());

        perform_uninstall(&paths, &hosts_path, &etc_root).unwrap();

        assert_eq!(std::fs::read_to_string(&hosts_path).unwrap(), "127.0.0.1 localhost\n");
        assert!(!etc_root.join("opt/chrome/policies/managed/homeguard.json").exists());
        assert!(!paths.data_dir.exists());
    }
}
