use tauri::{AppHandle, WebviewWindow};

use crate::{app_state::AppState, error::AppError, platform::window_controller};

#[tauri::command]
pub async fn toggle_chat_window(app: AppHandle) -> Result<(), AppError> {
    window_controller::toggle_chat(&app)
}

#[tauri::command]
pub async fn open_settings_window(app: AppHandle) -> Result<(), AppError> {
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

#[cfg(test)]
mod tests {
    use std::future::Future;

    use super::*;

    fn assert_async_window_command<F, Fut>(_command: F)
    where
        F: Fn(AppHandle) -> Fut,
        Fut: Future<Output = Result<(), AppError>>,
    {
    }

    #[test]
    fn webview_window_creation_commands_remain_async() {
        assert_async_window_command(toggle_chat_window);
        assert_async_window_command(open_settings_window);
    }
}
