use crate::{app_state::AppState, error::AppError};

#[tauri::command]
pub async fn clear_memory(state: tauri::State<'_, AppState>) -> Result<(), AppError> {
    state.memory.clear_memory().await
}
