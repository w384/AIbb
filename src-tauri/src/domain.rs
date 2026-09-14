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
pub struct AibbProfile {
    pub name: String,
    pub avatar_data_url: Option<String>,
    pub version: i64,
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum WebMode {
    Auto,
    Force,
    Off,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
    System,
}

impl Role {
    pub fn as_storage_value(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::System => "system",
        }
    }

    pub fn from_storage_value(value: &str) -> Option<Self> {
        match value {
            "user" => Some(Self::User),
            "assistant" => Some(Self::Assistant),
            "system" => Some(Self::System),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub id: String,
    pub role: Role,
    pub content: String,
    pub created_at: i64,
    pub summarized_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MemoryContext {
    pub current_input: String,
    pub last_assistant_paragraph: Option<String>,
    pub recent_messages: Vec<Message>,
    pub summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SummaryCandidate {
    pub messages: Vec<Message>,
    pub total_characters: usize,
    pub through_message_created_at: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WebMaterial {
    pub pages: Vec<WebPageMaterial>,
}

impl WebMaterial {
    pub fn empty() -> Self {
        Self::default()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OutingSource {
    pub title: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WebPageMaterial {
    pub source: OutingSource,
    pub text: String,
}

impl WebPageMaterial {
    pub(crate) fn from_untrusted(title: &str, raw_url: &str, text: &str) -> Option<Self> {
        let source = OutingSource::from_untrusted(title, raw_url)?;
        Some(Self {
            source,
            text: text.chars().take(6_000).collect(),
        })
    }
}

impl OutingSource {
    pub(crate) fn from_untrusted(title: &str, raw_url: &str) -> Option<Self> {
        let title = title.trim();
        if title.is_empty()
            || title.chars().any(char::is_control)
            || title.chars().count() > 200
            || raw_url.len() > 2_048
        {
            return None;
        }

        let mut url = url::Url::parse(raw_url).ok()?;
        if url.scheme() != "https"
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return None;
        }
        url.set_fragment(None);

        Some(Self {
            title: title.to_string(),
            url: url.to_string(),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExplorationResult {
    pub items: [String; 4],
    pub diary: String,
    pub sources: Vec<OutingSource>,
    /// Interesting pictures AIbb brought back: the picture itself is embedded
    /// as a data URL so the renderer never needs to contact the origin host.
    pub images: Vec<ExplorationImage>,
    pub round_number: u64,
    pub elapsed_seconds: u64,
    pub raw_response: String,
    /// The model decides where inside the diary a source link or a picture
    /// should be shown, so the outing reads like a live share rather than a
    /// wall of text. Empty when the diary never asked for any.
    #[serde(default)]
    pub highlights: Vec<DiaryHighlight>,
    /// The four divergent angles the model planned before writing, one short
    /// title per diary section (in order). Empty when the model did not plan
    /// them (older data / fallback), which the renderer treats as no label.
    #[serde(default)]
    pub sections: Vec<String>,
}

/// One spot inside the diary where AIbb points the reader at something she
/// actually saw: `paragraph` is the zero-based paragraph index (paragraphs
/// are split on blank lines), and at least one of `sourceIndex` / `imageIndex`
/// is present, referencing `ExplorationResult.sources` / `.images`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DiaryHighlight {
    pub paragraph: usize,
    #[serde(default)]
    pub source_index: Option<usize>,
    #[serde(default)]
    pub image_index: Option<usize>,
}

/// Parsed outing diary: the free-text body, the model-chosen highlights, and
/// the four pre-planned section angles (titles) that gave the body its shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutingDiary {
    pub text: String,
    pub highlights: Vec<DiaryHighlight>,
    pub sections: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExplorationImage {
    pub title: String,
    pub page_url: String,
    pub data_url: String,
}

/// A finished outing as returned by `load_chat_history`, so the renderer can
/// rebuild the same diary card (sources and pictures included) after restart.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CompletedOuting {
    pub round_number: u64,
    pub direction: Option<String>,
    pub diary: String,
    pub sources: Vec<OutingSource>,
    pub images: Vec<ExplorationImage>,
    pub elapsed_seconds: u64,
    pub created_at: i64,
    /// The four planned section angles, restored so the diary card can show
    /// the same direction chips after a restart. Empty for older outings.
    #[serde(default)]
    pub sections: Vec<String>,
}

/// How many finished outings went to one direction. The raw user wording is
/// kept so a future heat map can group by place.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DirectionCount {
    pub direction: String,
    pub count: u64,
}

/// Collection-style outing statistics shown in the chat window: total trips,
/// distinct directions and the per-direction counts that will later feed a
/// play-heat map.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OutingStats {
    pub total_outings: u64,
    pub total_directions: u64,
    pub last_outing_at: Option<i64>,
    pub directions: Vec<DirectionCount>,
}

impl WebMode {
    pub fn as_storage_value(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Force => "force",
            Self::Off => "off",
        }
    }

    pub fn from_storage_value(value: &str) -> Option<Self> {
        match value {
            "auto" => Some(Self::Auto),
            "force" => Some(Self::Force),
            "off" => Some(Self::Off),
            _ => None,
        }
    }
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
