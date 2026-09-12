use crate::{
    app_state::AppState,
    error::AppError,
    settings::{ApiSettings, SaveSettings},
};

#[tauri::command]
pub async fn load_settings(state: tauri::State<'_, AppState>) -> Result<ApiSettings, AppError> {
    state.settings.load().await
}

#[tauri::command]
pub async fn save_settings(
    state: tauri::State<'_, AppState>,
    settings: SaveSettings,
) -> Result<(), AppError> {
    state.settings.save(settings).await
}

#[tauri::command]
pub async fn clear_api_key(state: tauri::State<'_, AppState>) -> Result<(), AppError> {
    state.settings.clear_api_key().await
}

#[tauri::command]
pub async fn test_connection(state: tauri::State<'_, AppState>) -> Result<(), AppError> {
    state.settings.test_connection().await
}

#[tauri::command]
pub async fn list_available_models(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<String>, AppError> {
    state.settings.list_available_models().await
}
