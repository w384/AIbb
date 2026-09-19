use crate::{app_state::AppState, error::AppError};

#[tauri::command]
pub async fn clear_memory(state: tauri::State<'_, AppState>) -> Result<(), AppError> {
    if let Some(exploration) = state.exploration.as_ref() {
        return exploration.cancel_all_and_clear_memory(&state.memory).await;
    }
    state.memory.clear_memory().await
}

/// Clears only the vocabulary channel; the ordinary chat, summaries and
/// outing records stay untouched.
#[tauri::command]
pub async fn clear_vocab_memory(state: tauri::State<'_, AppState>) -> Result<(), AppError> {
    state.memory.clear_vocab_memory().await
}
