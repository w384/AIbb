use std::sync::RwLock;

use crate::domain::BootstrapState;
use crate::settings::SettingsService;

pub struct AppState {
    pub bootstrap: RwLock<BootstrapState>,
    pub settings: SettingsService,
}

impl AppState {
    pub fn new(bootstrap: BootstrapState, settings: SettingsService) -> Self {
        Self {
            bootstrap: RwLock::new(bootstrap),
            settings,
        }
    }
}
