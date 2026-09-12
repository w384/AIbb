use std::{fs, io::Cursor, path::PathBuf, sync::Arc};

use aibb_desktop_pet_lib::{
    app_state::AppState,
    archive::service::ArchiveService,
    current_bootstrap_state,
    domain::{BootstrapState, PetStatus, WebMode},
    error::AppError,
    memory::MemoryRepository,
    platform::window_controller::{self, restored_pet_position, Position, Size, WorkArea},
    settings::{AibbProfile, ApiSettings, CredentialStore, SaveSettings, SettingsService},
    storage::Database,
};
use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use image::{DynamicImage, GenericImageView, ImageBuffer, ImageFormat, Rgb, Rgba};
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
        Err(AppError::new(
            "credentialFailure",
            "Authorization: Bearer sk-do-not-log\nkey=sk-do-not-log",
        ))
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
        Err(AppError::new(
            format!("credential-set-failed-{api_key}"),
            format!("inline Authorization: Bearer {api_key}"),
        ))
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
                Err(AppError::new("saveAFailed", "save A failed"))
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

    fn app_data_dir(&self) -> &std::path::Path {
        self._directory.path()
    }

    fn persisted_profile(&self) -> (String, Option<String>, i64) {
        rusqlite::Connection::open(self.path())
            .unwrap()
            .query_row(
                "SELECT aibb_name, avatar_filename, profile_version \
                 FROM app_settings WHERE singleton = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap()
    }

    fn reject_avatar_metadata_updates(&self) {
        rusqlite::Connection::open(self.path())
            .unwrap()
            .execute_batch(
                "CREATE TRIGGER reject_avatar_metadata_update \
                 BEFORE UPDATE OF avatar_filename ON app_settings \
                 BEGIN SELECT RAISE(FAIL, 'forced avatar metadata failure'); END;",
            )
            .unwrap();
    }
}

fn valid_webp_bytes() -> Vec<u8> {
    encoded_test_image(ImageFormat::WebP)
}

fn encoded_test_image(format: ImageFormat) -> Vec<u8> {
    let image = match format {
        ImageFormat::Jpeg => {
            DynamicImage::ImageRgb8(ImageBuffer::from_pixel(2, 2, Rgb([32_u8, 96, 192])))
        }
        _ => DynamicImage::ImageRgba8(ImageBuffer::from_pixel(2, 2, Rgba([32_u8, 96, 192, 255]))),
    };
    let mut bytes = Cursor::new(Vec::new());
    image.write_to(&mut bytes, format).unwrap();
    bytes.into_inner()
}

fn avatar_webp_bytes(profile: &AibbProfile) -> Vec<u8> {
    let data_url = profile.avatar_data_url.as_deref().unwrap();
    let encoded = data_url.strip_prefix("data:image/webp;base64,").unwrap();
    BASE64_STANDARD.decode(encoded).unwrap()
}

#[tokio::test]
async fn profile_rejects_fake_image_bytes_with_an_allowed_mime() {
    let db = TestDatabase::new();
    let service = SettingsService::new_with_app_data_dir(
        db.handle(),
        FakeCredentialStore::default(),
        db.app_data_dir(),
    );

    let error = service
        .save_aibb_avatar(b"not actually a png".to_vec(), "image/png".into())
        .await
        .unwrap_err();

    assert_eq!(error.code, "invalidProfile");
    assert_eq!(db.persisted_profile(), ("AIbb".into(), None, 0));
    assert!(!db.app_data_dir().join("aibb-profile/avatar.webp").exists());
}

