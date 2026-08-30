use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::{
    domain::{BootstrapState, PetStatus, WebMode},
    error::AppError,
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

#[derive(Clone)]
pub struct SettingsService {
    database: Database,
    credentials: Arc<dyn CredentialStore>,
}

impl SettingsService {
    pub fn new<C>(database: Database, credentials: C) -> Self
    where
        C: CredentialStore + 'static,
    {
        Self {
            database,
            credentials: Arc::new(credentials),
        }
    }

    pub async fn load(&self) -> Result<ApiSettings, AppError> {
        let (persisted, api_configured) = self.load_persisted_state().await?;
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

    pub async fn save(&self, settings: SaveSettings) -> Result<(), AppError> {
        self.database.save_settings(
            &settings.api_base,
            &settings.model,
            settings.web_mode.as_storage_value(),
            settings.always_on_top,
            settings.autostart,
        )?;

        if let Some(api_key) = settings.api_key {
            self.credentials
                .set(&api_key)
                .await
                .map_err(|error| sanitize_credential_error(error, Some(&api_key)))?;
        }

        Ok(())
    }

    pub async fn clear_api_key(&self) -> Result<(), AppError> {
        let current_key = self
            .credentials
            .get()
            .await
            .map_err(|error| sanitize_credential_error(error, None))?;
        self.credentials
            .clear()
            .await
            .map_err(|error| sanitize_credential_error(error, current_key.as_deref()))
    }

    pub async fn mark_connection_verified(&self) -> Result<(), AppError> {
        self.database.set_first_run_complete()
    }

    pub async fn load_bootstrap_state(&self) -> Result<BootstrapState, AppError> {
        let (persisted, api_configured) = self.load_persisted_state().await?;

        Ok(BootstrapState {
            first_run: !persisted.first_run_complete,
            pet_status: PetStatus::Idle,
            api_configured,
        })
    }

    pub fn persisted_settings(&self) -> Result<PersistedSettings, AppError> {
        self.database.load_settings()
    }

    pub fn save_pet_position(&self, x: i32, y: i32) -> Result<(), AppError> {
        self.database.save_pet_position(x, y)
    }

    async fn load_persisted_state(&self) -> Result<(PersistedSettings, bool), AppError> {
        let persisted = self.database.load_settings()?;
        let api_configured = self
            .credentials
            .get()
            .await
            .map_err(|error| sanitize_credential_error(error, None))?
            .is_some();
        Ok((persisted, api_configured))
    }
}

fn parse_web_mode(settings: &PersistedSettings) -> Result<WebMode, AppError> {
    WebMode::from_storage_value(&settings.web_mode).ok_or_else(|| AppError {
        code: "invalidSettings".to_string(),
        message: "Application settings are invalid.".to_string(),
    })
}

fn sanitize_credential_error(error: AppError, current_key: Option<&str>) -> AppError {
    AppError::sanitized(error.code, error.message, current_key)
}
