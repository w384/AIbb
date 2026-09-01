use std::sync::RwLock;

use crate::commands::chat::ChatService;
use crate::domain::BootstrapState;
use crate::exploration::ExplorationOrchestrator;
use crate::memory::MemoryRepository;
use crate::settings::SettingsService;

pub struct AppState {
    pub bootstrap: RwLock<BootstrapState>,
    pub settings: SettingsService,
    pub memory: MemoryRepository,
    pub chat: Option<ChatService>,
    pub exploration: Option<ExplorationOrchestrator>,
}

impl AppState {
    pub fn new(
        bootstrap: BootstrapState,
        settings: SettingsService,
        memory: MemoryRepository,
    ) -> Self {
        Self {
            bootstrap: RwLock::new(bootstrap),
            settings,
            memory,
            chat: None,
            exploration: None,
        }
    }

    pub fn with_exploration(
        bootstrap: BootstrapState,
        settings: SettingsService,
        memory: MemoryRepository,
        exploration: ExplorationOrchestrator,
    ) -> Self {
        let mut state = Self::new(bootstrap, settings, memory);
        state.exploration = Some(exploration);
        state
    }

    pub fn with_services(
        bootstrap: BootstrapState,
        settings: SettingsService,
        memory: MemoryRepository,
        chat: ChatService,
        exploration: ExplorationOrchestrator,
    ) -> Self {
        let mut state = Self::with_exploration(bootstrap, settings, memory, exploration);
        state.chat = Some(chat);
        state
    }
}
