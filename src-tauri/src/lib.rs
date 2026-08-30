pub mod app_state;
pub mod domain;
pub mod error;

use std::sync::RwLock;

use app_state::AppState;
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState {
            bootstrap: RwLock::new(BootstrapState {
                first_run: true,
                pet_status: PetStatus::Idle,
                api_configured: false,
            }),
        })
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![get_bootstrap_state])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
