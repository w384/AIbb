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

pub fn profile_window_title(label: &str, profile_name: &str) -> String {
    match label {
        "chat" => format!("{profile_name} Chat"),
        "settings" => format!("{profile_name} Settings"),
        _ => profile_name.to_string(),
    }
}

pub fn toggle_chat(app: &AppHandle, profile_name: &str) -> Result<(), AppError> {
    let title = profile_window_title("chat", profile_name);
    let window = get_or_create_window(app, "chat", &title, 420.0, 620.0)?;
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

pub fn open_settings(app: &AppHandle, profile_name: &str) -> Result<(), AppError> {
    let title = profile_window_title("settings", profile_name);
    let window = get_or_create_window(app, "settings", &title, 520.0, 640.0)?;
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
    work_areas: &[WorkArea],
) -> Result<Option<Position>, AppError> {
    Ok(settings
        .pet_position()?
        .map(|(x, y)| clamp_position_to_work_areas(Position { x, y }, size, work_areas)))
}

pub fn restore_pet_window_position(
    app: &AppHandle,
    settings: &SettingsService,
) -> Result<(), AppError> {
    let Some(window) = app.get_webview_window("pet") else {
        return Ok(());
    };
    let outer_size = window
        .outer_size()
        .map_err(|error| window_error("read pet window size", error))?;
    let work_areas = window
        .available_monitors()
        .map_err(|error| window_error("enumerate monitor work areas", error))?
        .into_iter()
        .map(|monitor| {
            let work_area = monitor.work_area();
            WorkArea {
                x: work_area.position.x,
                y: work_area.position.y,
                width: i32::try_from(work_area.size.width).unwrap_or(i32::MAX),
                height: i32::try_from(work_area.size.height).unwrap_or(i32::MAX),
            }
        })
        .collect::<Vec<_>>();
    let Some(position) = restored_pet_position(
        settings,
        Size {
            width: i32::try_from(outer_size.width).unwrap_or(i32::MAX),
            height: i32::try_from(outer_size.height).unwrap_or(i32::MAX),
        },
        &work_areas,
    )?
    else {
        return Ok(());
    };

    window
        .set_position(PhysicalPosition::new(position.x, position.y))
        .map_err(|error| window_error("restore pet window position", error))
}

pub fn clamp_position_to_work_areas(
    position: Position,
    size: Size,
    work_areas: &[WorkArea],
) -> Position {
    let center_x = i64::from(position.x) + i64::from(size.width) / 2;
    let center_y = i64::from(position.y) + i64::from(size.height) / 2;
    let selected = work_areas
        .iter()
        .find(|work_area| contains_point(work_area, center_x, center_y))
        .or_else(|| {
            work_areas
                .iter()
                .min_by_key(|work_area| distance_squared(work_area, center_x, center_y))
        });

    selected
        .map(|work_area| clamp_position(position, size, *work_area))
        .unwrap_or(position)
}

fn contains_point(work_area: &WorkArea, x: i64, y: i64) -> bool {
    let left = i64::from(work_area.x);
    let top = i64::from(work_area.y);
    let right = left + i64::from(work_area.width.max(0));
    let bottom = top + i64::from(work_area.height.max(0));
    x >= left && x < right && y >= top && y < bottom
}

fn distance_squared(work_area: &WorkArea, x: i64, y: i64) -> i128 {
    let left = i64::from(work_area.x);
    let top = i64::from(work_area.y);
    let right = left + i64::from(work_area.width.max(0));
    let bottom = top + i64::from(work_area.height.max(0));
    let nearest_x = x.clamp(left, right);
    let nearest_y = y.clamp(top, bottom);
    let dx = i128::from(x - nearest_x);
    let dy = i128::from(y - nearest_y);
    dx * dx + dy * dy
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
    AppError::new(
        "windowOperationFailed",
        format!("Failed to {action}: {error}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_window_titles_use_the_saved_profile_name() {
        assert_eq!(profile_window_title("pet", "小团子"), "小团子");
        assert_eq!(profile_window_title("chat", "小团子"), "小团子 Chat");
        assert_eq!(
            profile_window_title("settings", "小团子"),
            "小团子 Settings"
        );
    }

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
    fn keeps_a_saved_position_on_a_negative_coordinate_secondary_monitor() {
        let restored = clamp_position_to_work_areas(
            Position { x: -1700, y: 120 },
            Size {
                width: 220,
                height: 240,
            },
            &[
                WorkArea {
                    x: 0,
                    y: 0,
                    width: 1920,
                    height: 1080,
                },
                WorkArea {
                    x: -1920,
                    y: 0,
                    width: 1920,
                    height: 1080,
                },
            ],
        );

        assert_eq!(restored, Position { x: -1700, y: 120 });
    }

    #[test]
    fn keeps_a_saved_position_on_a_right_side_secondary_monitor() {
        let restored = clamp_position_to_work_areas(
            Position { x: 2100, y: 120 },
            Size {
                width: 220,
                height: 240,
            },
            &[
                WorkArea {
                    x: 0,
                    y: 0,
                    width: 1920,
                    height: 1080,
                },
                WorkArea {
                    x: 1920,
                    y: 0,
                    width: 1920,
                    height: 1080,
                },
            ],
        );

        assert_eq!(restored, Position { x: 2100, y: 120 });
    }

    #[test]
    fn clamps_to_the_nearest_remaining_monitor_when_the_original_was_removed() {
        let restored = clamp_position_to_work_areas(
            Position { x: 2100, y: 120 },
            Size {
                width: 220,
                height: 240,
            },
            &[WorkArea {
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
            }],
        );

        assert_eq!(restored, Position { x: 1700, y: 120 });
    }

    #[test]
    fn chooses_the_monitor_that_contains_the_saved_window_center() {
        let restored = clamp_position_to_work_areas(
            Position { x: -100, y: 120 },
            Size {
                width: 300,
                height: 240,
            },
            &[
                WorkArea {
                    x: -1920,
                    y: 0,
                    width: 1920,
                    height: 1080,
                },
                WorkArea {
                    x: 0,
                    y: 0,
                    width: 1920,
                    height: 1080,
                },
            ],
        );

        assert_eq!(restored, Position { x: 0, y: 120 });
    }
}
