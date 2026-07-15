//! News-headlines bridge command — Google News RSS via fm-fetch (no key).

use crate::error::{AppError, AppResult};

/// Fetch up to `limit` (default 5) headlines for a ticker/query. Returns a JSON
/// array of `{ title, source, url, published }`.
#[tauri::command(rename_all = "snake_case")]
pub async fn get_news(query: String, limit: Option<usize>) -> AppResult<String> {
    let lim = limit.unwrap_or(5).clamp(1, 20);
    tauri::async_runtime::spawn_blocking(move || {
        let hs = fm_fetch::fetch_headlines(&query, lim)
            .map_err(|e| AppError::Engine(format!("news fetch failed: {e}")))?;
        serde_json::to_string(&hs).map_err(|e| AppError::Engine(e.to_string()))
    })
    .await
    .map_err(|e| AppError::Engine(format!("news task failed: {e}")))?
}
