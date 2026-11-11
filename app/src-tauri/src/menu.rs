//! The native menu bar. Tauri's own [`tauri::menu::Menu::default`] builds
//! a perfectly reasonable default (app menu, Edit, Window, Help...) but
//! has no way to insert into it — there's no public "add at index" API —
//! so this mirrors that same structure by hand, the one addition being
//! "Préférences…" (⌘,) in the macOS app menu, which every native app on
//! this platform is expected to have and which this app otherwise has no
//! other way to reach (the sidebar's old gear button is gone — this menu
//! item is now the only way in).

use tauri::menu::{AboutMetadata, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Emitter, Runtime};

pub const PREFERENCES_ITEM_ID: &str = "preferences";
/// What `App.svelte` listens for to open the Réglages modal — the exact
/// same thing the old gear button's click used to do locally.
pub const OPEN_SETTINGS_EVENT: &str = "open-settings";

pub fn build<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    let pkg_info = app.package_info();
    let config = app.config();
    let about_metadata = AboutMetadata {
        name: Some(pkg_info.name.clone()),
        version: Some(pkg_info.version.to_string()),
        copyright: config.bundle.copyright.clone(),
        authors: config.bundle.publisher.clone().map(|p| vec![p]),
        ..Default::default()
    };

    let preferences = MenuItem::with_id(app, PREFERENCES_ITEM_ID, "Préférences…", true, Some("CmdOrCtrl+,"))?;

    let window_menu = Submenu::with_items(
        app,
        "Window",
        true,
        &[
            &PredefinedMenuItem::minimize(app, None)?,
            &PredefinedMenuItem::maximize(app, None)?,
            #[cfg(target_os = "macos")]
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::close_window(app, None)?,
        ],
    )?;

    let help_menu = Submenu::with_items(
        app,
        "Help",
        true,
        &[
            #[cfg(not(target_os = "macos"))]
            &PredefinedMenuItem::about(app, None, Some(about_metadata))?,
        ],
    )?;

    Menu::with_items(
        app,
        &[
            #[cfg(target_os = "macos")]
            &Submenu::with_items(
                app,
                pkg_info.name.clone(),
                true,
                &[
                    &PredefinedMenuItem::about(app, None, Some(about_metadata))?,
                    &PredefinedMenuItem::separator(app)?,
                    &preferences,
                    &PredefinedMenuItem::separator(app)?,
                    &PredefinedMenuItem::services(app, None)?,
                    &PredefinedMenuItem::separator(app)?,
                    &PredefinedMenuItem::hide(app, None)?,
                    &PredefinedMenuItem::hide_others(app, None)?,
                    &PredefinedMenuItem::separator(app)?,
                    &PredefinedMenuItem::quit(app, None)?,
                ],
            )?,
            #[cfg(not(target_os = "macos"))]
            &Submenu::with_items(app, "File", true, &[&preferences, &PredefinedMenuItem::quit(app, None)?])?,
            &Submenu::with_items(
                app,
                "Edit",
                true,
                &[
                    &PredefinedMenuItem::undo(app, None)?,
                    &PredefinedMenuItem::redo(app, None)?,
                    &PredefinedMenuItem::separator(app)?,
                    &PredefinedMenuItem::cut(app, None)?,
                    &PredefinedMenuItem::copy(app, None)?,
                    &PredefinedMenuItem::paste(app, None)?,
                    &PredefinedMenuItem::select_all(app, None)?,
                ],
            )?,
            #[cfg(target_os = "macos")]
            &Submenu::with_items(app, "View", true, &[&PredefinedMenuItem::fullscreen(app, None)?])?,
            &window_menu,
            &help_menu,
        ],
    )
}

/// `Preferences…` is the only item this app's menu adds beyond Tauri's
/// own predefined ones (which handle themselves — about, quit, cut/copy/
/// paste, etc.), so there's nothing to dispatch on here yet.
pub fn handle_event<R: Runtime>(app: &AppHandle<R>, event: MenuEvent) {
    if event.id() == PREFERENCES_ITEM_ID {
        let _ = app.emit(OPEN_SETTINGS_EVENT, ());
    }
}
