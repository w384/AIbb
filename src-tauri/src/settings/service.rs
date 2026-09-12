use std::{path::PathBuf, sync::Arc};

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex as AsyncMutex;
use tokio_util::sync::CancellationToken;

use crate::{
    domain::{AibbProfile, BootstrapState, PetStatus, WebMode},
    error::AppError,
    llm::{LlmTransport, OpenAiClient},
    storage::{Database, PersistedSettings},
};

use super::{
    profile::{
        avatar_data_url, load_or_recover_avatar_data_url, normalize_avatar, read_avatar_bytes,
        validate_aibb_name, AvatarFileTransaction,
    },
    CredentialStore, FixedCredentialStore,
};

type TransportFactory = dyn Fn(ApiSettings, Option<String>) -> Arc<dyn LlmTransport> + Send + Sync;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ApiSettings {
    pub api_base: String,
    pub model: String,
    pub web_mode: WebMode,
    pub always_on_top: bool,
    pub autostart: bool,
    pub api_configured: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveSettings {
    pub api_base: String,
    pub model: String,
    pub api_key: Option<String>,
    pub web_mode: WebMode,
    pub always_on_top: bool,
    pub autostart: bool,
}

pub struct ExplorationTaskSnapshot {
    settings: ApiSettings,
    api_key: Option<String>,
}

impl ExplorationTaskSnapshot {
    pub(crate) fn into_parts(self) -> (ApiSettings, Option<String>) {
        (self.settings, self.api_key)
    }
}

#[derive(Clone)]
pub struct SettingsService {
    database: Database,
    credentials: Arc<dyn CredentialStore>,
    operation: Arc<AsyncMutex<()>>,
    profile_directory: Option<Arc<PathBuf>>,
    transport_factory: Arc<TransportFactory>,
}

impl SettingsService {
    pub fn new<C>(database: Database, credentials: C) -> Self
    where
        C: CredentialStore + 'static,
    {
        Self::new_with_transport_factory(database, credentials, |settings, api_key| {
            Arc::new(OpenAiClient::new(
                settings,
                FixedCredentialStore::new(api_key),
            ))
        })
    }

    pub fn new_with_transport_factory<C, F>(
        database: Database,
        credentials: C,
        transport_factory: F,
    ) -> Self
    where
        C: CredentialStore + 'static,
        F: Fn(ApiSettings, Option<String>) -> Arc<dyn LlmTransport> + Send + Sync + 'static,
    {
        Self {
            database,
            credentials: Arc::new(credentials),
            operation: Arc::new(AsyncMutex::new(())),
            profile_directory: None,
            transport_factory: Arc::new(transport_factory),
        }
    }

    pub fn new_with_app_data_dir<C>(
        database: Database,
        credentials: C,
        app_data_dir: impl Into<PathBuf>,
    ) -> Self
    where
        C: CredentialStore + 'static,
    {
        let mut service = Self::new(database, credentials);
        service.profile_directory = Some(Arc::new(app_data_dir.into().join("aibb-profile")));
        service
    }

    pub async fn load(&self) -> Result<ApiSettings, AppError> {
        let _operation = self.operation.lock().await;
        let (persisted, api_configured) = self.load_persisted_state_unlocked().await?;
        let web_mode = parse_web_mode(&persisted)?;

        Ok(ApiSettings {
            api_base: persisted.api_base,
            model: persisted.model,
            web_mode,
            always_on_top: persisted.always_on_top,
            autostart: persisted.autostart,
            api_configured,
        })
    }

    pub async fn load_aibb_profile(&self) -> Result<AibbProfile, AppError> {
        let _operation = self.operation.lock().await;
        self.load_aibb_profile_unlocked()
    }

    fn load_aibb_profile_unlocked(&self) -> Result<AibbProfile, AppError> {
        let profile = self.database.load_aibb_profile()?;
        let (avatar_data_url, version) = match profile.avatar_filename.as_deref() {
            Some(filename) => {
                let avatar =
                    load_or_recover_avatar_data_url(self.profile_directory()?, Some(filename))?;
                match avatar {
                    Some(avatar) => (Some(avatar), profile.version),
                    None => (None, self.database.set_aibb_avatar_present(false)?),
                }
            }
            None => (None, profile.version),
        };

        Ok(AibbProfile {
            name: profile.name,
            avatar_data_url,
            version,
        })
    }

    pub async fn save_aibb_name(&self, name: String) -> Result<AibbProfile, AppError> {
        let _operation = self.operation.lock().await;
        let name = validate_aibb_name(name)?;
        let previous = self.load_aibb_profile_unlocked()?;
        let version = self.database.save_aibb_name(&name)?;
        Ok(AibbProfile {
            name,
            avatar_data_url: previous.avatar_data_url,
            version,
        })
    }

    pub async fn save_aibb_avatar(
        &self,
        bytes: Vec<u8>,
        mime_type: String,
    ) -> Result<AibbProfile, AppError> {
        let _operation = self.operation.lock().await;
        let normalized = normalize_avatar(&bytes, &mime_type)?;
        let previous = self.load_aibb_profile_unlocked()?;
        let transaction =
            AvatarFileTransaction::replace(self.profile_directory()?, Some(&normalized))?;
        let version = match self.database.set_aibb_avatar_present(true) {
            Ok(version) => version,
            Err(error) => {
                transaction.rollback()?;
                return Err(error);
            }
        };
        transaction.commit();
        Ok(AibbProfile {
            name: previous.name,
            avatar_data_url: Some(avatar_data_url(&normalized)),
            version,
        })
    }

    pub async fn reset_aibb_avatar(&self) -> Result<AibbProfile, AppError> {
        let _operation = self.operation.lock().await;
        let previous = self.load_aibb_profile_unlocked()?;
        let transaction = AvatarFileTransaction::replace(self.profile_directory()?, None)?;
        let version = match self.database.set_aibb_avatar_present(false) {
            Ok(version) => version,
            Err(error) => {
                transaction.rollback()?;
                return Err(error);
            }
        };
        transaction.commit();
        Ok(AibbProfile {
            name: previous.name,
            avatar_data_url: None,
            version,
        })
    }

    /// The stored normalized avatar bytes (`None` when no avatar is set), for
    /// deriving tray and desktop-shortcut icons.
    pub async fn load_avatar_bytes(&self) -> Result<Option<Vec<u8>>, AppError> {
        let _operation = self.operation.lock().await;
        read_avatar_bytes(self.profile_directory()?)
    }

    pub async fn exploration_task_snapshot(&self) -> Result<ExplorationTaskSnapshot, AppError> {
        let _operation = self.operation.lock().await;
        let persisted = self.database.load_settings()?;
        let web_mode = parse_web_mode(&persisted)?;
        validate_required_settings(&persisted.api_base, &persisted.model)?;
        let api_key = self
            .credentials
            .get()
            .await
            .map_err(|_| credential_store_access_error())?
            .and_then(normalize_replacement_key);

        Ok(ExplorationTaskSnapshot {
            settings: ApiSettings {
                api_base: persisted.api_base,
                model: persisted.model,
                web_mode,
                always_on_top: persisted.always_on_top,
                autostart: persisted.autostart,
                api_configured: api_key.as_ref().is_some_and(|key| !key.is_empty()),
            },
            api_key,
        })
    }

    pub async fn save(&self, settings: SaveSettings) -> Result<(), AppError> {
        let _operation = self.operation.lock().await;
        let api_base = settings.api_base.trim().to_string();
        let model = settings.model.trim().to_string();
        validate_required_settings(&api_base, &model)?;
        let replacement_key = settings.api_key.and_then(normalize_replacement_key);
        let previous = self.database.load_settings()?;
        self.database.save_settings(
            &api_base,
            &model,
            settings.web_mode.as_storage_value(),
            settings.always_on_top,
            settings.autostart,
        )?;

        if let Some(api_key) = replacement_key {
            if self.credentials.set(&api_key).await.is_err() {
                if self
                    .database
                    .save_settings(
                        &previous.api_base,
                        &previous.model,
                        &previous.web_mode,
                        previous.always_on_top,
                        previous.autostart,
                    )
                    .is_err()
                {
                    return Err(settings_rollback_error());
                }
                return Err(credential_store_write_error());
            }
        }

        Ok(())
    }

    pub async fn clear_api_key(&self) -> Result<(), AppError> {
        let _operation = self.operation.lock().await;
        self.credentials
            .clear()
            .await
            .map_err(|_| credential_store_clear_error())
    }

    pub async fn test_connection(&self) -> Result<(), AppError> {
        let _operation = self.operation.lock().await;
        let persisted = self.database.load_settings()?;
        let web_mode = parse_web_mode(&persisted)?;
        validate_required_settings(&persisted.api_base, &persisted.model)?;
        let settings = ApiSettings {
            api_base: persisted.api_base,
            model: persisted.model,
            web_mode,
            always_on_top: persisted.always_on_top,
            autostart: persisted.autostart,
            api_configured: false,
        };
        let api_key = self
            .credentials
            .get()
            .await
            .map_err(|_| credential_store_access_error())?;
        let transport = (self.transport_factory)(settings, api_key);

        transport.test_connection(CancellationToken::new()).await?;
        self.database.set_first_run_complete()
    }

    pub async fn list_available_models(&self) -> Result<Vec<String>, AppError> {
        let _operation = self.operation.lock().await;
        let persisted = self.database.load_settings()?;
        if persisted.api_base.trim().is_empty() {
            return Err(AppError::new(
                "invalidSettings",
                "API 地址不能为空，请先填写 API 地址。",
            ));
        }
        let web_mode = parse_web_mode(&persisted)?;
        let settings = ApiSettings {
            api_base: persisted.api_base,
            model: persisted.model,
            web_mode,
            always_on_top: persisted.always_on_top,
            autostart: persisted.autostart,
            api_configured: false,
        };
        let api_key = self
            .credentials
            .get()
            .await
            .map_err(|_| credential_store_access_error())?;
        let transport = (self.transport_factory)(settings, api_key);
        transport.list_models().await
    }

    pub(crate) fn exploration_transport(
        &self,
        settings: ApiSettings,
        api_key: Option<String>,
    ) -> Arc<dyn LlmTransport> {
        (self.transport_factory)(settings, api_key)
    }

    pub async fn load_bootstrap_state(&self) -> Result<BootstrapState, AppError> {
        let _operation = self.operation.lock().await;
        let (persisted, api_configured) = self.load_persisted_state_unlocked().await?;

        Ok(BootstrapState {
            first_run: !persisted.first_run_complete,
            pet_status: PetStatus::Idle,
            api_configured,
        })
    }

    pub fn pet_position(&self) -> Result<Option<(i32, i32)>, AppError> {
        self.database.load_pet_position()
    }

    pub fn save_pet_position(&self, x: i32, y: i32) -> Result<(), AppError> {
        self.database.save_pet_position(x, y)
    }

    async fn load_persisted_state_unlocked(&self) -> Result<(PersistedSettings, bool), AppError> {
        let persisted = self.database.load_settings()?;
        let api_configured = self
            .credentials
            .get()
            .await
            .map_err(|_| credential_store_access_error())?
            .is_some_and(|key| !key.trim().is_empty());
        Ok((persisted, api_configured))
    }

    fn profile_directory(&self) -> Result<&PathBuf, AppError> {
        self.profile_directory.as_deref().ok_or_else(|| {
            AppError::new(
                "profileStorageUnavailable",
                "The AIbb profile image could not be accessed.",
            )
        })
    }
}

fn normalize_replacement_key(api_key: String) -> Option<String> {
    let api_key = api_key.trim();
    (!api_key.is_empty()).then(|| api_key.to_string())
}

fn validate_required_settings(api_base: &str, model: &str) -> Result<(), AppError> {
    let valid_api_base = url::Url::parse(api_base.trim()).ok().is_some_and(|url| {
        url.scheme() == "https"
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
    });
    if !valid_api_base || model.trim().is_empty() {
        return Err(AppError::new(
            "invalidSettings",
            "A secure HTTPS API address and model name are required.",
        ));
    }
    Ok(())
}

fn parse_web_mode(settings: &PersistedSettings) -> Result<WebMode, AppError> {
    WebMode::from_storage_value(&settings.web_mode)
        .ok_or_else(|| AppError::new("invalidSettings", "Application settings are invalid."))
}

fn credential_store_access_error() -> AppError {
    AppError::new(
        "credentialStoreUnavailable",
        "The protected API credential could not be accessed.",
    )
}

fn credential_store_write_error() -> AppError {
    AppError::new(
        "credentialStoreUnavailable",
        "The protected API credential could not be stored.",
    )
}

fn credential_store_clear_error() -> AppError {
    AppError::new(
        "credentialStoreUnavailable",
        "The protected API credential could not be cleared.",
    )
}

fn settings_rollback_error() -> AppError {
    AppError::new(
        "settingsRollbackFailed",
        "Previous settings could not be restored after credential storage failed.",
    )
}
