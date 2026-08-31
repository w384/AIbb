use std::{fs, path::PathBuf, sync::Arc};

use aibb_desktop_pet_lib::{
    app_state::AppState,
    domain::{BootstrapState, PetStatus, WebMode},
    error::AppError,
    platform::window_controller::{self, restored_pet_position, Position, Size, WorkArea},
    settings::{ApiSettings, CredentialStore, SaveSettings, SettingsService},
    storage::Database,
};
use async_trait::async_trait;
use tempfile::TempDir;
use tokio::{
    sync::{Mutex, Notify},
    time::{timeout, Duration},
};

#[derive(Clone, Default)]
struct FakeCredentialStore {
    value: Arc<Mutex<Option<String>>>,
}

#[async_trait]
impl CredentialStore for FakeCredentialStore {
    async fn get(&self) -> Result<Option<String>, AppError> {
        Ok(self.value.lock().await.clone())
    }

    async fn set(&self, api_key: &str) -> Result<(), AppError> {
        *self.value.lock().await = Some(api_key.to_owned());
        Ok(())
    }

    async fn clear(&self) -> Result<(), AppError> {
        *self.value.lock().await = None;
        Ok(())
    }
}

struct TestDatabase {
    _directory: TempDir,
    path: PathBuf,
    database: Database,
}

#[derive(Clone, Default)]
struct LeakyCredentialStore;

#[async_trait]
impl CredentialStore for LeakyCredentialStore {
    async fn get(&self) -> Result<Option<String>, AppError> {
        Ok(None)
    }

    async fn set(&self, _api_key: &str) -> Result<(), AppError> {
        Err(AppError {
            code: "credentialFailure".into(),
            message: "Authorization: Bearer sk-do-not-log\nkey=sk-do-not-log".into(),
        })
    }

    async fn clear(&self) -> Result<(), AppError> {
        Ok(())
    }
}

#[derive(Clone)]
struct FailingSetCredentialStore {
    value: Arc<Mutex<Option<String>>>,
}

impl FailingSetCredentialStore {
    fn with_existing_key(api_key: &str) -> Self {
        Self {
            value: Arc::new(Mutex::new(Some(api_key.to_owned()))),
        }
    }
}

#[async_trait]
impl CredentialStore for FailingSetCredentialStore {
    async fn get(&self) -> Result<Option<String>, AppError> {
        Ok(self.value.lock().await.clone())
    }

    async fn set(&self, api_key: &str) -> Result<(), AppError> {
        Err(AppError {
            code: format!("credential-set-failed-{api_key}"),
            message: format!("inline Authorization: Bearer {api_key}"),
        })
    }

    async fn clear(&self) -> Result<(), AppError> {
        *self.value.lock().await = None;
        Ok(())
    }
}

#[derive(Clone)]
struct PausingCredentialStore {
    value: Arc<Mutex<Option<String>>>,
    save_a_entered: Arc<Notify>,
    release_save_a: Arc<Notify>,
    save_b_entered: Arc<Notify>,
}

impl PausingCredentialStore {
    fn with_existing_key(api_key: &str) -> Self {
        Self {
            value: Arc::new(Mutex::new(Some(api_key.to_owned()))),
            save_a_entered: Arc::new(Notify::new()),
            release_save_a: Arc::new(Notify::new()),
            save_b_entered: Arc::new(Notify::new()),
        }
    }
}

#[async_trait]
impl CredentialStore for PausingCredentialStore {
    async fn get(&self) -> Result<Option<String>, AppError> {
        Ok(self.value.lock().await.clone())
    }

    async fn set(&self, api_key: &str) -> Result<(), AppError> {
        match api_key {
            "key-a" => {
                self.save_a_entered.notify_one();
                self.release_save_a.notified().await;
                Err(AppError {
                    code: "saveAFailed".into(),
                    message: "save A failed".into(),
                })
            }
            "key-b" => {
                self.save_b_entered.notify_one();
                *self.value.lock().await = Some(api_key.to_owned());
                Ok(())
            }
            _ => panic!("unexpected key in concurrency test"),
        }
    }

