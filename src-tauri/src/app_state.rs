use std::sync::RwLock;

use crate::domain::BootstrapState;

pub struct AppState {
    pub bootstrap: RwLock<BootstrapState>,
    pub pet_position: RwLock<Option<(i32, i32)>>,
}
