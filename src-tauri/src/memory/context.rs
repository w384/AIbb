use crate::{
    domain::{MemoryContext, SummaryCandidate},
    error::AppError,
};

use super::MemoryRepository;

pub struct ContextBuilder {
    repository: MemoryRepository,
}

impl ContextBuilder {
    pub fn new(repository: MemoryRepository) -> Self {
        Self { repository }
    }

    pub async fn build(&self, current_input: impl Into<String>) -> Result<MemoryContext, AppError> {
        let snapshot = self.repository.context_snapshot(40).await?;
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
        let messages = self
            .repository
            .unsummarized_before_recent_window(40)
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
