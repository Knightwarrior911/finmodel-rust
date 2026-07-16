pub mod commands;
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
        .setup(|app| {
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