#[tokio::test]
async fn profile_png_and_jpeg_inputs_are_returned_as_decodable_webp() {
    for (mime_type, format) in [
        ("image/png", ImageFormat::Png),
        ("image/jpeg", ImageFormat::Jpeg),
    ] {
        let db = TestDatabase::new();
        let service = SettingsService::new_with_app_data_dir(
            db.handle(),
            FakeCredentialStore::default(),
            db.app_data_dir(),
        );

        let profile = service
            .save_aibb_avatar(encoded_test_image(format), mime_type.into())
            .await
            .unwrap();
        let webp = avatar_webp_bytes(&profile);
        let decoded = image::load_from_memory_with_format(&webp, ImageFormat::WebP).unwrap();

        assert_eq!(decoded.dimensions(), (256, 256));
        assert_eq!(
            fs::read(db.app_data_dir().join("aibb-profile/avatar.webp")).unwrap(),
            webp
        );
    }
}

#[tokio::test]
async fn profile_load_avatar_bytes_follows_save_and_reset() {
    let db = TestDatabase::new();
    let service = SettingsService::new_with_app_data_dir(
        db.handle(),
        FakeCredentialStore::default(),
        db.app_data_dir(),
    );

    assert_eq!(service.load_avatar_bytes().await.unwrap(), None);

    let profile = service
        .save_aibb_avatar(encoded_test_image(ImageFormat::Png), "image/png".into())
        .await
        .unwrap();
    let bytes = service.load_avatar_bytes().await.unwrap().unwrap();
    assert_eq!(bytes, avatar_webp_bytes(&profile));

    service.reset_aibb_avatar().await.unwrap();
    assert_eq!(service.load_avatar_bytes().await.unwrap(), None);
}

#[tokio::test]
async fn profile_avatar_database_failure_restores_the_previous_file_and_profile() {
    let db = TestDatabase::new();
    let service = SettingsService::new_with_app_data_dir(
        db.handle(),
        FakeCredentialStore::default(),
        db.app_data_dir(),
    );
    let previous = service
        .save_aibb_avatar(valid_webp_bytes(), "image/webp".into())
        .await
        .unwrap();
    let previous_file = fs::read(db.app_data_dir().join("aibb-profile/avatar.webp")).unwrap();
    let previous_row = db.persisted_profile();
    db.reject_avatar_metadata_updates();

    let error = service
        .save_aibb_avatar(encoded_test_image(ImageFormat::Png), "image/png".into())
        .await
        .unwrap_err();

    assert_eq!(error.code, "storageUnavailable");
    assert_eq!(db.persisted_profile(), previous_row);
    assert_eq!(
        fs::read(db.app_data_dir().join("aibb-profile/avatar.webp")).unwrap(),
        previous_file
    );
    assert_eq!(service.load_aibb_profile().await.unwrap(), previous);
}

#[tokio::test]
async fn profile_avatar_reset_database_failure_restores_the_previous_avatar() {
    let db = TestDatabase::new();
    let service = SettingsService::new_with_app_data_dir(
        db.handle(),
        FakeCredentialStore::default(),
        db.app_data_dir(),
    );
    let previous = service
        .save_aibb_avatar(valid_webp_bytes(), "image/webp".into())
        .await
        .unwrap();
    let previous_file = fs::read(db.app_data_dir().join("aibb-profile/avatar.webp")).unwrap();
    let previous_row = db.persisted_profile();
    db.reject_avatar_metadata_updates();

    let error = service.reset_aibb_avatar().await.unwrap_err();

    assert_eq!(error.code, "storageUnavailable");
    assert_eq!(db.persisted_profile(), previous_row);
    assert_eq!(
        fs::read(db.app_data_dir().join("aibb-profile/avatar.webp")).unwrap(),
        previous_file
    );
    assert_eq!(service.load_aibb_profile().await.unwrap(), previous);
}

#[tokio::test]
async fn profile_load_recovers_a_valid_backup_left_by_an_interrupted_avatar_write() {
    let db = TestDatabase::new();
    let service = SettingsService::new_with_app_data_dir(
        db.handle(),
        FakeCredentialStore::default(),
        db.app_data_dir(),
    );
    let saved = service
        .save_aibb_avatar(valid_webp_bytes(), "image/webp".into())
        .await
        .unwrap();
    let directory = db.app_data_dir().join("aibb-profile");
    let target = directory.join("avatar.webp");
    let backup = directory.join(".avatar-crash.backup");
    fs::rename(&target, &backup).unwrap();

    let recovered = service.load_aibb_profile().await.unwrap();

    assert_eq!(recovered, saved);
    assert!(target.exists());
    assert!(!backup.exists());
    assert_eq!(db.persisted_profile().1.as_deref(), Some("avatar.webp"));
}

