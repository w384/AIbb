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
    use std::{fs, path::Path};

    #[test]
    fn pet_capability_grants_only_its_required_renderer_permissions() {
        let capability: serde_json::Value =
            serde_json::from_str(include_str!("../capabilities/default.json")).unwrap();

        assert_eq!(capability["windows"], serde_json::json!(["pet"]));
        assert_eq!(
            capability["permissions"],
            serde_json::json!([
                "core:window:allow-outer-position",
                "allow-toggle-chat-window",
                "allow-open-settings-window",
                "allow-start-pet-drag",
                "allow-save-pet-position"
            ])
        );
    }

    #[test]
    fn chat_and_settings_receive_no_capabilities() {
        let capability_directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("capabilities");
        let restricted_windows = ["chat", "settings"];

        for entry in fs::read_dir(capability_directory).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
                continue;
            }
            let capability: serde_json::Value =
                serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
            let windows = capability["windows"].as_array().unwrap();

            for restricted_window in restricted_windows {
                assert!(
                    !windows.iter().any(|window| window == restricted_window),
                    "{restricted_window} must not receive a capability in Task 2"
                );
            }
        }
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
