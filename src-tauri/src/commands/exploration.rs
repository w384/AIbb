use std::sync::Arc;

use async_trait::async_trait;
use tauri::Emitter;

use crate::{
    error::AppError,
    exploration::{
        DefaultPublicWebFactory, ExplorationEvent, ExplorationEventSink, ExplorationOrchestrator,
        ExplorationRuntimeFactory, ExplorationTaskCredential, ExplorationTaskRuntime, NoopNotifier,
    },
    memory::MemoryRepository,
    settings::SettingsService,
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
        let persona = settings.persona.clone();
        let llm = self.settings.exploration_transport(settings, api_key);
        Ok(ExplorationTaskRuntime::new(
            llm,
            web_mode,
            credential,
            persona,
        ))
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
