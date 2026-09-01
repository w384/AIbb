use std::sync::Arc;

use async_trait::async_trait;
use tauri::Emitter;
use uuid::Uuid;

use crate::{
    app_state::AppState,
    error::{AppError, ErrorCode},
    exploration::{
        DefaultPublicWebFactory, ExplorationEvent, ExplorationEventSink, ExplorationOrchestrator,
        ExplorationRequest, ExplorationRuntimeFactory, ExplorationTaskCredential,
        ExplorationTaskRuntime, NoopNotifier,
    },
    llm::OpenAiClient,
    memory::MemoryRepository,
    settings::{FixedCredentialStore, SettingsService},
    storage::Database,
};

pub(crate) fn build_exploration_orchestrator(
    app: tauri::AppHandle,
    database: Database,
    memory: MemoryRepository,
    settings: SettingsService,
) -> ExplorationOrchestrator {
    ExplorationOrchestrator::with_runtime_factory(
        Arc::new(database),
        Arc::new(memory),
        Arc::new(SettingsExplorationRuntimeFactory::new(settings)),
        Arc::new(DefaultPublicWebFactory),
        Arc::new(TauriEventSink { app }),
        Arc::new(NoopNotifier),
    )
}

pub struct SettingsExplorationRuntimeFactory {
    settings: SettingsService,
}

impl SettingsExplorationRuntimeFactory {
    pub fn new(settings: SettingsService) -> Self {
        Self { settings }
    }
}

#[async_trait]
impl ExplorationRuntimeFactory for SettingsExplorationRuntimeFactory {
    async fn create(&self) -> Result<ExplorationTaskRuntime, AppError> {
        let snapshot = self.settings.exploration_task_snapshot().await?;
        let (settings, api_key) = snapshot.into_parts();
        let credential = api_key
            .as_ref()
            .filter(|key| !key.is_empty())
            .map(|key| ExplorationTaskCredential::exact(key.clone()))
            .unwrap_or_else(ExplorationTaskCredential::missing);
        let web_mode = settings.web_mode;
        let llm = OpenAiClient::new(settings, FixedCredentialStore::new(api_key));
        Ok(ExplorationTaskRuntime::new(
            Arc::new(llm),
            web_mode,
            credential,
        ))
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
