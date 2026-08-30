pub mod app_state;
pub mod commands;
pub mod domain;
pub mod error;
pub mod platform;

use std::sync::RwLock;

use app_state::AppState;
use commands::window::{
    open_settings_window, save_pet_position, start_pet_drag, toggle_chat_window,
};
use domain::{BootstrapState, PetStatus};
use error::AppError;

#[tauri::command]
fn get_bootstrap_state(state: tauri::State<'_, AppState>) -> Result<BootstrapState, AppError> {
    let bootstrap = state.bootstrap.read().map_err(|_| AppError {
        code: "stateUnavailable".to_string(),
        message: "Application state is unavailable.".to_string(),
    })?;

    Ok(bootstrap.clone())
}

#[cfg(test)]
mod tests {
    #[test]
    fn default_capability_grants_only_required_window_and_command_permissions() {
        let capability: serde_json::Value =
            serde_json::from_str(include_str!("../capabilities/default.json")).unwrap();

        assert_eq!(
            capability["windows"],
            serde_json::json!(["pet", "chat", "settings"])
        );
        assert_eq!(
            capability["permissions"],
            serde_json::json!([
                "core:window:allow-start-dragging",
                "core:window:allow-outer-position",
                "core:window:allow-set-position",
                "core:window:allow-show",
                "core:window:allow-hide",
                "allow-toggle-chat-window",
                "allow-open-settings-window",
                "allow-start-pet-drag",
                "allow-save-pet-position"
            ])
        );
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState {
            bootstrap: RwLock::new(BootstrapState {
                first_run: true,
                pet_status: PetStatus::Idle,
                api_configured: false,
            }),
            pet_position: RwLock::new(None),
        })
        .invoke_handler(tauri::generate_handler![
            get_bootstrap_state,
            toggle_chat_window,
            open_settings_window,
            start_pet_drag,
            save_pet_position
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
