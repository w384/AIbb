use crate::{
    domain::{MemoryContext, SummaryCandidate},
    error::AppError,
};

use super::{MemoryRepository, CHAT_CHANNEL, CONTEXT_CHAR_BUDGET};

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
        // 上下文窗口上限：两个通道都注入最近的 128K 字符（显示上限），
        // 普通聊天额外叠加旧摘要，词汇通道保持薄层直注。
        let snapshot = self
            .repository
            .context_snapshot_by_chars(channel, CONTEXT_CHAR_BUDGET)
            .await?;
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
        // 与上下文窗口保持一致：窗口内的消息不参与摘要，窗口之前的旧内容
        // 超过阈值才触发压缩。
        let snapshot = self
            .repository
            .context_snapshot_by_chars(channel, CONTEXT_CHAR_BUDGET)
            .await?;
        let recent_count = snapshot.recent_messages.len();
        let messages = self
            .repository
            .unsummarized_before_recent_window_in(channel, recent_count)
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
