fn main() {
    const COMMANDS: &[&str] = &[
        "get_bootstrap_state",
        "toggle_chat_window",
        "open_settings_window",
        "exit_app",
        "pet_drag_begin",
        "pet_drag_move",
        "pet_drag_end",
        "save_pet_position",
        "load_settings",
        "save_settings",
        "clear_api_key",
        "test_connection",
        "clear_memory",
        "submit_user_input",
        "load_aibb_profile",
        "save_aibb_name",
        "save_aibb_avatar",
        "reset_aibb_avatar",
    ];

    let attributes = tauri_build::Attributes::new()
        .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS));
    tauri_build::try_build(attributes).expect("failed to run Tauri build script");
}
