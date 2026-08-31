mod context;
mod repository;

pub use context::{last_non_empty_paragraph, ContextBuilder};
pub use repository::MemoryRepository;