#[tokio::test]
async fn profile_load_falls_back_to_default_when_no_valid_owned_avatar_survives() {
    let db = TestDatabase::new();
    let service = SettingsService::new_with_app_data_dir(
        db.handle(),
        FakeCredentialStore::default(),
        db.app_data_dir(),
    );
    service
        .save_aibb_avatar(valid_webp_bytes(), "image/webp".into())
        .await
        .unwrap();
    fs::remove_file(db.app_data_dir().join("aibb-profile/avatar.webp")).unwrap();

    let recovered = service.load_aibb_profile().await.unwrap();

    assert_eq!(recovered.avatar_data_url, None);
    assert_eq!(recovered.version, 2);
    assert_eq!(db.persisted_profile(), ("AIbb".into(), None, 2));
}

#[tokio::test]
async fn profile_load_falls_back_to_default_when_the_owned_avatar_is_corrupt() {
    let db = TestDatabase::new();
    let service = SettingsService::new_with_app_data_dir(
        db.handle(),
        FakeCredentialStore::default(),
        db.app_data_dir(),
    );
    service
        .save_aibb_avatar(valid_webp_bytes(), "image/webp".into())
        .await
        .unwrap();
    let target = db.app_data_dir().join("aibb-profile/avatar.webp");
    fs::write(&target, b"not a webp").unwrap();

    let recovered = service.load_aibb_profile().await.unwrap();

    assert_eq!(recovered.avatar_data_url, None);
    assert_eq!(recovered.version, 2);
    assert_eq!(db.persisted_profile(), ("AIbb".into(), None, 2));
    assert!(!target.exists());
}

#[tokio::test]
async fn profile_storage_access_failure_precedes_name_persistence() {
    let db = TestDatabase::new();
    let service = SettingsService::new_with_app_data_dir(
        db.handle(),
        FakeCredentialStore::default(),
        db.app_data_dir(),
    );
    service
        .save_aibb_avatar(valid_webp_bytes(), "image/webp".into())
        .await
        .unwrap();
    let profile_directory = db.app_data_dir().join("aibb-profile");
    let preserved_directory = db.app_data_dir().join("aibb-profile-preserved");
    fs::rename(&profile_directory, &preserved_directory).unwrap();
    fs::write(&profile_directory, b"not a directory").unwrap();
    let previous_row = db.persisted_profile();

    let error = service.save_aibb_name("新名字".into()).await.unwrap_err();

    assert_eq!(error.code, "profileStorageUnavailable");
    assert_eq!(db.persisted_profile(), previous_row);
    assert!(preserved_directory.join("avatar.webp").exists());
}

