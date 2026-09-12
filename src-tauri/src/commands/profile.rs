use tauri::{AppHandle, Emitter, Manager, Runtime};

use crate::{
    app_state::AppState, domain::AibbProfile, error::AppError,
    platform::avatar_icons, platform::window_controller::profile_window_title,
};

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
    refresh_avatar_icons(&app, &state).await;
    Ok(profile)
}

#[tauri::command]
pub async fn reset_aibb_avatar(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<AibbProfile, AppError> {
    let profile = state.settings.reset_aibb_avatar().await?;
    publish_profile_updated(&app, &profile);
    if let Ok(directory) = app.path().app_data_dir() {
        avatar_icons::apply_avatar_icons(&app, &directory, None);
    }
    Ok(profile)
}

/// Best-effort: re-read the stored avatar and push it into the tray icon and
/// the desktop shortcut. Failures here must never fail the avatar save.
async fn refresh_avatar_icons<R: Runtime>(
    app: &AppHandle<R>,
    state: &tauri::State<'_, AppState>,
) {
    let Ok(directory) = app.path().app_data_dir() else {
        return;
    };
    let bytes = state.settings.load_avatar_bytes().await.ok().flatten();
    avatar_icons::apply_avatar_icons(app, &directory, bytes.as_deref());
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
            title: profile_window_title("pet", &profile.name),
            payload: profile,
        },
        ProfileWindowUpdate {
            label: "chat",
            title: profile_window_title("chat", &profile.name),
            payload: profile,
        },
        ProfileWindowUpdate {
            label: "settings",
            title: profile_window_title("settings", &profile.name),
            payload: profile,
        },
    ]
}
