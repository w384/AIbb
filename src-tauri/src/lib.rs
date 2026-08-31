pub mod app_state;
pub mod commands;
pub mod domain;
pub mod error;
pub mod exploration;
pub mod llm;
pub mod memory;
pub mod platform;
pub mod prompts;
pub mod settings;
pub mod storage;

use app_state::AppState;
use commands::memory::clear_memory;
use commands::settings::{clear_api_key, load_settings, save_settings, test_connection};
use commands::window::{
    open_settings_window, save_pet_position, start_pet_drag, toggle_chat_window,
};
use domain::BootstrapState;
use error::AppError;
use memory::MemoryRepository;
use settings::{NativeCredentialStore, SettingsService};
use storage::Database;

#[tauri::command]
fn get_bootstrap_state(state: tauri::State<'_, AppState>) -> Result<BootstrapState, AppError> {
    let bootstrap = state
        .bootstrap
        .read()
        .map_err(|_| AppError::new("stateUnavailable", "Application state is unavailable."))?;

    Ok(bootstrap.clone())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            use tauri::Manager;

            let database_path = app.path().app_data_dir()?.join("aibb.sqlite3");
            let database = Database::open(database_path)?;
            let settings = SettingsService::new(database.clone(), NativeCredentialStore);
            let memory = MemoryRepository::new(database);
            let bootstrap = tauri::async_runtime::block_on(settings.load_bootstrap_state())?;
            app.manage(AppState::new(bootstrap, settings.clone(), memory));
            platform::window_controller::restore_pet_window_position(app.handle(), &settings)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_bootstrap_state,
            toggle_chat_window,
            open_settings_window,
            start_pet_drag,
            save_pet_position,
            load_settings,
            save_settings,
            clear_api_key,
            test_connection,
            clear_memory
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
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
    fn chat_receives_no_capabilities() {
        let capability_directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("capabilities");

        for entry in fs::read_dir(capability_directory).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
                continue;
            }
            let capability: serde_json::Value =
                serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
            let windows = capability["windows"].as_array().unwrap();

            assert!(
                !windows.iter().any(|window| window == "chat"),
                "chat must not receive a capability"
            );
        }
    }

    #[test]
    fn settings_capability_grants_only_sanitized_settings_commands() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("capabilities")
            .join("settings.json");
        assert!(path.exists(), "settings capability must exist");
        let capability: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();

        assert_eq!(capability["windows"], serde_json::json!(["settings"]));
        assert_eq!(
            capability["permissions"],
            serde_json::json!([
                "allow-load-settings",
                "allow-save-settings",
                "allow-clear-api-key",
                "allow-test-connection"
            ])
        );
    }
}
