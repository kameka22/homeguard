mod app_identity;
mod commands;
#[cfg(target_os = "macos")]
mod keychain_ca_store;
mod net;
mod state;

use tauri::Manager;

use crate::state::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt::init();

    tauri::Builder::default()
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            let state = AppState::init(&data_dir)?;
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_agents,
            commands::set_agent_display_name,
            commands::remove_agent,
            commands::discover_agents,
            commands::discover_and_sync_all,
            commands::probe_agent_by_ip,
            commands::sync_agent,
            commands::refresh_agents_status,
            commands::update_agent,
            commands::update_all_outdated_agents,
            commands::list_groups,
            commands::create_group,
            commands::delete_group,
            commands::add_group_member,
            commands::remove_group_member,
            commands::agent_group_ids,
            commands::list_lists,
            commands::create_list,
            commands::update_list_domains,
            commands::delete_list,
            commands::assignments_for,
            commands::list_assignments,
            commands::list_exceptions,
            commands::create_assignment,
            commands::delete_assignment,
            commands::update_assignment_schedule,
            commands::exceptions_for,
            commands::create_exception,
            commands::delete_exception,
            commands::recent_history,
            commands::upcoming_transitions,
            commands::fleet_timeline,
            commands::start_enrollment,
            commands::stop_enrollment,
            commands::bundled_agent_version,
            commands::ca_fingerprint,
            commands::ca_details,
            commands::get_timezone,
            commands::set_timezone,
            commands::get_theme,
            commands::set_theme,
            commands::get_onboarding_done,
            commands::set_onboarding_done,
            commands::get_scan_on_launch,
            commands::set_scan_on_launch,
            commands::get_auto_update_agents,
            commands::set_auto_update_agents,
            commands::get_default_unblock_minutes,
            commands::set_default_unblock_minutes,
            commands::get_auto_www,
            commands::set_auto_www,
            commands::list_aliases,
            commands::upsert_alias,
            commands::delete_alias,
            commands::write_text_file,
            commands::export_config,
            commands::import_config,
        ])
        .run(tauri::generate_context!())
        .expect("error while running the HomeGuard application");
}
