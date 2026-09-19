use std::sync::RwLock;

use crate::{
    archive::service::ArchiveService,
    commands::chat::ChatService,
    domain::BootstrapState,
    exploration::ExplorationOrchestrator,
    memory::MemoryRepository,
    settings::SettingsService,
};

pub struct AppState {
    pub bootstrap: RwLock<BootstrapState>,
    pub settings: SettingsService,
    pub memory: MemoryRepository,
    pub archive: ArchiveService,
    pub chat: Option<ChatService>,
    pub vocab: Option<ChatService>,
    pub exploration: Option<ExplorationOrchestrator>,
}

impl AppState {
    pub fn new(
        bootstrap: BootstrapState,
        settings: SettingsService,
        memory: MemoryRepository,
        archive: ArchiveService,
    ) -> Self {
        Self {
            bootstrap: RwLock::new(bootstrap),
            settings,
            memory,
            archive,
            chat: None,
            vocab: None,
            exploration: None,
        }
    }

    pub fn with_exploration(
        bootstrap: BootstrapState,
        settings: SettingsService,
        memory: MemoryRepository,
        archive: ArchiveService,
        exploration: ExplorationOrchestrator,
    ) -> Self {
        let mut state = Self::new(bootstrap, settings, memory, archive);
        state.exploration = Some(exploration);
        state
    }

    pub fn with_services(
        bootstrap: BootstrapState,
        settings: SettingsService,
        memory: MemoryRepository,
        archive: ArchiveService,
        chat: ChatService,
        vocab: ChatService,
        exploration: ExplorationOrchestrator,
    ) -> Self {
        let mut state = Self::with_exploration(bootstrap, settings, memory, archive, exploration);
        state.chat = Some(chat);
        state.vocab = Some(vocab);
        state
    }
}
