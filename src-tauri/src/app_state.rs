use std::sync::RwLock;

use crate::domain::BootstrapState;

pub struct AppState {
    pub bootstrap: RwLock<BootstrapState>,
}
