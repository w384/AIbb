mod context;
mod repository;

pub use context::{last_non_empty_paragraph, ContextBuilder};
pub use repository::MemoryRepository;

/// Conversation channel for the ordinary AIbb chat (plus outing diaries).
pub const CHAT_CHANNEL: &str = "chat";
/// Conversation channel for the vocabulary assistant (术语词汇中台).
/// Entries live in their own channel so terms never pollute the chat context.
pub const VOCAB_CHANNEL: &str = "vocab";

/// 上下文窗口显示上限：模型每次注入某个通道最近这么多字符（128K），
/// AIbb 对话与词汇对话共用。
pub const CONTEXT_CHAR_BUDGET: usize = 128 * 1024;
/// 文档备份量：数据库为每个通道保留 10 倍于上下文上限的最近内容（≈1.28M 字符），
/// 超出部分在写入时直接删除（超出的删除）。
pub const STORAGE_CHAR_LIMIT: usize = 10 * CONTEXT_CHAR_BUDGET;
