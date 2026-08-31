fn main() {
    const COMMANDS: &[&str] = &[
        "toggle_chat_window",
        "open_settings_window",
        "start_pet_drag",
        "save_pet_position",
        "test_connection",
    ];

    let attributes = tauri_build::Attributes::new()
        .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS));
    tauri_build::try_build(attributes).expect("failed to run Tauri build script");
}
