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
pub mod web;

use app_state::AppState;
use commands::chat::{build_chat_service, submit_user_input};
use commands::exploration::build_exploration_orchestrator;
use commands::memory::clear_memory;
use commands::profile::{load_aibb_profile, reset_aibb_avatar, save_aibb_avatar, save_aibb_name};
use commands::settings::{clear_api_key, load_settings, save_settings, test_connection};
use commands::window::{
    exit_app, open_settings_window, save_pet_position, start_pet_drag, toggle_chat_window,
};
use domain::BootstrapState;
use error::AppError;
use memory::MemoryRepository;
use settings::{NativeCredentialStore, SettingsService};
use storage::Database;

#[tauri::command]
async fn get_bootstrap_state(
    state: tauri::State<'_, AppState>,
) -> Result<BootstrapState, AppError> {
    current_bootstrap_state(&state).await
}

pub async fn current_bootstrap_state(state: &AppState) -> Result<BootstrapState, AppError> {
    state.settings.load_bootstrap_state().await
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default();
    #[cfg(desktop)]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
        use tauri::Manager;

        if let Some(pet) = app.get_webview_window("pet") {
            let _ = pet.set_focus();
        }
    }));

    builder
        .setup(|app| {
            use tauri::Manager;

            let app_data_dir = app.path().app_data_dir()?;
            let database_path = app_data_dir.join("aibb.sqlite3");
            let database = Database::open(database_path)?;
            let settings = SettingsService::new_with_app_data_dir(
                database.clone(),
                NativeCredentialStore,
                app_data_dir,
            );
            let memory = MemoryRepository::new(database.clone());
            let bootstrap = tauri::async_runtime::block_on(settings.load_bootstrap_state())?;
            let profile = tauri::async_runtime::block_on(settings.load_aibb_profile())?;
            let exploration = build_exploration_orchestrator(
                app.handle().clone(),
                database,
                memory.clone(),
                settings.clone(),
            );
            let chat = build_chat_service(app.handle().clone(), memory.clone(), settings.clone());
            tauri::async_runtime::block_on(exploration.recover_interrupted())?;
            app.manage(AppState::with_services(
                bootstrap,
                settings.clone(),
                memory,
                chat,
                exploration,
            ));
            platform::window_controller::restore_pet_window_position(app.handle(), &settings)?;
            commands::profile::publish_profile_updated(app.handle(), &profile);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_bootstrap_state,
            toggle_chat_window,
            open_settings_window,
            exit_app,
            start_pet_drag,
            save_pet_position,
            load_settings,
            save_settings,
            clear_api_key,
            test_connection,
            load_aibb_profile,
            save_aibb_name,
            save_aibb_avatar,
            reset_aibb_avatar,
            clear_memory,
            submit_user_input
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use crate::{
        commands::profile::{
            dispatch_profile_updates, profile_window_updates, PROFILE_UPDATED_EVENT,
        },
        domain::AibbProfile,
        exploration::{ExplorationEvent, ExplorationStatus},
    };
    use uuid::Uuid;

    #[test]
    fn single_instance_plugin_precedes_setup_and_interrupted_task_recovery() {
        let source = include_str!("lib.rs");
        let plugin = source
            .find(".plugin(tauri_plugin_single_instance::init")
            .expect("single-instance plugin must be registered on the builder");
        let setup = source
            .find(".setup(|app|")
            .expect("setup must be registered");
        let recovery = source
            .find("exploration.recover_interrupted()")
            .expect("interrupted exploration recovery must remain wired");

        assert!(
            plugin < setup,
            "single-instance must be registered before setup"
        );
        assert!(
            plugin < recovery,
            "single-instance must be registered before interrupted-task recovery"
        );
    }

    #[test]
    fn pet_capability_grants_only_its_required_renderer_permissions() {
        let capability: serde_json::Value =
            serde_json::from_str(include_str!("../capabilities/default.json")).unwrap();

        assert_eq!(capability["windows"], serde_json::json!(["pet"]));
        assert_eq!(
            capability["permissions"],
            serde_json::json!([
                "core:event:allow-listen",
                "core:event:allow-unlisten",
                "core:window:allow-outer-position",
                "allow-load-aibb-profile",
                "allow-toggle-chat-window",
                "allow-open-settings-window",
                "allow-start-pet-drag",
                "allow-save-pet-position"
            ])
        );
    }

    #[test]
    fn pet_window_is_compact_transparent_and_undecorated() {
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let pet = &config["app"]["windows"][0];

        assert_eq!(pet["label"], serde_json::json!("pet"));
        assert_eq!(pet["width"], serde_json::json!(48));
        assert_eq!(pet["height"], serde_json::json!(48));
        assert_eq!(pet["transparent"], serde_json::json!(true));
        assert_eq!(pet["decorations"], serde_json::json!(false));
        assert_eq!(pet["shadow"], serde_json::json!(false));
    }

    #[test]
    fn webviews_enforce_a_local_only_content_security_policy() {
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let csp = config["app"]["security"]["csp"]
            .as_str()
            .expect("production webviews must configure CSP");

        assert!(csp.contains("default-src 'self'"));
        assert!(csp.contains("connect-src ipc: http://ipc.localhost"));
        assert!(csp.contains("img-src 'self' data:"));
        assert!(!csp.contains("https:"));
        assert!(!csp.contains("http: "));
    }

    #[test]
    fn chat_capability_grants_only_chat_events_and_required_commands() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("capabilities")
            .join("chat.json");
        assert!(path.exists(), "chat capability must exist");
        let capability: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();

        assert_eq!(capability["windows"], serde_json::json!(["chat"]));
        assert_eq!(
            capability["permissions"],
            serde_json::json!([
                "core:event:allow-listen",
                "core:event:allow-unlisten",
                "allow-get-bootstrap-state",
                "allow-load-aibb-profile",
                "allow-open-settings-window",
                "allow-submit-user-input"
            ])
        );
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
                "core:event:allow-listen",
                "core:event:allow-unlisten",
                "allow-load-settings",
                "allow-save-settings",
                "allow-test-connection",
                "allow-clear-memory",
                "allow-exit-app",
                "allow-load-aibb-profile",
                "allow-save-aibb-name",
                "allow-save-aibb-avatar",
                "allow-reset-aibb-avatar"
            ])
        );
    }

    #[test]
    fn profile_update_is_scoped_to_profile_windows_and_contains_only_profile() {
        let profile = AibbProfile {
            name: "小团子".into(),
            avatar_data_url: Some("data:image/webp;base64,UklGRg==".into()),
            version: 7,
        };

        let updates = profile_window_updates(&profile);

        assert_eq!(PROFILE_UPDATED_EVENT, "profile://updated");
        assert_eq!(
            updates
                .iter()
                .map(|update| (update.label, update.title.as_str()))
                .collect::<Vec<_>>(),
            vec![
                ("pet", "小团子"),
                ("chat", "小团子 Chat"),
                ("settings", "小团子 Settings")
            ]
        );
        for update in updates {
            let payload = serde_json::to_value(update.payload).unwrap();
            assert_eq!(
                payload,
                serde_json::json!({
                    "name": "小团子",
                    "avatarDataUrl": "data:image/webp;base64,UklGRg==",
                    "version": 7
                })
            );
        }
    }

    #[test]
    fn profile_ui_propagation_failures_are_best_effort_after_persistence() {
        let profile = AibbProfile {
            name: "小团子".into(),
            avatar_data_url: None,
            version: 8,
        };
        let mut attempted = Vec::new();

        dispatch_profile_updates(&profile, |update| {
            attempted.push(update.label);
            Err::<(), ()>(())
        });

        assert_eq!(attempted, vec!["pet", "chat", "settings"]);
    }

    #[test]
    fn lower_level_chat_and_unrelated_window_commands_are_never_granted() {
        let capability_directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("capabilities");
        let forbidden = [
            "allow-start-chat",
            "allow-start-exploration",
            "allow-cancel-exploration",
            "allow-load-settings",
            "allow-save-settings",
            "allow-start-pet-drag",
            "allow-save-pet-position",
        ];
        let chat: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(capability_directory.join("chat.json")).unwrap(),
        )
        .unwrap();
        let permissions = chat["permissions"].as_array().unwrap();
        for permission in forbidden {
            assert!(
                !permissions.iter().any(|value| value == permission),
                "chat must not receive {permission}"
            );
        }
    }

    #[test]
    fn exploration_events_serialize_task_id_for_renderer_filters() {
        let task_id = Uuid::nil();
        let serialized = serde_json::to_value(ExplorationEvent::Progress {
            task_id,
            status: ExplorationStatus::Writing,
        })
        .unwrap();

        assert_eq!(serialized["taskId"], serde_json::json!(task_id));
        assert!(serialized.get("task_id").is_none());
    }
}
