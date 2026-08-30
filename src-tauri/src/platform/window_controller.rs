use tauri::{
    AppHandle, Manager, PhysicalPosition, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
};

use crate::{app_state::AppState, error::AppError, settings::SettingsService};

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
    state.settings.save_pet_position(x, y)
}

pub fn restored_pet_position(
    settings: &SettingsService,
    size: Size,
    work_area: WorkArea,
) -> Result<Option<Position>, AppError> {
    Ok(settings
        .persisted_settings()?
        .pet_position
        .map(|(x, y)| clamp_position(Position { x, y }, size, work_area)))
}

pub fn restore_pet_window_position(
    app: &AppHandle,
    settings: &SettingsService,
) -> Result<(), AppError> {
    let Some(window) = app.get_webview_window("pet") else {
        return Ok(());
    };
    let Some(monitor) = window
        .current_monitor()
        .map_err(|error| window_error("read pet monitor", error))?
    else {
        return Ok(());
    };
    let outer_size = window
        .outer_size()
        .map_err(|error| window_error("read pet window size", error))?;
    let work_area = monitor.work_area();
    let Some(position) = restored_pet_position(
        settings,
        Size {
            width: i32::try_from(outer_size.width).unwrap_or(i32::MAX),
            height: i32::try_from(outer_size.height).unwrap_or(i32::MAX),
        },
        WorkArea {
            x: work_area.position.x,
            y: work_area.position.y,
            width: i32::try_from(work_area.size.width).unwrap_or(i32::MAX),
            height: i32::try_from(work_area.size.height).unwrap_or(i32::MAX),
        },
    )?
    else {
        return Ok(());
    };

    window
        .set_position(PhysicalPosition::new(position.x, position.y))
        .map_err(|error| window_error("restore pet window position", error))
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
}
