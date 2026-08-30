use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use crate::{app_state::AppState, error::AppError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Position {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Size {
    pub width: i32,
    pub height: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkArea {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

pub fn toggle_chat(app: &AppHandle) -> Result<(), AppError> {
    let window = get_or_create_window(app, "chat", "AIbb Chat", 420.0, 620.0)?;
    let is_visible = window
        .is_visible()
        .map_err(|error| window_error("read chat window visibility", error))?;

    if is_visible {
        window
            .hide()
            .map_err(|error| window_error("hide chat window", error))
    } else {
        window
            .show()
            .map_err(|error| window_error("show chat window", error))?;
        window
            .set_focus()
            .map_err(|error| window_error("focus chat window", error))
    }
}

pub fn open_settings(app: &AppHandle) -> Result<(), AppError> {
    let window = get_or_create_window(app, "settings", "AIbb Settings", 520.0, 640.0)?;
    window
        .show()
        .map_err(|error| window_error("show settings window", error))?;
    window
        .set_focus()
        .map_err(|error| window_error("focus settings window", error))
}

pub fn start_pet_drag(window: &WebviewWindow) -> Result<(), AppError> {
    window
        .start_dragging()
        .map_err(|error| window_error("start pet drag", error))
}

pub fn save_pet_position(state: &AppState, x: i32, y: i32) -> Result<(), AppError> {
    let mut position = state.pet_position.write().map_err(|_| AppError {
        code: "stateUnavailable".to_string(),
        message: "Application state is unavailable.".to_string(),
    })?;
    *position = Some((x, y));
    Ok(())
}

pub fn clamp_position(position: Position, size: Size, work_area: WorkArea) -> Position {
    let max_x = work_area
        .x
        .saturating_add(work_area.width)
        .saturating_sub(size.width)
        .max(work_area.x);
    let max_y = work_area
        .y
        .saturating_add(work_area.height)
        .saturating_sub(size.height)
        .max(work_area.y);

    Position {
        x: position.x.clamp(work_area.x, max_x),
        y: position.y.clamp(work_area.y, max_y),
    }
}

fn get_or_create_window(
    app: &AppHandle,
    label: &str,
    title: &str,
    width: f64,
    height: f64,
) -> Result<WebviewWindow, AppError> {
    if let Some(window) = app.get_webview_window(label) {
        return Ok(window);
    }

    WebviewWindowBuilder::new(
        app,
        label,
        WebviewUrl::App(format!("index.html?window={label}").into()),
    )
    .title(title)
    .inner_size(width, height)
    .visible(false)
    .build()
    .map_err(|error| window_error("create application window", error))
}

fn window_error(action: &str, error: tauri::Error) -> AppError {
    AppError {
        code: "windowOperationFailed".to_string(),
        message: format!("Failed to {action}: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::RwLock;

    use crate::{
        app_state::AppState,
        domain::{BootstrapState, PetStatus},
    };

    #[test]
    fn clamps_a_pet_that_would_be_off_the_right_and_bottom_edges() {
        let clamped = clamp_position(
            Position { x: 1900, y: 1060 },
            Size {
                width: 220,
                height: 240,
            },
            WorkArea {
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
            },
        );

        assert_eq!(clamped, Position { x: 1700, y: 840 });
    }

    #[test]
    fn clamps_a_pet_that_would_be_off_the_left_and_top_edges() {
        let clamped = clamp_position(
            Position { x: -40, y: -20 },
            Size {
                width: 220,
                height: 240,
            },
            WorkArea {
                x: 10,
                y: 30,
                width: 1920,
                height: 1080,
            },
        );

        assert_eq!(clamped, Position { x: 10, y: 30 });
    }

    #[test]
    fn saves_the_latest_pet_position_in_application_memory() {
        let state = AppState {
            bootstrap: RwLock::new(BootstrapState {
                first_run: true,
                pet_status: PetStatus::Idle,
                api_configured: false,
            }),
            pet_position: RwLock::new(None),
        };

        save_pet_position(&state, 320, 180).unwrap();

        assert_eq!(*state.pet_position.read().unwrap(), Some((320, 180)));
    }
}
