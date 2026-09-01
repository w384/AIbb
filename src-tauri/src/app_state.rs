use std::sync::RwLock;

use crate::domain::BootstrapState;
use crate::exploration::ExplorationOrchestrator;
use crate::memory::MemoryRepository;
use crate::settings::SettingsService;

pub struct AppState {
    pub bootstrap: RwLock<BootstrapState>,
    pub settings: SettingsService,
    pub memory: MemoryRepository,
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
}
