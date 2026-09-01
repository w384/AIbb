use async_trait::async_trait;

use crate::error::AppError;

#[async_trait]
pub trait CredentialStore: Send + Sync {
    async fn get(&self) -> Result<Option<String>, AppError>;
    async fn set(&self, api_key: &str) -> Result<(), AppError>;
    async fn clear(&self) -> Result<(), AppError>;
}

const SERVICE: &str = "com.clink.aibb";
const ACCOUNT: &str = "model-api-key";

#[derive(Debug, Clone, Copy, Default)]
pub struct NativeCredentialStore;

impl NativeCredentialStore {
    fn entry() -> Result<keyring::Entry, AppError> {
        keyring::Entry::new(SERVICE, ACCOUNT).map_err(|_| credential_error())
    }
}

pub(crate) struct FixedCredentialStore {
    api_key: Option<String>,
}

impl FixedCredentialStore {
    pub(crate) fn new(api_key: Option<String>) -> Self {
        Self { api_key }
    }
}

#[async_trait]
impl CredentialStore for FixedCredentialStore {
    async fn get(&self) -> Result<Option<String>, AppError> {
        Ok(self.api_key.clone())
    }

    async fn set(&self, _api_key: &str) -> Result<(), AppError> {
        Err(fixed_credential_error())
    }

    async fn clear(&self) -> Result<(), AppError> {
        Err(fixed_credential_error())
    }
}

#[async_trait]
impl CredentialStore for NativeCredentialStore {
    async fn get(&self) -> Result<Option<String>, AppError> {
        match Self::entry()?.get_password() {
            Ok(api_key) => Ok(Some(api_key)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err(credential_error()),
        }
    }

    async fn set(&self, api_key: &str) -> Result<(), AppError> {
        Self::entry()?
            .set_password(api_key)
            .map_err(|_| credential_error())
    }

    async fn clear(&self) -> Result<(), AppError> {
        match Self::entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err(credential_error()),
        }
    }
}

fn credential_error() -> AppError {
    AppError::new(
        "credentialStoreUnavailable",
        "The protected API credential could not be accessed.",
    )
}

fn fixed_credential_error() -> AppError {
    AppError::new(
        "fixedCredentialSnapshot",
        "The task credential snapshot cannot be changed.",
    )
}
