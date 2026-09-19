use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::Serialize;
use tauri::Manager;
use tokio_util::sync::CancellationToken;

use crate::{
    app_state::AppState,
    domain::Message,
    error::{sanitize_sensitive_text, AppError},
    llm::{ChatMessage, ChatRequest},
    memory::VOCAB_CHANNEL,
    prompts::VOCAB_EXPORT_INSTRUCTION,
};

use super::chat::{ChatRuntimeFactory, VocabChatRuntimeFactory};

const DEFAULT_HISTORY_MESSAGES: usize = 50;
const EXPORT_PREVIEW_CHARACTERS: usize = 2_000;

/// Starts a vocabulary-assistant reply. Unlike `submit_user_input`, this never
/// routes through outing-intent detection: an English term like `agent` or
/// `tool` must not trigger an exploration.
#[tauri::command]
pub async fn submit_vocab_input(
    state: tauri::State<'_, AppState>,
    message: String,
    request_id: String,
) -> Result<(), AppError> {
    state
        .vocab
        .as_ref()
        .ok_or_else(vocab_service_error)?
        .start(message, request_id)
        .await
}

/// Recent messages from the vocabulary channel only.
#[tauri::command]
pub async fn load_vocab_history(
    state: tauri::State<'_, AppState>,
    limit: Option<usize>,
) -> Result<Vec<Message>, AppError> {
    state
        .memory
        .recent_messages_in(
            VOCAB_CHANNEL,
            limit.unwrap_or(DEFAULT_HISTORY_MESSAGES),
        )
        .await
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct VocabExportResult {
    /// Absolute path of the written markdown file.
    pub file_path: String,
    /// Leading slice of the organized table for the renderer preview.
    pub preview: String,
}

/// 「导出词表」: hands the vocabulary conversation to the model to tidy it into
/// a deduplicated markdown table, then writes the result under the app data
/// directory's `vocab-export` folder. The conversation itself is unchanged.
#[tauri::command]
pub async fn export_vocab_glossary(
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<VocabExportResult, AppError> {
    let messages = state
        .memory
        .recent_messages_in(VOCAB_CHANNEL, 120)
        .await?;
    if messages.is_empty() {
        return Err(AppError::new(
            "emptyVocabGlossary",
            "还没有可导出的词汇。先在词汇助手对话里收录一些术语吧。",
        ));
    }

    let runtime = VocabChatRuntimeFactory::new(state.settings.clone())
        .create()
        .await?;
    let conversation = messages
        .iter()
        .map(|message| format!("{}: {}", message.role.as_storage_value(), message.content))
        .collect::<Vec<_>>()
        .join("\n");
    let request = ChatRequest {
        messages: vec![
            ChatMessage::new("system", VOCAB_EXPORT_INSTRUCTION),
            ChatMessage::user(conversation),
        ],
    };
    let table = runtime
        .llm
        .complete(request, CancellationToken::new())
        .await?;
    let table = sanitize_sensitive_text(&table, runtime.exact_key.as_deref()).trim().to_string();
    if table.is_empty() {
        return Err(AppError::new(
            "invalidVocabExport",
            "词表整理结果为空，请稍后重试。",
        ));
    }

    let directory = export_directory(&app)?;
    let timestamp = unix_milliseconds()?;
    let file_path = directory.join(format!("词汇表-{timestamp}.md"));
    fs::write(&file_path, table.clone() + "\n").map_err(|_| export_write_error())?;

    Ok(VocabExportResult {
        file_path: file_path.to_string_lossy().into_owned(),
        preview: table.chars().take(EXPORT_PREVIEW_CHARACTERS).collect(),
    })
}

fn export_directory(app: &tauri::AppHandle) -> Result<PathBuf, AppError> {
    let directory = app
        .path()
        .app_data_dir()
        .map_err(|_| export_write_error())?
        .join("vocab-export");
    fs::create_dir_all(&directory).map_err(|_| export_write_error())?;
    Ok(directory)
}

fn unix_milliseconds() -> Result<i64, AppError> {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| export_write_error())?;
    i64::try_from(elapsed.as_millis()).map_err(|_| export_write_error())
}

fn vocab_service_error() -> AppError {
    AppError::new("vocabServiceUnavailable", "The vocabulary service is unavailable.")
}

fn export_write_error() -> AppError {
    AppError::new("vocabExportFailed", "The glossary export could not be written.")
}
