use std::{
    path::Path,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{SystemTime, UNIX_EPOCH},
};

use rusqlite::params;
use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard};

use crate::{
    domain::{Message, Role, SummaryCandidate},
    error::AppError,
    storage::Database,
};

use super::{CHAT_CHANNEL, VOCAB_CHANNEL};

#[derive(Clone)]
pub struct MemoryRepository {
    database: Database,
    operation: Arc<AsyncMutex<()>>,
    completion_boundary: Arc<AsyncMutex<()>>,
    generation: Arc<AtomicU64>,
}

pub(super) struct ContextSnapshot {
    pub recent_messages: Vec<Message>,
    pub newest_assistant_message: Option<Message>,
    pub summary: Option<String>,
}

impl MemoryRepository {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, AppError> {
        Ok(Self::new(Database::open(path)?))
    }

    pub fn new(database: Database) -> Self {
        Self {
            database,
            operation: Arc::new(AsyncMutex::new(())),
            completion_boundary: Arc::new(AsyncMutex::new(())),
            generation: Arc::new(AtomicU64::new(0)),
        }
    }

    pub(crate) fn generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }

    pub(crate) async fn lock_completion_boundary(&self) -> OwnedMutexGuard<()> {
        self.completion_boundary.clone().lock_owned().await
    }

    /// Appends to the ordinary chat channel.
    pub async fn append(
        &self,
        role: Role,
        content: impl Into<String>,
    ) -> Result<Message, AppError> {
        self.append_in(CHAT_CHANNEL, role, content).await
    }

    /// Appends to an explicit channel (`chat` or `vocab`).
    pub async fn append_in(
        &self,
        channel: &str,
        role: Role,
        content: impl Into<String>,
    ) -> Result<Message, AppError> {
        let _operation = self.operation.lock().await;
        self.append_locked(channel, role, content.into())
    }

    pub(crate) async fn append_if_generation_in(
        &self,
        channel: &str,
        expected_generation: u64,
        role: Role,
        content: impl Into<String>,
    ) -> Result<Option<Message>, AppError> {
        let _operation = self.operation.lock().await;
        if self.generation.load(Ordering::SeqCst) != expected_generation {
            return Ok(None);
        }
        self.append_locked(channel, role, content.into()).map(Some)
    }

    fn append_locked(&self, channel: &str, role: Role, content: String) -> Result<Message, AppError> {
        let connection = self.database.connection()?;
        let now = unix_milliseconds()?;
        let latest = connection
            .query_row(
                "SELECT COALESCE(MAX(created_at), 0) FROM messages",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map_err(|_| memory_error())?;
        let created_at = now.max(latest.saturating_add(1));
        let id = format!("message-{created_at}");

        connection
            .execute(
                "INSERT INTO messages(id, role, content, created_at, channel) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![id, role.as_storage_value(), content, created_at, channel],
            )
            .map_err(|_| memory_error())?;

        Ok(Message {
            id,
            role,
            content,
            created_at,
            summarized_at: None,
        })
    }

    pub(super) async fn context_snapshot_in(
        &self,
        channel: &str,
        recent_limit: usize,
    ) -> Result<ContextSnapshot, AppError> {
        let _operation = self.operation.lock().await;
        let connection = self.database.connection()?;

        Ok(ContextSnapshot {
            recent_messages: recent_messages_from_connection(&connection, channel, recent_limit)?,
            newest_assistant_message: newest_assistant_from_connection(&connection, channel)?,
            summary: newest_summary_from_connection(&connection, channel, recent_limit)?,
        })
    }

    /// Recent messages from the ordinary chat channel.
    pub async fn recent_messages(&self, limit: usize) -> Result<Vec<Message>, AppError> {
        self.recent_messages_in(CHAT_CHANNEL, limit).await
    }

    /// Recent messages from an explicit channel (`chat` or `vocab`).
    pub async fn recent_messages_in(
        &self,
        channel: &str,
        limit: usize,
    ) -> Result<Vec<Message>, AppError> {
        let _operation = self.operation.lock().await;
        let connection = self.database.connection()?;
        recent_messages_from_connection(&connection, channel, limit)
    }

    pub(crate) async fn unsummarized_before_recent_window_in(
        &self,
        channel: &str,
        recent_limit: usize,
    ) -> Result<Vec<Message>, AppError> {
        let _operation = self.operation.lock().await;
        let connection = self.database.connection()?;
        let recent_offset =
            i64::try_from(recent_limit.saturating_sub(1)).map_err(|_| memory_error())?;
        let mut statement = connection
            .prepare(
                "SELECT id, role, content, created_at, summarized_at FROM messages \
                 WHERE summarized_at IS NULL AND channel = ?1 AND created_at < ( \
                   SELECT created_at FROM messages \
                   WHERE channel = ?1 \
                   ORDER BY created_at DESC, rowid DESC LIMIT 1 OFFSET ?2 \
                 ) \
                 ORDER BY created_at ASC, rowid ASC",
            )
            .map_err(|_| memory_error())?;
        let mut rows = statement
            .query(params![channel, recent_offset])
            .map_err(|_| memory_error())?;
        let mut messages = Vec::new();

        while let Some(row) = rows.next().map_err(|_| memory_error())? {
            messages.push(message_from_row(row)?);
        }
        Ok(messages)
    }

    pub async fn save_summary(
        &self,
        candidate: &SummaryCandidate,
        content: impl Into<String>,
    ) -> Result<(), AppError> {
        let _operation = self.operation.lock().await;
        let Some(last_message) = candidate.messages.last() else {
            return Err(memory_error());
        };
        if last_message.created_at != candidate.through_message_created_at {
            return Err(memory_error());
        }

        let content = content.into();
        let mut connection = self.database.connection()?;
        let transaction = connection.transaction().map_err(|_| memory_error())?;
        let now = unix_milliseconds()?;
        let latest = transaction
            .query_row(
                "SELECT COALESCE(MAX(created_at), 0) FROM memory_summaries",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map_err(|_| memory_error())?;
        let created_at = now.max(latest.saturating_add(1));
        let id = format!("summary-{created_at}");
        transaction
            .execute(
                "INSERT INTO memory_summaries( \
                   id, content, through_message_created_at, created_at \
                 ) VALUES (?1, ?2, ?3, ?4)",
                params![
                    id,
                    content,
                    candidate.through_message_created_at,
                    created_at
                ],
            )
            .map_err(|_| memory_error())?;

        for message in &candidate.messages {
            let changed = transaction
                .execute(
                    "UPDATE messages SET summarized_at = ?1 \
                     WHERE id = ?2 AND summarized_at IS NULL",
                    params![created_at, message.id],
                )
                .map_err(|_| memory_error())?;
            if changed != 1 {
                return Err(memory_error());
            }
        }

        transaction.commit().map_err(|_| memory_error())
    }

    /// Clears the ordinary chat channel, its summaries and all outing records
    /// (the vocabulary channel is untouched — it has its own clear entry).
    pub async fn clear_memory(&self) -> Result<(), AppError> {
        self.clear_channel(CHAT_CHANNEL, true).await
    }

    /// Clears only the vocabulary channel, leaving the chat and outings intact.
    pub async fn clear_vocab_memory(&self) -> Result<(), AppError> {
        self.clear_channel(VOCAB_CHANNEL, false).await
    }

    async fn clear_channel(&self, channel: &str, clear_chat_extras: bool) -> Result<(), AppError> {
        let _completion_boundary = self.completion_boundary.clone().lock_owned().await;
        let _operation = self.operation.lock().await;
        let mut connection = self.database.connection()?;
        let transaction = connection.transaction().map_err(|_| memory_error())?;

        transaction
            .execute(
                "DELETE FROM messages WHERE channel = ?1",
                params![channel],
            )
            .map_err(|_| memory_error())?;
        if clear_chat_extras {
            transaction
                .execute("DELETE FROM memory_summaries", [])
                .map_err(|_| memory_error())?;
            transaction
                .execute("DELETE FROM explorations", [])
                .map_err(|_| memory_error())?;
        }

        transaction.commit().map_err(|_| memory_error())?;
        self.generation.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

fn recent_messages_from_connection(
    connection: &rusqlite::Connection,
    channel: &str,
    limit: usize,
) -> Result<Vec<Message>, AppError> {
    let mut statement = connection
        .prepare(
            "SELECT id, role, content, created_at, summarized_at FROM messages \
             WHERE channel = ?1 \
             ORDER BY created_at DESC, rowid DESC LIMIT ?2",
        )
        .map_err(|_| memory_error())?;
    let mut rows = statement
        .query(params![channel, i64::try_from(limit).map_err(|_| memory_error())?])
        .map_err(|_| memory_error())?;
    let mut messages = Vec::with_capacity(limit);

    while let Some(row) = rows.next().map_err(|_| memory_error())? {
        messages.push(message_from_row(row)?);
    }
    messages.reverse();
    Ok(messages)
}

fn newest_assistant_from_connection(
    connection: &rusqlite::Connection,
    channel: &str,
) -> Result<Option<Message>, AppError> {
    let mut statement = connection
        .prepare(
            "SELECT id, role, content, created_at, summarized_at FROM messages \
             WHERE role = 'assistant' AND channel = ?1 \
             ORDER BY created_at DESC, rowid DESC LIMIT 1",
        )
        .map_err(|_| memory_error())?;
    let mut rows = statement.query(params![channel]).map_err(|_| memory_error())?;

    rows.next()
        .map_err(|_| memory_error())?
        .map(message_from_row)
        .transpose()
}

fn newest_summary_from_connection(
    connection: &rusqlite::Connection,
    channel: &str,
    recent_limit: usize,
) -> Result<Option<String>, AppError> {
    let recent_offset =
        i64::try_from(recent_limit.saturating_sub(1)).map_err(|_| memory_error())?;
    let mut statement = connection
        .prepare(
            "SELECT content FROM memory_summaries \
             WHERE through_message_created_at < ( \
               SELECT created_at FROM messages \
               WHERE channel = ?1 \
               ORDER BY created_at DESC, rowid DESC LIMIT 1 OFFSET ?2 \
             ) \
             ORDER BY created_at DESC, rowid DESC LIMIT 1",
        )
        .map_err(|_| memory_error())?;
    let mut rows = statement
        .query(params![channel, recent_offset])
        .map_err(|_| memory_error())?;

    rows.next()
        .map_err(|_| memory_error())?
        .map(|row| row.get::<_, String>(0).map_err(|_| memory_error()))
        .transpose()
}

fn message_from_row(row: &rusqlite::Row<'_>) -> Result<Message, AppError> {
    let role_value = row.get::<_, String>(1).map_err(|_| memory_error())?;
    let role = Role::from_storage_value(&role_value).ok_or_else(memory_error)?;

    Ok(Message {
        id: row.get(0).map_err(|_| memory_error())?,
        role,
        content: row.get(2).map_err(|_| memory_error())?,
        created_at: row.get(3).map_err(|_| memory_error())?,
        summarized_at: row.get(4).map_err(|_| memory_error())?,
    })
}

fn unix_milliseconds() -> Result<i64, AppError> {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| memory_error())?;
    i64::try_from(elapsed.as_millis()).map_err(|_| memory_error())
}

fn memory_error() -> AppError {
    AppError::new(
        "storageUnavailable",
        "Conversation memory could not be accessed.",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn chat_and_vocab_channels_are_isolated() {
        let directory = tempfile::tempdir().unwrap();
        let repository = MemoryRepository::open(directory.path().join("aibb.sqlite3")).unwrap();

        repository.append(Role::User, "普通聊天").await.unwrap();
        repository
            .append_in(VOCAB_CHANNEL, Role::User, "agent 术语")
            .await
            .unwrap();
        repository
            .append_in(VOCAB_CHANNEL, Role::Assistant, "# agent 词条")
            .await
            .unwrap();

        let chat = repository.recent_messages(10).await.unwrap();
        let vocab = repository.recent_messages_in(VOCAB_CHANNEL, 10).await.unwrap();
        assert_eq!(chat.len(), 1);
        assert_eq!(chat[0].content, "普通聊天");
        assert_eq!(vocab.len(), 2);
        assert_eq!(vocab[0].content, "agent 术语");
        assert_eq!(vocab[1].content, "# agent 词条");

        // 分开清除：只清词汇通道，普通聊天保留。
        repository.clear_vocab_memory().await.unwrap();
        assert!(repository
            .recent_messages_in(VOCAB_CHANNEL, 10)
            .await
            .unwrap()
            .is_empty());
        assert_eq!(repository.recent_messages(10).await.unwrap().len(), 1);

        repository.clear_memory().await.unwrap();
        assert!(repository.recent_messages(10).await.unwrap().is_empty());
    }
}