    async fn clear(&self) -> Result<(), AppError> {
        *self.value.lock().await = None;
        Ok(())
    }
}

impl TestDatabase {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("aibb-test.sqlite3");
        let database = Database::open(&path).unwrap();

        Self {
            _directory: directory,
            path,
            database,
        }
    }

    fn handle(&self) -> Database {
        self.database.clone()
    }

    fn raw_bytes(&self) -> Vec<u8> {
        fs::read(&self.path).unwrap()
    }

    fn reopen(&self) -> Database {
        Database::open(&self.path).unwrap()
    }

    fn path(&self) -> &std::path::Path {
        &self.path
    }
}

#[tokio::test]
async fn saves_key_outside_sqlite_and_never_returns_it() {
    let db = TestDatabase::new();
    let vault = FakeCredentialStore::default();
    let service = SettingsService::new(db.handle(), vault.clone());

    service
        .save(SaveSettings {
            api_base: "https://example.test/v1".into(),
            model: "model-a".into(),
            api_key: Some("sk-secret".into()),
            web_mode: WebMode::Auto,
            always_on_top: true,
            autostart: false,
        })
        .await
        .unwrap();

    let loaded = service.load().await.unwrap();
    assert!(loaded.api_configured);
    assert!(!serde_json::to_string(&loaded)
        .unwrap()
        .contains("sk-secret"));
    assert_eq!(vault.get().await.unwrap().as_deref(), Some("sk-secret"));
    assert!(!db
        .raw_bytes()
        .windows(b"sk-secret".len())
        .any(|window| window == b"sk-secret"));
}

#[tokio::test]
async fn omitted_key_preserves_the_credential_until_explicitly_cleared() {
    let db = TestDatabase::new();
    let vault = FakeCredentialStore::default();
    vault.set("existing-secret").await.unwrap();
    let service = SettingsService::new(db.handle(), vault.clone());

    service
        .save(SaveSettings {
            api_base: "https://example.test/v1".into(),
            model: "model-b".into(),
            api_key: None,
            web_mode: WebMode::Off,
            always_on_top: false,
            autostart: true,
        })
        .await
        .unwrap();

    assert_eq!(
        vault.get().await.unwrap().as_deref(),
        Some("existing-secret")
    );

    service.clear_api_key().await.unwrap();

    assert_eq!(vault.get().await.unwrap(), None);
    assert!(!service.load().await.unwrap().api_configured);
}

#[tokio::test]
async fn marks_first_run_complete_only_after_connection_verification() {
    let db = TestDatabase::new();
    let service = SettingsService::new(db.handle(), FakeCredentialStore::default());

    assert!(!db.handle().load_settings().unwrap().first_run_complete);

    service.mark_connection_verified().await.unwrap();

    assert!(db.handle().load_settings().unwrap().first_run_complete);
}

#[tokio::test]
async fn rebuilds_bootstrap_state_from_persisted_completion_and_protected_credential() {
    let db = TestDatabase::new();
    let vault = FakeCredentialStore::default();
    let service = SettingsService::new(db.handle(), vault.clone());

    assert_eq!(
        service.load_bootstrap_state().await.unwrap(),
        BootstrapState {
            first_run: true,
            pet_status: PetStatus::Idle,
            api_configured: false,
        }
    );

    vault.set("protected-after-restart").await.unwrap();
    service.mark_connection_verified().await.unwrap();
    let restarted = SettingsService::new(db.reopen(), vault);

    assert_eq!(
        restarted.load_bootstrap_state().await.unwrap(),
        BootstrapState {
            first_run: false,
            pet_status: PetStatus::Idle,
            api_configured: true,
        }
    );
}

#[test]
fn persists_and_clamps_the_pet_position_after_restart() {
    let db = TestDatabase::new();
    let service = SettingsService::new(db.handle(), FakeCredentialStore::default());
    let state = AppState::new(
        BootstrapState {
            first_run: true,
            pet_status: PetStatus::Idle,
            api_configured: false,
        },
        service,
    );

    window_controller::save_pet_position(&state, 1900, 1060).unwrap();

    let restarted = SettingsService::new(db.reopen(), FakeCredentialStore::default());
    let restored = restored_pet_position(
        &restarted,
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
    )
    .unwrap();

    assert_eq!(restored, Some(Position { x: 1700, y: 840 }));
}

