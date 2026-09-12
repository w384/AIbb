use tauri::{
    image::Image,
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle,
};

use crate::platform::window_controller;

/// Fallback tray icon: the app's 128×128 PNG embedded at compile time.
/// `default_window_icon()` is preferred when the runtime provides one.
const TRAY_ICON_PNG: &[u8] = include_bytes!("../../icons/128x128.png");

/// Install the system-tray icon with a small menu.
///
/// Left-click toggles the chat window; the menu offers "显示 AIbb" (open the
/// chat window) and "退出" (quit the whole app). The pet window is
/// `skipTaskbar`, so the tray is the persistent presence in the taskbar's
/// notification area.
pub fn install_tray(app: &AppHandle, profile_name: &str) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "显示 AIbb", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;

    let icon = app
        .default_window_icon()
        .cloned()
        .or_else(|| Image::from_bytes(TRAY_ICON_PNG).ok());

    let menu_name = profile_name.to_string();
    let click_name = profile_name.to_string();

    let mut builder = TrayIconBuilder::with_id("aibb-tray")
        .menu(&menu)
        .tooltip(format!("{profile_name} — AIbb"))
        .on_menu_event(move |app, event| match event.id.as_ref() {
            "show" => {
                let _ = window_controller::open_chat(app, &menu_name);
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(move |tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let _ = window_controller::toggle_chat(tray.app_handle(), &click_name);
            }
        });

    if let Some(icon) = icon {
        builder = builder.icon(icon);
    }

    builder.build(app)?;
    Ok(())
}
