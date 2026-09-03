use tauri::{AppHandle, Emitter, Manager, Runtime};

use crate::{app_state::AppState, domain::AibbProfile, error::AppError};

pub const PROFILE_UPDATED_EVENT: &str = "profile://updated";

pub(crate) struct ProfileWindowUpdate<'a> {
    pub label: &'static str,
    pub title: String,
    pub payload: &'a AibbProfile,
}

#[tauri::command]
pub async fn load_aibb_profile(state: tauri::State<'_, AppState>) -> Result<AibbProfile, AppError> {
    state.settings.load_aibb_profile().await
}

#[tauri::command]
pub async fn save_aibb_name(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    name: String,
) -> Result<AibbProfile, AppError> {
    let profile = state.settings.save_aibb_name(name).await?;
    publish_profile_updated(&app, &profile);
    Ok(profile)
}

#[tauri::command]
pub async fn save_aibb_avatar(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    bytes: Vec<u8>,
    mime_type: String,
) -> Result<AibbProfile, AppError> {
    let profile = state.settings.save_aibb_avatar(bytes, mime_type).await?;
    publish_profile_updated(&app, &profile);
    Ok(profile)
}

#[tauri::command]
pub async fn reset_aibb_avatar(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<AibbProfile, AppError> {
    let profile = state.settings.reset_aibb_avatar().await?;
    publish_profile_updated(&app, &profile);
    Ok(profile)
}

pub(crate) fn publish_profile_updated<R: Runtime>(app: &AppHandle<R>, profile: &AibbProfile) {
    dispatch_profile_updates(profile, |update| {
        let Some(window) = app.get_webview_window(update.label) else {
            return Ok::<(), ()>(());
        };
        let _ = window.set_title(&update.title);
        let _ = window.emit(PROFILE_UPDATED_EVENT, update.payload.clone());
        Ok(())
    });
}

pub(crate) fn dispatch_profile_updates<E>(
    profile: &AibbProfile,
    mut dispatch: impl FnMut(&ProfileWindowUpdate<'_>) -> Result<(), E>,
) {
    for update in profile_window_updates(profile) {
        let _ = dispatch(&update);
    }
}

pub(crate) fn profile_window_updates(profile: &AibbProfile) -> [ProfileWindowUpdate<'_>; 3] {
    [
        ProfileWindowUpdate {
            label: "pet",
            title: profile.name.clone(),
            payload: profile,
        },
        ProfileWindowUpdate {
            label: "chat",
            title: format!("{} Chat", profile.name),
            payload: profile,
        },
        ProfileWindowUpdate {
            label: "settings",
            title: format!("{} Settings", profile.name),
            payload: profile,
        },
    ]
}
