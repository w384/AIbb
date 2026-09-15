use std::sync::Mutex;

use tauri::{AppHandle, Manager, PhysicalPosition};

use crate::{app_state::AppState, error::AppError, platform::window_controller};

#[tauri::command]
pub async fn toggle_chat_window(app: AppHandle) -> Result<(), AppError> {
    let profile = app.state::<AppState>().settings.load_aibb_profile().await?;
    window_controller::toggle_chat(&app, &profile.name)
}

#[tauri::command]
pub async fn open_settings_window(app: AppHandle) -> Result<(), AppError> {
    let profile = app.state::<AppState>().settings.load_aibb_profile().await?;
    window_controller::open_settings(&app, &profile.name)
}

#[tauri::command]
pub fn exit_app(app: AppHandle) {
    app.exit(0);
}

/// Anchor of an in-flight manual pet drag: the window position and the pointer
/// screen coordinates (CSS pixels) at the moment the drag started. Moving the
/// window manually instead of using the system `start_dragging` keeps the
/// transparent window transparent the whole time — no white block, no
/// rectangular WebView snapshot while it moves.
#[derive(Clone, Copy)]
struct PetDragAnchor {
    window_x: i32,
    window_y: i32,
    screen_x: f64,
    screen_y: f64,
}

static PET_DRAG_ANCHOR: Mutex<Option<PetDragAnchor>> = Mutex::new(None);

#[tauri::command]
pub fn pet_drag_begin(
    app: AppHandle,
    screen_x: f64,
    screen_y: f64,
) -> Result<(), AppError> {
    let window = window_controller::pet_window(&app)?;
    let position = window
        .outer_position()
        .map_err(|error| window_error("read pet drag start position", error))?;
    *PET_DRAG_ANCHOR
        .lock()
        .map_err(|_| drag_state_error())? = Some(PetDragAnchor {
        window_x: position.x,
        window_y: position.y,
        screen_x,
        screen_y,
    });
    Ok(())
}

#[tauri::command]
pub fn pet_drag_move(
    app: AppHandle,
    screen_x: f64,
    screen_y: f64,
) -> Result<(), AppError> {
    let Some(anchor) = *PET_DRAG_ANCHOR
        .lock()
        .map_err(|_| drag_state_error())?
    else {
        return Ok(());
    };
    let window = window_controller::pet_window(&app)?;
    let scale = window
        .scale_factor()
        .map_err(|error| window_error("read pet drag scale factor", error))?;
    let delta_x = (screen_x - anchor.screen_x) * scale;
    let delta_y = (screen_y - anchor.screen_y) * scale;
    window
        .set_position(PhysicalPosition::new(
            anchor.window_x + delta_x.round() as i32,
            anchor.window_y + delta_y.round() as i32,
        ))
        .map_err(|error| window_error("move pet window", error))
}

#[tauri::command]
pub fn pet_drag_end(app: AppHandle) -> Result<(), AppError> {
    *PET_DRAG_ANCHOR
        .lock()
        .map_err(|_| drag_state_error())? = None;
    let window = window_controller::pet_window(&app)?;
    window_controller::snap_pet_to_work_area_edge(&window)?;
    let position = window
        .outer_position()
        .map_err(|error| window_error("read pet drag end position", error))?;
    let state = app.state::<AppState>();
    window_controller::save_pet_position(&state, position.x, position.y)
}

#[tauri::command]
pub fn save_pet_position(
    state: tauri::State<'_, AppState>,
    x: i32,
    y: i32,
) -> Result<(), AppError> {
    window_controller::save_pet_position(&state, x, y)
}

fn window_error(action: &str, error: tauri::Error) -> AppError {
    AppError::new(
        "windowOperationFailed",
        format!("Failed to {action}: {error}"),
    )
}

fn drag_state_error() -> AppError {
    AppError::new(
        "windowOperationFailed",
        "The pet drag state could not be accessed.",
    )
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
