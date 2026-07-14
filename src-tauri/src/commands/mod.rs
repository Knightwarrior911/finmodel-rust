pub mod benchmark;
pub mod model;
pub mod settings;
pub mod update;

/// All bridge commands registered in one place. `lib.rs` calls
/// `.invoke_handler(commands::handler())`.
pub fn handler() -> impl Fn(tauri::ipc::Invoke<tauri::Wry>) -> bool + Send + Sync + 'static {
    tauri::generate_handler![
        model::build_model,
        model::open_path,
        benchmark::benchmark_peers,
        settings::load_settings,
        settings::save_settings,
        settings::list_models,
        update::check_for_update,
        update::install_update,
    ]
}
