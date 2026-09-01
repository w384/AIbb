use tauri::{AppHandle, WebviewWindow};

use crate::{app_state::AppState, error::AppError, platform::window_controller};

#[tauri::command]
pub fn toggle_chat_window(app: AppHandle) -> Result<(), AppError> {
    window_controller::toggle_chat(&app)
}

#[tauri::command]
pub fn open_settings_window(app: AppHandle) -> Result<(), AppError> {
    window_controller::open_settings(&app)
}

#[tauri::command]
pub fn exit_app(app: AppHandle) {
    app.exit(0);
}

#[tauri::command]
pub fn start_pet_drag(window: WebviewWindow) -> Result<(), AppError> {
    window_controller::start_pet_drag(&window)
}

#[tauri::command]
pub fn save_pet_position(
    state: tauri::State<'_, AppState>,
    x: i32,
    y: i32,
) -> Result<(), AppError> {
    window_controller::save_pet_position(&state, x, y)
}
