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
use tokio::sync::Mutex;

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
        WorkArea {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        },
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

    assert!(!error.to_string().contains("sk-do-not-log"));
    assert!(!serde_json::to_string(&error)
        .unwrap()
        .contains("sk-do-not-log"));
}
