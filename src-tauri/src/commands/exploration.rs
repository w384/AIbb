use std::sync::Arc;

use async_trait::async_trait;
use tauri::Emitter;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::{
    app_state::AppState,
    error::{AppError, ErrorCode},
    exploration::{
        DefaultPublicWebFactory, ExplorationEvent, ExplorationEventSink, ExplorationOrchestrator,
        ExplorationPreferences, ExplorationRequest, ExplorationSecrets, NoopNotifier,
    },
    llm::{ChatRequest, DeltaSink, LlmTransport, NativeWebOutcome, NativeWebRequest, OpenAiClient},
    memory::MemoryRepository,
    settings::{CredentialStore, NativeCredentialStore, SettingsService},
    storage::Database,
};

pub(crate) fn build_exploration_orchestrator(
    app: tauri::AppHandle,
    database: Database,
    memory: MemoryRepository,
    settings: SettingsService,
) -> ExplorationOrchestrator {
    ExplorationOrchestrator::with_preferences(
        Arc::new(database),
        Arc::new(memory),
        Arc::new(SettingsBackedLlm::new(settings.clone())),
        Arc::new(DefaultPublicWebFactory),
        Arc::new(TauriEventSink { app }),
        Arc::new(NoopNotifier),
        Arc::new(SettingsPreferences { settings }),
        Arc::new(CredentialSecrets),
    )
}

struct CredentialSecrets;

#[async_trait]
impl ExplorationSecrets for CredentialSecrets {
    async fn current_api_key(&self) -> Result<Option<String>, AppError> {
        NativeCredentialStore.get().await
    }
}

#[tauri::command]
pub async fn start_exploration(
    state: tauri::State<'_, AppState>,
    request: ExplorationRequest,
) -> Result<String, AppError> {
    state
        .exploration
        .as_ref()
        .ok_or_else(exploration_service_error)?
        .start(request)
        .await
        .map(|task_id| task_id.to_string())
}

#[tauri::command]
pub async fn cancel_exploration(
    state: tauri::State<'_, AppState>,
    task_id: String,
) -> Result<(), AppError> {
    let task_id =
        Uuid::parse_str(&task_id).map_err(|_| AppError::from_code(ErrorCode::InvalidRequest))?;
    state
        .exploration
        .as_ref()
        .ok_or_else(exploration_service_error)?
        .cancel(task_id)
        .await
}

fn exploration_service_error() -> AppError {
    AppError::new(
        "exploration_service_unavailable",
        "The exploration service is unavailable.",
    )
}

struct SettingsPreferences {
    settings: SettingsService,
}

#[async_trait]
impl ExplorationPreferences for SettingsPreferences {
    async fn web_mode(&self) -> Result<crate::domain::WebMode, AppError> {
        Ok(self.settings.load().await?.web_mode)
    }
}

struct SettingsBackedLlm {
    settings: SettingsService,
}

impl SettingsBackedLlm {
    fn new(settings: SettingsService) -> Self {
        Self { settings }
    }

    async fn client(&self) -> Result<OpenAiClient, AppError> {
        let settings = self.settings.load().await?;
        Ok(OpenAiClient::from_shared(
            settings,
            Arc::new(NativeCredentialStore),
        ))
    }
}

#[async_trait]
impl LlmTransport for SettingsBackedLlm {
    async fn stream_chat(
        &self,
        request: ChatRequest,
        sink: &dyn DeltaSink,
        cancellation: CancellationToken,
    ) -> Result<(), AppError> {
        self.client()
            .await?
            .stream_chat(request, sink, cancellation)
            .await
    }

    async fn complete(
        &self,
        request: ChatRequest,
        cancellation: CancellationToken,
    ) -> Result<String, AppError> {
        self.client().await?.complete(request, cancellation).await
    }

    async fn try_native_web(
        &self,
        request: NativeWebRequest,
        cancellation: CancellationToken,
    ) -> Result<NativeWebOutcome, AppError> {
        self.client()
            .await?
            .try_native_web(request, cancellation)
            .await
    }

    async fn test_connection(&self, cancellation: CancellationToken) -> Result<(), AppError> {
        self.client().await?.test_connection(cancellation).await
    }
}

struct TauriEventSink {
    app: tauri::AppHandle,
}

#[async_trait]
impl ExplorationEventSink for TauriEventSink {
    async fn emit(&self, event: ExplorationEvent) -> Result<(), AppError> {
        self.app.emit(event.name(), &event).map_err(|_| {
            AppError::new(
                "exploration_event_unavailable",
                "The exploration event could not be delivered.",
            )
        })
    }
}
