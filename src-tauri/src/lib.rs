mod codex_desktop;
mod commands;
mod desktop_boot;
// Domain commands live in commands/*; overview owns desktop state and startup paths.
mod overview;
mod remote;
mod resources;
mod services;

use tauri::{Manager, WebviewWindowBuilder};

pub use overview::{agenthub_paths_for_home, AgentHubPaths, DesktopState};
pub use services::probe::desktop_probe_status_with_state;
pub use services::updates::desktop_update_status_with_state;

pub fn run() {
    let active_handler: fn(tauri::ipc::Invoke<tauri::Wry>) -> bool = tauri::generate_handler![
        codex_desktop::attach,
        codex_desktop::diagnostics,
        codex_desktop::action,
        codex_desktop::detach,
        codex_desktop::open,
        codex_desktop::catalog,
        codex_desktop::choose,
        codex_desktop::create,
        codex_desktop::attachments,
        commands::plans::savePlanMarkdown,
        remote::remoteGet,
        remote::remoteSshHosts,
        remote::remoteSshDelete,
        remote::remoteInstall,
        remote::remoteVerify,
        remote::remoteSave,
        remote::remoteRemove,
        remote::remoteSelect,
        remote::remoteInvoke,
        commands::sessions::readSessionAttachment,
        commands::sessions::previewSessionBatch,
        commands::sessions::executeSessionBatch,
        commands::sessions::searchSessions,
        commands::system::getSystemCapabilities,
        commands::system::getSystemVersion,
        commands::system::listProviders,
        commands::system::getPlatformOverview,
        commands::threads::listThreads,
        commands::threads::getThread,
        commands::threads::getSubagentDetail,
        commands::threads::getThreadBlocks,
        commands::threads::archiveThread,
        commands::threads::restoreThread,
        commands::threads::renameThread,
        commands::probe::getProbeStatus,
        commands::updates::getUpdateStatus,
        commands::updates::updatesCheck,
        commands::updates::updatesInstall,
        commands::settings::getProbeSettings,
        commands::settings::saveProbeSettings,
        commands::settings::getProbeEvents,
        commands::settings::probeBarkTest,
        commands::settings::probeInstallHooks,
        commands::settings::probeRemoveHooks,
        commands::settings::claudeHooks,
        commands::settings::dryRunUnifiedCleanup,
        commands::settings::startUnifiedCleanup,
        commands::settings::dryRunHiddenThreadDelete,
        commands::jobs::listJobs,
        commands::jobs::getJob,
        commands::grok::listGrokSessions,
        commands::grok::getGrokSession,
        commands::grok::renameGrokSession,
        commands::grok::previewGrokSessionDelete,
        commands::grok::deleteGrokSession,
        commands::claude::listClaudeSessions,
        commands::claude::getClaudeSession,
        commands::claude::renameClaudeSession,
        commands::claude::claudeOwnershipDiagnostics,
        commands::claude::previewClaudeSessionDelete,
        commands::claude::deleteClaudeSession,
        commands::provider_sessions::listGrokBotSessions,
        commands::provider_sessions::getGrokBotSession,
        commands::provider_sessions::listAntigravitySessions,
        commands::provider_sessions::getAntigravitySession,
        commands::provider_sessions::renameAntigravitySession,
        commands::provider_sessions::archiveAntigravitySession,
        commands::provider_sessions::restoreAntigravitySession,
        commands::provider_sessions::listDshSessions,
        commands::provider_sessions::getDshSession,
        commands::provider_sessions::renameDshSession,
        commands::provider_sessions::archiveDshSession,
        commands::provider_sessions::restoreDshSession,
    ];
    tauri::Builder::default()
        .append_invoke_initialization_script(desktop_boot::DESKTOP_RUNTIME_MARKER_SCRIPT)
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(active_handler)
        .on_window_event(|window, event| {
            // Wry's single-webview window routes OS drops as WindowEvent;
            // multi-webview surfaces use WebviewEvent below. Neither uses JS.
            if let tauri::WindowEvent::DragDrop(tauri::DragDropEvent::Drop { paths, position }) =
                event
            {
                if let Some(window) = window.app_handle().get_webview_window(window.label()) {
                    codex_desktop::native_drop(window, paths.clone(), *position);
                }
            }
        })
        .on_webview_event(|webview, event| {
            if let tauri::WebviewEvent::DragDrop(tauri::DragDropEvent::Drop { paths, position }) =
                event
            {
                if let Some(window) = webview.app_handle().get_webview_window(webview.label()) {
                    codex_desktop::native_drop(window, paths.clone(), *position);
                }
            }
        })
        .setup(|app| {
            if let Ok(resource_dir) = app.path().resource_dir() {
                resources::sync_agenthub_webd_helper_from_resource(&resource_dir)?;
            }
            let state = DesktopState::current().map_err(|err| err.to_string())?;
            if let Err(error) = resources::repair_probe_error_monitor_launch_agent(
                &state.config(),
                state.platform(),
            ) {
                eprintln!("Probe monitor repair failed: {error}");
            }
            let remote = remote::RemoteConnections::load(&state.config().paths.data_dir)?;
            app.manage(codex_desktop::CodexDesktopBridge::new(
                agenthub_core::services::credentials::CredentialAccess::default(),
            ));
            app.manage(remote);
            app.manage(state);
            let main_window_config = app
                .config()
                .app
                .windows
                .iter()
                .find(|window| window.label == desktop_boot::MAIN_WINDOW_LABEL)
                .ok_or_else(|| "main Tauri window config not found".to_string())?;
            let window = WebviewWindowBuilder::from_config(app.handle(), main_window_config)
                .map_err(|err| err.to_string())?
                .build()
                .map_err(|err| err.to_string())?;
            desktop_boot::initialize_main_window(&window);
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("failed to build AgentHub desktop app")
        .run(|app, event| match event {
            tauri::RunEvent::Exit => {
                app.state::<remote::RemoteConnections>().disconnect();
            }
            tauri::RunEvent::Ready => {
                if let Some(window) = app.get_webview_window(desktop_boot::MAIN_WINDOW_LABEL) {
                    desktop_boot::reveal_main_window(&window);
                }
            }
            #[cfg(target_os = "macos")]
            tauri::RunEvent::Reopen { .. } => {
                if let Some(window) = app.get_webview_window(desktop_boot::MAIN_WINDOW_LABEL) {
                    desktop_boot::reveal_main_window(&window);
                }
            }
            _ => {}
        });
}

#[cfg(test)]
mod entry_guard_tests;
