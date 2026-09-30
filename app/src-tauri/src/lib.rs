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
            commands::create_assignment,
            commands::delete_assignment,
            commands::exceptions_for,
            commands::create_exception,
            commands::delete_exception,
            commands::recent_history,
            commands::upcoming_transitions,
            commands::start_enrollment,
            commands::stop_enrollment,
            commands::bundled_agent_version,
            commands::ca_fingerprint,
            commands::get_timezone,
            commands::set_timezone,
            commands::export_config,
            commands::import_config,
        ])
        .run(tauri::generate_context!())
        .expect("error while running the HomeGuard application");
}
