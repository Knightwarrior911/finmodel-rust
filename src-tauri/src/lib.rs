pub mod agent;
pub mod commands;
mod error;
pub mod store;

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
            // Fail fast (debug) if any workflow references an unregistered tool;
            // in release, log and continue (the registry stays authoritative).
            if let Err((wf, tool)) = agent::tools::ToolRegistry::shared().validate_workflows() {
                let msg = format!("workflow '{wf}' references unregistered tool '{tool}'");
                debug_assert!(false, "{msg}");
                eprintln!("startup: {msg}");
            }
            // Open the SQLite store (Phase A). The legacy JSON conversation
            // directory stays the live source of truth until the Phase G
            // cutover, so the import here is non-destructive and a store failure
            // must never take the app down.
            match app.path().app_config_dir() {
                Ok(dir) => {
                    // Seed the bundled IB/financial-analysis skills and the
                    // starter agent bench once (never overwrites user files;
                    // deletions stay sticky per the .seeded marker).
                    let _ = agent::skills::seed_builtin_skills(&dir);
                    let _ = agent::agents::seed_builtin_agents(&dir);
                    // Re-register persisted Recent artifacts (generated memos,
                    // models, decks + their folders) so open_path allowlists
                    // them after a restart — otherwise reloaded card Open /
                    // Show-in-folder buttons fail until a new file is made.
                    commands::model::rehydrate_recent(app.handle());
                    match store::init(&dir) {
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
                            // Approval-expiry sweep (Task 4.3): every 60s, expire
                            // pending approvals older than the 10-minute safety window
                            // and DENY their parked oneshots so `await_approval` never
                            // wedges. The 600s cutoff matches the in-driver timeout, so
                            // the sweep never denies earlier than existing behavior.
                            if let (Some(store), Some(reg)) = (
                                app.try_state::<store::AppStore>(),
                                app.try_state::<agent::registry::ActorRegistry>(),
                            ) {
                                let handle = store.handle.clone();
                                let registry = (*reg).clone();
                                tauri::async_runtime::spawn(async move {
                                    let mut ticker =
                                        tokio::time::interval(std::time::Duration::from_secs(60));
                                    loop {
                                        ticker.tick().await;
                                        let now = store::now_iso();
                                        let cutoff = store::iso_seconds_ago(600);
                                        let _ = agent::approvals::expire_and_deny_stale_approvals(
                                            &handle, &registry, &cutoff, &now,
                                        )
                                        .await;
                                    }
                                });
                            }
                            // Skill aging (Task 7.3): once at startup then daily, mark
                            // long-unused skills `stale` (30d) then `archived` (90d) so
                            // they leave the default catalog while staying restorable.
                            if let Some(store) = app.try_state::<store::AppStore>() {
                                let handle = store.handle.clone();
                                tauri::async_runtime::spawn(async move {
                                    let mut ticker = tokio::time::interval(
                                        std::time::Duration::from_secs(24 * 60 * 60),
                                    );
                                    loop {
                                        ticker.tick().await;
                                        let now = store::now_iso();
                                        let stale = store::iso_seconds_ago(30 * 24 * 60 * 60);
                                        let archive = store::iso_seconds_ago(90 * 24 * 60 * 60);
                                        let _ = handle
                                            .call(move |db| db.age_skills(&stale, &archive, &now))
                                            .await;
                                    }
                                });
                            }
                        }
                        Err(e) => eprintln!("store init failed (continuing on JSON): {e}"),
                    }
                }
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
            // the UI. The UI claims only after `file_drop_ready` — never races
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
                                    .emit("file_drop_ready", serde_json::json!({ "count": n }));
                            }
                        }
                    }
                });
            }
            // Scheduler tick (Task 8.3, live): every 60s, claim due schedules
            // and launch their runs. First sweep after a short boot delay so
            // store/registry state is managed before any launch.
            {
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_secs(15)).await;
                    loop {
                        commands::agent::run_due_schedules(&handle).await;
                        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                    }
                });
            }
            Ok(())
        })
        .invoke_handler(commands::handler())
        .run(tauri::generate_context!())
        .expect("error while running finmodel application");
}
