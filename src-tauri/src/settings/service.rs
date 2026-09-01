use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex as AsyncMutex;
use tokio_util::sync::CancellationToken;

use crate::{
    domain::{BootstrapState, PetStatus, WebMode},
    error::AppError,
    llm::{LlmTransport, OpenAiClient},
    storage::{Database, PersistedSettings},
};

use super::CredentialStore;

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
}

impl SettingsService {
    pub fn new<C>(database: Database, credentials: C) -> Self
    where
        C: CredentialStore + 'static,
    {
        Self {
            database,
            credentials: Arc::new(credentials),
            operation: Arc::new(AsyncMutex::new(())),
        }
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

    pub async fn exploration_task_snapshot(&self) -> Result<ExplorationTaskSnapshot, AppError> {
        let _operation = self.operation.lock().await;
        let persisted = self.database.load_settings()?;
        let web_mode = parse_web_mode(&persisted)?;
        let api_key = self
            .credentials
            .get()
            .await
            .map_err(|_| credential_store_access_error())?;

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
        let previous = self.database.load_settings()?;
        self.database.save_settings(
            &settings.api_base,
            &settings.model,
            settings.web_mode.as_storage_value(),
            settings.always_on_top,
            settings.autostart,
        )?;

        if let Some(api_key) = settings.api_key {
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
        let settings = ApiSettings {
            api_base: persisted.api_base,
            model: persisted.model,
            web_mode,
            always_on_top: persisted.always_on_top,
            autostart: persisted.autostart,
            api_configured: false,
        };
        let transport = OpenAiClient::from_shared(settings, Arc::clone(&self.credentials));

        transport.test_connection(CancellationToken::new()).await?;
        self.database.set_first_run_complete()
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
            .is_some();
        Ok((persisted, api_configured))
    }
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
