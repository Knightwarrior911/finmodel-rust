pub mod agent;
pub mod analysis;
pub mod artifacts;
pub mod attachments;
pub mod benchmark;
pub mod cache;
pub mod chat;
pub mod dataroom;
pub mod mcp;
pub mod model;
pub mod news;
pub mod omp_gateway;
pub mod research;
pub mod research_state;
pub mod run;
pub mod search;
pub mod secrets;
pub mod settings;
pub mod subscription;
pub mod update;

/// All bridge commands registered in one place. `lib.rs` calls
/// `.invoke_handler(commands::handler())`.
pub fn handler() -> impl Fn(tauri::ipc::Invoke<tauri::Wry>) -> bool + Send + Sync + 'static {
    tauri::generate_handler![
        model::build_model,
        model::prepare_model,
        model::finalize_model,
        model::analyze_pdf,
        model::list_recent,
        model::open_path,
        model::open_url,
        benchmark::benchmark_peers,
        analysis::ev_bridge,
        analysis::ifrs_bridge,
        analysis::tie_out,
        news::get_news,
        search::web_search,
        search::read_page,
        search::test_mcp,
        chat::financials_card,
        chat::list_conversations,
        chat::load_conversation,
        chat::delete_conversation,
        chat::rename_conversation,
        research_state::research_retry,
        agent::agent_cancel,
        agent::agent_pause,
        agent::agent_resume,
        agent::agent_send,
        agent::schedule_create,
        agent::memory_add,
        agent::schedules_list,
        agent::schedule_cancel,
        agent::agent_approve,
        agent::list_active_runs,
        agent::get_run_events_after,
        agent::get_run_snapshot,
        agent::memory_list,
        agent::memory_delete,
        agent::memory_pin,
        agent::memory_edit,
        agent::grounding_get_global,
        agent::grounding_set_global,
        agent::grounding_get_project,
        agent::grounding_set_project,
        agent::projects_list,
        agent::project_create,
        agent::project_rename,
        agent::project_delete,
        agent::conversation_set_project,
        agent::agents_list,
        agent::agents_get,
        agent::agents_save,
        agent::agents_delete,
        agent::skills_list,
        agent::skills_get,
        agent::skills_save,
        agent::skills_delete,
        agent::skill_suggest,
        agent::skill_restore,
        research::review_suggested_assumptions,
        settings::load_settings,
        settings::save_settings,
        settings::list_models,
        settings::test_model,
        settings::clear_api_key,
        artifacts::pick_pdf_artifact,
        artifacts::claim_dropped_file,
        attachments::stage_attachment,
        settings::set_model,
        settings::refine_prompt,
        subscription::subscription_providers_status,
        subscription::import_opencode_go_key,
        subscription::connect_opencode_go,
        subscription::probe_cursor_models,
        omp_gateway::ensure_cursor_omp_gateway,
        omp_gateway::use_cursor_omp,
        omp_gateway::connect_cursor_omp,
        update::check_for_update,
        update::install_update,
        update::restart_app,
    ]
}
