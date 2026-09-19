mod context;
mod repository;

pub use context::{last_non_empty_paragraph, ContextBuilder};
pub use repository::MemoryRepository;

/// Conversation channel for the ordinary AIbb chat (plus outing diaries).
pub const CHAT_CHANNEL: &str = "chat";
/// Conversation channel for the vocabulary assistant (术语词汇中台).
/// Entries live in their own channel so terms never pollute the chat context.
pub const VOCAB_CHANNEL: &str = "vocab";
