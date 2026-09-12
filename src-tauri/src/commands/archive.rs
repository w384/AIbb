use tauri::{AppHandle, Emitter};

use crate::{
    app_state::AppState,
    archive::models::{
        ArchiveFileResult, ArchiveLedgerEntry, ArchiveSettingsView, DiscoveredStructure,
        SaveArchiveSettings,
    },
    error::AppError,
    platform::window_controller,
};

#[tauri::command]
pub async fn archive_files(
    state: tauri::State<'_, AppState>,
    paths: Vec<String>,
    project: String,
) -> Result<Vec<ArchiveFileResult>, AppError> {
    state.archive.archive_files(paths, project).await
}

#[tauri::command]
pub async fn load_archive_settings(
    state: tauri::State<'_, AppState>,
) -> Result<ArchiveSettingsView, AppError> {
    state.archive.load_settings().await
}

#[tauri::command]
pub async fn save_archive_settings(
    state: tauri::State<'_, AppState>,
    settings: SaveArchiveSettings,
) -> Result<(), AppError> {
    state.archive.save_settings(settings).await
}

#[tauri::command]
pub async fn discover_archive_structure(
    state: tauri::State<'_, AppState>,
) -> Result<Option<DiscoveredStructure>, AppError> {
    state.archive.discover_structure().await
}

#[tauri::command]
pub async fn take_pending_archive_paths(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<String>, AppError> {
    Ok(state.archive.take_pending_paths().await)
}

#[tauri::command]
pub async fn open_archive_window(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    paths: Vec<String>,
) -> Result<(), AppError> {
    if !paths.is_empty() {
        state.archive.set_pending_paths(paths.clone()).await;
    }
    let profile_name = state.settings.load_aibb_profile().await?.name;
    window_controller::open_chat(&app, &profile_name)?;
    // Let an already-open chat window pick up the pending paths without
    // needing to remount (the fresh-window path takes them on mount).
    let _ = app.emit("archive://pending", paths.len());
    Ok(())
}

#[tauri::command]
pub async fn archive_ledger(
    state: tauri::State<'_, AppState>,
    limit: Option<u32>,
) -> Result<Vec<ArchiveLedgerEntry>, AppError> {
    state.archive.ledger(limit.unwrap_or(20) as usize).await
}
