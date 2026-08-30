use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapState {
    pub first_run: bool,
    pub pet_status: PetStatus,
    pub api_configured: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PetStatus {
    Idle,
    Chatting,
    Exploring,
    Returned,
    Error,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bootstrap_state_serializes_with_renderer_field_names() {
        let state = BootstrapState {
            first_run: true,
            pet_status: PetStatus::Idle,
            api_configured: false,
        };

        assert_eq!(
            serde_json::to_value(state).unwrap(),
            serde_json::json!({
                "firstRun": true,
                "petStatus": "idle",
                "apiConfigured": false
            })
        );
    }
}
