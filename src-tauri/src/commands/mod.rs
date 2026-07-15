pub mod benchmark;
pub mod chat;
pub mod model;
pub mod news;
pub mod search;
pub mod settings;
pub mod update;

/// All bridge commands registered in one place. `lib.rs` calls
/// `.invoke_handler(commands::handler())`.
pub fn handler() -> impl Fn(tauri::ipc::Invoke<tauri::Wry>) -> bool + Send + Sync + 'static {
    tauri::generate_handler![
        model::build_model,
        model::prepare_model,
        model::finalize_model,
        model::list_recent,
        model::open_path,
        model::open_url,
        benchmark::benchmark_peers,
        news::get_news,
        search::web_search,
        search::read_page,
        search::test_mcp,
        chat::list_conversations,
        chat::load_conversation,
        chat::delete_conversation,
        chat::rename_conversation,
        chat::chat_send,
        chat::chat_cancel,
        settings::load_settings,
        settings::save_settings,
        settings::list_models,
        settings::clear_api_key,
        update::check_for_update,
        update::install_update,
        update::restart_app,
    ]
}
