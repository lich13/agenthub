use tauri::{
    menu::{Menu, MenuItem},
    Manager,
};

pub(crate) fn install(app: &tauri::AppHandle) -> tauri::Result<()> {
    let menu = Menu::default(app)?;
    for item in menu.items()? {
        let Some(edit) = item.as_submenu() else {
            continue;
        };
        if edit.text()? != "Edit" {
            continue;
        }
        for (index, item) in edit.items()?.into_iter().enumerate() {
            if let Some(action) = item.as_predefined_menuitem() {
                if action.text()? == "Select All" {
                    edit.remove_at(index)?;
                    edit.insert(
                        &MenuItem::with_id(
                            app,
                            "agenthub-select-all",
                            "Select All",
                            true,
                            Some("CmdOrCtrl+A"),
                        )?,
                        index,
                    )?;
                }
            }
        }
    }
    app.set_menu(menu)?;
    app.on_menu_event(|app, event| {
        if event.id().as_ref() == "agenthub-select-all" {
            if let Some(window) = app.get_webview_window(crate::desktop_boot::MAIN_WINDOW_LABEL) {
                let _ = window.eval(include_str!("select_all.js"));
            }
        }
    });
    Ok(())
}
