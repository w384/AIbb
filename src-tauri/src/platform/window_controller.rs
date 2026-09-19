use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
};

use crate::{
    app_state::AppState, error::AppError, platform::avatar_icons, settings::SettingsService,
};

/// Fired on the chat window every time the pet click shows it, so the window
/// lands on the mode chooser (普通对话 / 词汇助手) rather than the last mode.
pub const CHAT_OPENED_EVENT: &str = "chat://opened";

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
        "chat" => format!("{profile_name}AIbb"),
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
        // The chooser screen resets on every pet click so the user always has
        // the two options in front of them (普通对话 / 词汇助手).
        let _ = app.emit_to("chat", CHAT_OPENED_EVENT, ());
        window
            .set_focus()
            .map_err(|error| window_error("focus chat window", error))
    }
}

/// Show the chat window (creating it on first use), used both by the pet
/// click toggle and by the file-drop archive flow.
pub fn open_chat(app: &AppHandle, profile_name: &str) -> Result<(), AppError> {
    let title = profile_window_title("chat", profile_name);
    let window = get_or_create_window(app, "chat", &title, 420.0, 620.0)?;
    window
        .show()
        .map_err(|error| window_error("show chat window", error))?;
    window
        .set_focus()
        .map_err(|error| window_error("focus chat window", error))
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

/// The always-visible floating pet window, if it exists yet.
pub fn pet_window(app: &AppHandle) -> Result<WebviewWindow, AppError> {
    app.get_webview_window("pet")
        .ok_or_else(|| window_error("find pet window", tauri::Error::WindowNotFound))
}

/// After a drag ends, snap the pet so its round avatar (which is centered in
/// the window) hugs the nearest edge of its current monitor's work area with
/// a tiny gap, and always clamp the window so it never ends up fully off the
/// display.
pub fn snap_pet_to_work_area_edge(window: &WebviewWindow) -> Result<(), AppError> {
    let position = window
        .outer_position()
        .map_err(|error| window_error("read pet drag position", error))?;
    let size = window
        .outer_size()
        .map_err(|error| window_error("read pet drag size", error))?;
    let Some(monitor) = window
        .current_monitor()
        .map_err(|error| window_error("read pet monitor", error))?
    else {
        return Ok(());
    };
    let work_area = monitor.work_area();
    let before = Position {
        x: position.x,
        y: position.y,
    };
    let after = snap_avatar_position(
        before,
        Size {
            width: i32::try_from(size.width).unwrap_or(44),
            height: i32::try_from(size.height).unwrap_or(44),
        },
        WorkArea {
            x: work_area.position.x,
            y: work_area.position.y,
            width: i32::try_from(work_area.size.width).unwrap_or(i32::MAX),
            height: i32::try_from(work_area.size.height).unwrap_or(i32::MAX),
        },
    );
    if after != before {
        window
            .set_position(PhysicalPosition::new(after.x, after.y))
            .map_err(|error| window_error("snap pet window position", error))?;
    }
    Ok(())
}

/// Snaps the pet so its round avatar hugs the chosen work-area edge with a
/// tiny gap, and otherwise clamps the window fully on screen. The avatar is
/// drawn centered inside the window (a 40px disc in a 44px window, so its
/// radius is 20/44 of the window's smaller side). Snapping the avatar instead
/// of the window edge means the result looks right even if the transparent
/// window is wider than the avatar: the transparent overhang may sit
/// off-screen, but the round pet itself lands flush against the edge.
pub fn snap_avatar_position(position: Position, size: Size, work_area: WorkArea) -> Position {
    const SNAP_MARGIN: i32 = 16;

    let min_side = size.width.min(size.height).max(1);
    let avatar_radius = ((min_side as f64) * 40.0 / 44.0 / 2.0).round() as i32;
    let edge_gap = ((min_side as f64) * 2.0 / 44.0).round() as i32;

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

    let x = if position.x <= work_area.x + SNAP_MARGIN {
        work_area.x + avatar_radius + edge_gap - size.width / 2
    } else if position.x >= max_x - SNAP_MARGIN {
        work_area.x + work_area.width - avatar_radius - edge_gap - size.width / 2
    } else {
        position.x.clamp(work_area.x, max_x)
    };
    let y = if position.y <= work_area.y + SNAP_MARGIN {
        work_area.y + avatar_radius + edge_gap - size.height / 2
    } else if position.y >= max_y - SNAP_MARGIN {
        work_area.y + work_area.height - avatar_radius - edge_gap - size.height / 2
    } else {
        position.y.clamp(work_area.y, max_y)
    };
    Position { x, y }
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

    let window = WebviewWindowBuilder::new(
        app,
        label,
        WebviewUrl::App(format!("index.html?window={label}").into()),
    )
    .title(title)
    .inner_size(width, height)
    .visible(false)
    .build()
    .map_err(|error| window_error("create application window", error))?;
    apply_window_avatar(app);
    Ok(window)
}

/// Best-effort: point a freshly created window's title-bar / taskbar icon at
/// the stored avatar, so the chat and settings windows (created lazily on
/// first use) match the uploaded picture.
fn apply_window_avatar(app: &AppHandle) {
    let Ok(directory) = app.path().app_data_dir() else {
        return;
    };
    let avatar = directory.join("aibb-profile").join("avatar.webp");
    let bytes = std::fs::read(avatar).ok();
    avatar_icons::apply_window_icons(app, bytes.as_deref());
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
        assert_eq!(profile_window_title("chat", "小团子"), "小团子AIbb");
        assert_eq!(
            profile_window_title("settings", "小团子"),
            "小团子 Settings"
        );
    }

    #[test]
    fn snaps_the_avatar_to_the_edge_when_the_drag_ends_close_to_it() {
        let area = WorkArea {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        };
        let size = Size {
            width: 44,
            height: 44,
        };
        // 44px window: the avatar (radius 20) centered in it lands 2px off
        // the edge, so the window edge itself stays flush on screen.
        assert_eq!(
            snap_avatar_position(Position { x: 12, y: 400 }, size, area),
            Position { x: 0, y: 400 }
        );
        assert_eq!(
            snap_avatar_position(Position { x: 400, y: 1040 }, size, area),
            Position { x: 400, y: 1036 }
        );
        assert_eq!(
            snap_avatar_position(Position { x: 1900, y: 500 }, size, area),
            Position { x: 1876, y: 500 }
        );
        assert_eq!(
            snap_avatar_position(Position { x: 8, y: 8 }, size, area),
            Position { x: 0, y: 0 }
        );
    }

    #[test]
    fn a_wide_window_snaps_its_centered_avatar_flush_to_the_edge() {
        let area = WorkArea {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        };
        // A window wider than the avatar (e.g. a platform-forced minimum
        // size): the avatar still lands 2px from the edge; the transparent
        // overhang simply sits off-screen.
        let wide = Size {
            width: 120,
            height: 44,
        };
        assert_eq!(
            snap_avatar_position(Position { x: 1850, y: 500 }, wide, area),
            Position { x: 1838, y: 500 }
        );
        // Left edge: the avatar lands 2px from the edge and the transparent
        // overhang sticks out off-screen.
        assert_eq!(
            snap_avatar_position(Position { x: 10, y: 500 }, wide, area),
            Position { x: -38, y: 500 }
        );
    }

    #[test]
    fn keeps_a_free_position_and_clamps_when_it_would_leave_the_screen() {
        let area = WorkArea {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        };
        let size = Size {
            width: 44,
            height: 44,
        };
        assert_eq!(
            snap_avatar_position(Position { x: 600, y: 400 }, size, area),
            Position { x: 600, y: 400 }
        );
        assert_eq!(
            snap_avatar_position(Position { x: 2100, y: -60 }, size, area),
            Position { x: 1876, y: 0 }
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
