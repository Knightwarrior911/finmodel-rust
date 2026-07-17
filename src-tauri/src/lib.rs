pub mod agent;
pub mod commands;
pub mod store;
mod error;

use tauri::{DragDropEvent, Emitter, Manager, WindowEvent};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(commands::model::SessionCache::default())
        .manage(commands::run::RunRegistry::default())
        .manage(commands::mcp::McpManager::default())
        .manage(commands::research_state::ResearchRunState::default())
        .manage(commands::artifacts::ArtifactRegistry::default())
        .manage(agent::registry::ActorRegistry::default())
        .setup(|app| {
            // Open the SQLite store (Phase A). The legacy JSON conversation
            // directory stays the live source of truth until the Phase G
            // cutover, so the import here is non-destructive and a store failure
            // must never take the app down.
            match app.path().app_config_dir() {
                Ok(dir) => match store::init(&dir) {
                    Ok((handle, report, workspace_id)) => {
                        if !report.quarantined.is_empty() {
                            let _ = app.handle().emit(
                                "store_migration_notice",
                                serde_json::json!({
                                    "imported": report.imported_conversations,
                                    "quarantined": report.quarantined,
                                }),
                            );
                        }
                        app.manage(store::AppStore {
                            handle,
                            default_workspace_id: workspace_id,
                        });
                    }
                    Err(e) => eprintln!("store init failed (continuing on JSON): {e}"),
                },
                Err(e) => eprintln!("no app_config_dir; store disabled: {e}"),
            }
            // Auto-updater (desktop only) — verifies signed releases against the
            // minisign pubkey in tauri.conf.json before installing.
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            {
                app.handle()
                    .plugin(tauri_plugin_updater::Builder::new().build())?;
            }
            // Capture OS drag-drop paths in Rust as one-use grants, then notify
            // the UI. The UI claims only after `pdf_drop_ready` — never races
            // the webview drag-drop event against Rust observation.
            if let Some(win) = app.get_webview_window("main") {
                let handle = app.handle().clone();
                win.on_window_event(move |event| {
                    if let WindowEvent::DragDrop(DragDropEvent::Drop { paths, .. }) = event {
                        if let Some(reg) =
                            handle.try_state::<commands::artifacts::ArtifactRegistry>()
                        {
                            let n = reg.observe_drop(paths);
                            if n > 0 {
                                let _ = handle
                                    .emit("pdf_drop_ready", serde_json::json!({ "count": n }));
                            }
                        }
                    }
                });
            }
            Ok(())
        })
        .invoke_handler(commands::handler())
        .run(tauri::generate_context!())
        .expect("error while running finmodel application");
}
