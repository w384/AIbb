use crate::{
    domain::{MemoryContext, SummaryCandidate},
    error::AppError,
};

use super::{MemoryRepository, CHAT_CHANNEL, VOCAB_CHANNEL};

/// How many recent messages to inject for the ordinary chat: keep it thin so
/// the model leans on its own thinking.
const CHAT_RECENT_MESSAGES: usize = 8;
/// The vocabulary assistant maintains its virtual glossary (V 编号、重复计数、
/// 20 条阈值) inside the conversation, so it needs a wide enough window to see
/// the entries it must count (≈ 40 独立词条).
const VOCAB_RECENT_MESSAGES: usize = 80;

pub struct ContextBuilder {
    repository: MemoryRepository,
}

impl ContextBuilder {
    pub fn new(repository: MemoryRepository) -> Self {
        Self { repository }
    }

    pub async fn build(&self, current_input: impl Into<String>) -> Result<MemoryContext, AppError> {
        self.build_in(CHAT_CHANNEL, current_input).await
    }

    /// Builds a conversation context scoped to one memory channel (`chat` or
    /// `vocab`); the vocabulary channel never carries a summary.
    pub async fn build_in(
        &self,
        channel: &str,
        current_input: impl Into<String>,
    ) -> Result<MemoryContext, AppError> {
        // Keep the injected conversation thin for ordinary chat; the
        // vocabulary channel gets a wide window so the model can actually
        // count and number the entries it maintains.
        let recent_limit = if channel == VOCAB_CHANNEL {
            VOCAB_RECENT_MESSAGES
        } else {
            CHAT_RECENT_MESSAGES
        };
        let snapshot = self.repository.context_snapshot_in(channel, recent_limit).await?;
        let last_assistant_paragraph = snapshot
            .newest_assistant_message
            .and_then(|message| last_non_empty_paragraph(&message.content));

        Ok(MemoryContext {
            current_input: current_input.into(),
            last_assistant_paragraph,
            recent_messages: snapshot.recent_messages,
            summary: snapshot.summary,
        })
    }

    pub async fn summary_candidate(&self) -> Result<Option<SummaryCandidate>, AppError> {
        self.summary_candidate_in(CHAT_CHANNEL).await
    }

    pub async fn summary_candidate_in(
        &self,
        channel: &str,
    ) -> Result<Option<SummaryCandidate>, AppError> {
        let messages = self
            .repository
            .unsummarized_before_recent_window_in(channel, 40)
            .await?;
        let total_characters = messages
            .iter()
            .map(|message| message.content.chars().count())
            .sum();

        if total_characters <= 12_000 {
            return Ok(None);
        }

        let through_message_created_at = messages
            .last()
            .map(|message| message.created_at)
            .ok_or_else(context_error)?;

        Ok(Some(SummaryCandidate {
            messages,
            total_characters,
            through_message_created_at,
        }))
    }
}

pub fn last_non_empty_paragraph(content: &str) -> Option<String> {
    let mut last_paragraph = None;
    let mut current_lines = Vec::new();

    for line in content.lines() {
        if line.trim().is_empty() {
            if !current_lines.is_empty() {
                last_paragraph = Some(current_lines.join("\n").trim().to_string());
                current_lines.clear();
            }
        } else {
            current_lines.push(line);
        }
    }

    if !current_lines.is_empty() {
        last_paragraph = Some(current_lines.join("\n").trim().to_string());
    }

    last_paragraph
}

fn context_error() -> AppError {
    AppError::new(
        "invalidMemoryContext",
        "Conversation context could not be built.",
    )
}