#[tokio::test]
async fn profile_avatar_import_is_app_owned_and_path_free() {
    let db = TestDatabase::new();
    let service = SettingsService::new_with_app_data_dir(
        db.handle(),
        FakeCredentialStore::default(),
        db.app_data_dir(),
    );
    let avatar = valid_webp_bytes();

    let profile = service
        .save_aibb_avatar(avatar.clone(), "image/webp".into())
        .await
        .unwrap();

    assert!(profile
        .avatar_data_url
        .as_deref()
        .unwrap()
        .starts_with("data:image/webp;base64,"));
    let stored_avatar = fs::read(db.app_data_dir().join("aibb-profile/avatar.webp")).unwrap();
    image::load_from_memory_with_format(&stored_avatar, ImageFormat::WebP).unwrap();
    let stored_filename: Option<String> = rusqlite::Connection::open(db.path())
        .unwrap()
        .query_row(
            "SELECT avatar_filename FROM app_settings WHERE singleton = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(stored_filename.as_deref(), Some("avatar.webp"));
    let database_bytes = db.raw_bytes();
    assert!(!database_bytes
        .windows(stored_avatar.len())
        .any(|window| window == stored_avatar));
    assert!(!database_bytes
        .windows(b"C:\\Users\\face.png".len())
        .any(|window| window == b"C:\\Users\\face.png"));
}

#[tokio::test]
async fn profile_invalid_avatar_keeps_the_previous_avatar() {
    let db = TestDatabase::new();
    let service = SettingsService::new_with_app_data_dir(
        db.handle(),
        FakeCredentialStore::default(),
        db.app_data_dir(),
    );
    let avatar = valid_webp_bytes();
    let seeded = service
        .save_aibb_avatar(avatar.clone(), "image/webp".into())
        .await
        .unwrap();
    let previous_file = fs::read(db.app_data_dir().join("aibb-profile/avatar.webp")).unwrap();

    let invalid_mime = service
        .save_aibb_avatar(b"not an image".to_vec(), "text/plain".into())
        .await
        .unwrap_err();
    let too_large = service
        .save_aibb_avatar(vec![0; 5 * 1024 * 1024 + 1], "image/png".into())
        .await
        .unwrap_err();

    assert_eq!(invalid_mime.code, "invalidProfile");
    assert_eq!(too_large.code, "invalidProfile");
    assert_eq!(service.load_aibb_profile().await.unwrap(), seeded);
    assert_eq!(
        fs::read(db.app_data_dir().join("aibb-profile/avatar.webp")).unwrap(),
        previous_file
    );
}

#[tokio::test]
async fn profile_avatar_reset_removes_the_app_owned_file() {
    let db = TestDatabase::new();
    let service = SettingsService::new_with_app_data_dir(
        db.handle(),
        FakeCredentialStore::default(),
        db.app_data_dir(),
    );
    service
        .save_aibb_avatar(valid_webp_bytes(), "image/webp".into())
        .await
        .unwrap();

    let profile = service.reset_aibb_avatar().await.unwrap();

    assert_eq!(profile.avatar_data_url, None);
    assert_eq!(profile.version, 2);
    assert!(!db.app_data_dir().join("aibb-profile/avatar.webp").exists());
    let stored_filename: Option<String> = rusqlite::Connection::open(db.path())
        .unwrap()
        .query_row(
            "SELECT avatar_filename FROM app_settings WHERE singleton = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(stored_filename, None);
}

#[tokio::test]
async fn profile_defaults_to_aibb_and_survives_reopen() {
    let db = TestDatabase::new();
    let service = SettingsService::new(db.handle(), FakeCredentialStore::default());

    assert_eq!(
        service.load_aibb_profile().await.unwrap(),
        AibbProfile {
            name: "AIbb".into(),
            avatar_data_url: None,
            version: 0,
        }
    );

    service.save_aibb_name("  小团子  ".into()).await.unwrap();

    assert_eq!(
        SettingsService::new(db.reopen(), FakeCredentialStore::default())
            .load_aibb_profile()
            .await
            .unwrap(),
        AibbProfile {
            name: "小团子".into(),
            avatar_data_url: None,
            version: 1,
        }
    );
}

#[tokio::test]
async fn invalid_name_keeps_the_existing_profile() {
    let db = TestDatabase::new();
    let service = SettingsService::new(db.handle(), FakeCredentialStore::default());
    service.save_aibb_name("小团子".into()).await.unwrap();

    for invalid_name in ["   ", "abcdefghijklmnopqrstuvwxy", "团\u{0007}子"] {
        let error = service
            .save_aibb_name(invalid_name.into())
            .await
            .unwrap_err();
        assert_eq!(error.code, "invalidProfile");
    }

    assert_eq!(
        service.load_aibb_profile().await.unwrap(),
        AibbProfile {
            name: "小团子".into(),
            avatar_data_url: None,
            version: 1,
        }
    );
}

#[tokio::test]
async fn aibb_name_accepts_twenty_four_unicode_scalars() {
    let db = TestDatabase::new();
    let service = SettingsService::new(db.handle(), FakeCredentialStore::default());
    let name = "团".repeat(24);

    service.save_aibb_name(name.clone()).await.unwrap();

    assert_eq!(service.load_aibb_profile().await.unwrap().name, name);
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
async fn rejects_a_blank_model_before_persisting_settings() {
    let db = TestDatabase::new();
    let service = SettingsService::new(db.handle(), FakeCredentialStore::default());

    let error = service
        .save(SaveSettings {
            api_base: "https://api.deepseek.com".into(),
            model: "   ".into(),
            api_key: None,
            web_mode: WebMode::Auto,
            always_on_top: true,
            autostart: false,
        })
        .await
        .unwrap_err();

    assert_eq!(error.code, "invalidSettings");
    assert!(db.handle().load_settings().unwrap().model.is_empty());
}

#[tokio::test]
async fn rejects_non_https_or_credential_bearing_api_bases_before_persistence() {
    for api_base in [
        "http://example.test/v1",
        "http://127.0.0.1:11434/v1",
        "not a url",
        "https://user:secret@example.test/v1",
    ] {
        let db = TestDatabase::new();
        let vault = FakeCredentialStore::default();
        let service = SettingsService::new(db.handle(), vault.clone());

        let error = service
            .save(SaveSettings {
                api_base: api_base.into(),
                model: "model-a".into(),
                api_key: Some("must-not-be-saved".into()),
                web_mode: WebMode::Auto,
                always_on_top: true,
                autostart: false,
            })
            .await
            .unwrap_err();

        assert_eq!(error.code, "invalidSettings", "api base: {api_base}");
        assert!(db.handle().load_settings().unwrap().api_base.is_empty());
        assert_eq!(vault.get().await.unwrap(), None);
    }
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
async fn blank_replacement_key_preserves_the_existing_protected_credential() {
    let db = TestDatabase::new();
    let vault = FakeCredentialStore::default();
    vault.set("existing-secret").await.unwrap();
    let service = SettingsService::new(db.handle(), vault.clone());

    service
        .save(SaveSettings {
            api_base: "https://example.test/v1".into(),
            model: "model-b".into(),
            api_key: Some("   ".into()),
            web_mode: WebMode::Auto,
            always_on_top: true,
            autostart: false,
        })
        .await
        .unwrap();

    assert_eq!(
        vault.get().await.unwrap().as_deref(),
        Some("existing-secret")
    );
    assert!(service.load().await.unwrap().api_configured);
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
    db.handle().set_first_run_complete().unwrap();
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

#[tokio::test]
async fn renderer_bootstrap_refreshes_after_the_protected_key_changes() {
    let db = TestDatabase::new();
    let vault = FakeCredentialStore::default();
    let service = SettingsService::new(db.handle(), vault.clone());
    let state = AppState::new(
        BootstrapState {
            first_run: true,
            pet_status: PetStatus::Idle,
            api_configured: false,
        },
        service,
        MemoryRepository::new(db.handle()),
        ArchiveService::new(db.handle(), std::env::temp_dir()),
    );

    vault.set("newly-configured-key").await.unwrap();

    assert!(
        current_bootstrap_state(&state)
            .await
            .unwrap()
            .api_configured
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
        MemoryRepository::new(db.handle()),
        ArchiveService::new(db.handle(), std::env::temp_dir()),
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
fn migrations_create_the_required_schema_without_an_api_key_column() {
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
            "pet_y",
            "aibb_name",
            "avatar_filename",
            "profile_version",
            "archive_root",
            "archive_auto_discover"
        ]
    );

    let mut exploration_column_statement = connection
        .prepare("PRAGMA table_info(explorations)")
        .unwrap();
    let exploration_columns = exploration_column_statement
        .query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(
        exploration_columns,
        vec![
            "id",
            "status",
            "user_direction",
            "items_json",
            "next_outing_request",
            "raw_response",
            "error_code",
            "created_at",
            "updated_at",
            "diary",
            "sources_json",
            "round_number",
            "elapsed_seconds",
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
