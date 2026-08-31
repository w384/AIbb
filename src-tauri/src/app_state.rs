use std::sync::RwLock;

use crate::domain::BootstrapState;
use crate::memory::MemoryRepository;
use crate::settings::SettingsService;

pub struct AppState {
    pub bootstrap: RwLock<BootstrapState>,
    pub settings: SettingsService,
    pub memory: MemoryRepository,
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
        }
    }
}