#[tokio::test]
async fn persists_non_secret_settings_across_database_reopen() {
    let db = TestDatabase::new();
    let service = SettingsService::new(db.handle(), FakeCredentialStore::default());
    service
        .save(SaveSettings {
            api_base: "https://restart.example/v1".into(),
            model: "restart-model".into(),
            api_key: None,
            web_mode: WebMode::Force,
            always_on_top: false,
            autostart: true,
        })
        .await
        .unwrap();

    let restarted = SettingsService::new(db.reopen(), FakeCredentialStore::default());

    assert_eq!(
        restarted.load().await.unwrap(),
        ApiSettings {
            api_base: "https://restart.example/v1".into(),
            model: "restart-model".into(),
            web_mode: WebMode::Force,
            always_on_top: false,
            autostart: true,
            api_configured: false,
        }
    );
}

#[test]
fn initial_migration_creates_the_required_schema_without_an_api_key_column() {
    let db = TestDatabase::new();
    let connection = rusqlite::Connection::open(db.path()).unwrap();
    let mut table_statement = connection
        .prepare(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name IN \
             ('app_settings', 'messages', 'memory_summaries', 'explorations') ORDER BY name",
        )
        .unwrap();
    let tables = table_statement
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert_eq!(
        tables,
        vec![
            "app_settings",
            "explorations",
            "memory_summaries",
            "messages"
        ]
    );

    let mut column_statement = connection
        .prepare("PRAGMA table_info(app_settings)")
        .unwrap();
    let columns = column_statement
        .query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert_eq!(
        columns,
        vec![
            "singleton",
            "api_base",
            "model",
            "web_mode",
            "always_on_top",
            "autostart",
            "first_run_complete",
            "pet_x",
            "pet_y"
        ]
    );
}

#[tokio::test]
async fn sanitizes_credential_failures_before_returning_app_error() {
    let db = TestDatabase::new();
    let service = SettingsService::new(db.handle(), LeakyCredentialStore);

    let error = service
        .save(SaveSettings {
            api_base: "https://example.test/v1".into(),
            model: "model-a".into(),
            api_key: Some("sk-do-not-log".into()),
            web_mode: WebMode::Auto,
            always_on_top: true,
            autostart: false,
        })
        .await
        .unwrap_err();

    assert_eq!(error.code, "credentialStoreUnavailable");
    assert_eq!(
        error.message,
        "The protected API credential could not be stored."
    );
    assert!(!error.to_string().contains("sk-do-not-log"));
    assert!(!serde_json::to_string(&error)
        .unwrap()
        .contains("sk-do-not-log"));
}

#[tokio::test]
async fn restores_previous_non_secret_settings_when_credential_set_fails() {
    let db = TestDatabase::new();
    let vault = FailingSetCredentialStore::with_existing_key("old-protected-key");
    let service = SettingsService::new(db.handle(), vault.clone());
    service
        .save(SaveSettings {
            api_base: "https://old.example/v1".into(),
            model: "old-model".into(),
            api_key: None,
            web_mode: WebMode::Off,
            always_on_top: false,
            autostart: true,
        })
        .await
        .unwrap();

    let error = service
        .save(SaveSettings {
            api_base: "https://new.example/v1".into(),
            model: "new-model".into(),
            api_key: Some("new-protected-key".into()),
            web_mode: WebMode::Force,
            always_on_top: true,
            autostart: false,
        })
        .await
        .unwrap_err();

    assert_eq!(
        service.load().await.unwrap(),
        ApiSettings {
            api_base: "https://old.example/v1".into(),
            model: "old-model".into(),
            web_mode: WebMode::Off,
            always_on_top: false,
            autostart: true,
            api_configured: true,
        }
    );
    assert_eq!(
        vault.get().await.unwrap().as_deref(),
        Some("old-protected-key")
    );
    assert_eq!(error.code, "credentialStoreUnavailable");
    assert_eq!(
        error.message,
        "The protected API credential could not be stored."
    );
}

#[tokio::test]
async fn returns_fixed_public_error_when_settings_rollback_fails() {
    let db = TestDatabase::new();
    let vault = FailingSetCredentialStore::with_existing_key("old-protected-key");
    let service = SettingsService::new(db.handle(), vault);
    service
        .save(SaveSettings {
            api_base: "https://old.example/v1".into(),
            model: "old-model".into(),
            api_key: None,
            web_mode: WebMode::Auto,
            always_on_top: true,
            autostart: false,
        })
        .await
        .unwrap();
    rusqlite::Connection::open(db.path())
        .unwrap()
        .execute_batch(
            "CREATE TRIGGER reject_old_settings_before_update \
             BEFORE UPDATE ON app_settings \
             WHEN NEW.api_base = 'https://old.example/v1' \
             BEGIN SELECT RAISE(FAIL, 'Authorization: Bearer old-protected-key'); END;",
        )
        .unwrap();

    let error = service
        .save(SaveSettings {
            api_base: "https://new.example/v1".into(),
            model: "new-model".into(),
            api_key: Some("new-protected-key".into()),
            web_mode: WebMode::Force,
            always_on_top: false,
            autostart: true,
        })
        .await
        .unwrap_err();

    assert_eq!(error.code, "settingsRollbackFailed");
    assert_eq!(
        error.message,
        "Previous settings could not be restored after credential storage failed."
    );
    let serialized = serde_json::to_string(&error).unwrap();
    assert!(!serialized.contains("old-protected-key"));
    assert!(!serialized.contains("new-protected-key"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn serializes_concurrent_saves_across_database_and_credential_operations() {
    let db = TestDatabase::new();
    let vault = PausingCredentialStore::with_existing_key("old-key");
    let service = SettingsService::new(db.handle(), vault.clone());
    service
        .save(SaveSettings {
            api_base: "https://old.example/v1".into(),
            model: "old-model".into(),
            api_key: None,
            web_mode: WebMode::Auto,
            always_on_top: true,
            autostart: false,
        })
        .await
        .unwrap();

    let save_a_service = service.clone();
    let save_a = tokio::spawn(async move {
        save_a_service
            .save(SaveSettings {
                api_base: "https://a.example/v1".into(),
                model: "model-a".into(),
                api_key: Some("key-a".into()),
                web_mode: WebMode::Off,
                always_on_top: false,
                autostart: true,
            })
            .await
    });
    timeout(Duration::from_secs(1), vault.save_a_entered.notified())
        .await
        .expect("save A must reach the paused credential operation");

    let save_b_service = service.clone();
    let save_b = tokio::spawn(async move {
        save_b_service
            .save(SaveSettings {
                api_base: "https://b.example/v1".into(),
                model: "model-b".into(),
                api_key: Some("key-b".into()),
                web_mode: WebMode::Force,
                always_on_top: true,
                autostart: false,
            })
            .await
    });

    assert!(
        timeout(Duration::from_millis(150), vault.save_b_entered.notified())
            .await
            .is_err(),
        "save B entered the credential store before save A completed"
    );

    vault.release_save_a.notify_one();
    let save_a_error = timeout(Duration::from_secs(1), save_a)
        .await
        .expect("save A must finish after release")
        .unwrap()
        .unwrap_err();
    assert_eq!(save_a_error.code, "credentialStoreUnavailable");
    timeout(Duration::from_secs(1), save_b)
        .await
        .expect("save B must finish after save A rolls back")
        .unwrap()
        .unwrap();

    assert_eq!(
        service.load().await.unwrap(),
        ApiSettings {
            api_base: "https://b.example/v1".into(),
            model: "model-b".into(),
            web_mode: WebMode::Force,
            always_on_top: true,
            autostart: false,
            api_configured: true,
        }
    );
    assert_eq!(vault.get().await.unwrap().as_deref(), Some("key-b"));
}
