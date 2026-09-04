use std::sync::{Arc, Mutex};

use aibb_desktop_pet_lib::{
    commands::chat::{ChatEvent, ChatEventSink, ChatService},
    error::{AppError, ErrorCode},
    llm::{ChatRequest, DeltaSink, LlmTransport, NativeWebOutcome, NativeWebRequest},
    memory::{ContextBuilder, MemoryRepository},
    storage::Database,
};
use async_trait::async_trait;
use tempfile::TempDir;
use tokio::sync::{Notify, Semaphore};
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
struct FakeLlm {
    chunks: Arc<Vec<String>>,
    completion: Arc<Mutex<Result<String, ErrorCode>>>,
}

impl FakeLlm {
    fn new(chunks: &[&str], completion: Result<&str, ErrorCode>) -> Self {
        Self {
            chunks: Arc::new(chunks.iter().map(|chunk| (*chunk).to_string()).collect()),
            completion: Arc::new(Mutex::new(completion.map(str::to_string))),
        }
    }
}

#[async_trait]
impl LlmTransport for FakeLlm {
    async fn stream_chat(
        &self,
        _request: ChatRequest,
        sink: &dyn DeltaSink,
        _cancellation: CancellationToken,
    ) -> Result<(), AppError> {
        for chunk in self.chunks.iter() {
            sink.send(chunk).await?;
        }
        Ok(())
    }

    async fn complete(
        &self,
        _request: ChatRequest,
        _cancellation: CancellationToken,
    ) -> Result<String, AppError> {
        self.completion
            .lock()
            .unwrap()
            .clone()
            .map_err(AppError::from_code)
    }

    async fn try_native_web(
        &self,
        _request: NativeWebRequest,
        _cancellation: CancellationToken,
    ) -> Result<NativeWebOutcome, AppError> {
        panic!("chat must not request web access")
    }

    async fn test_connection(&self, _cancellation: CancellationToken) -> Result<(), AppError> {
        panic!("chat must not run a connection test")
    }
}

#[derive(Clone)]
struct DelayedLlm {
    started: Arc<Notify>,
    release: Arc<Semaphore>,
}

impl DelayedLlm {
    fn new() -> Self {
        Self {
            started: Arc::new(Notify::new()),
            release: Arc::new(Semaphore::new(0)),
        }
    }

    async fn wait_until_started(&self) {
        self.started.notified().await;
    }

    fn release(&self) {
        self.release.add_permits(1);
    }
}

#[async_trait]
impl LlmTransport for DelayedLlm {
    async fn stream_chat(
        &self,
        _request: ChatRequest,
        sink: &dyn DeltaSink,
        _cancellation: CancellationToken,
    ) -> Result<(), AppError> {
        self.started.notify_one();
        let permit = self.release.acquire().await.unwrap();
        permit.forget();
        sink.send("迟到的回复").await
    }

    async fn complete(
        &self,
        _request: ChatRequest,
        _cancellation: CancellationToken,
    ) -> Result<String, AppError> {
        Ok("unused".into())
    }

    async fn try_native_web(
        &self,
        _request: NativeWebRequest,
        _cancellation: CancellationToken,
    ) -> Result<NativeWebOutcome, AppError> {
        panic!("chat must not request web access")
    }

    async fn test_connection(&self, _cancellation: CancellationToken) -> Result<(), AppError> {
        panic!("chat must not run a connection test")
    }
}

#[derive(Clone)]
struct RecordingEvents {
    memory: MemoryRepository,
    events: Arc<Mutex<Vec<ChatEvent>>>,
}

impl RecordingEvents {
    fn new(memory: MemoryRepository) -> Self {
        Self {
            memory,
            events: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn snapshot(&self) -> Vec<ChatEvent> {
        self.events.lock().unwrap().clone()
    }
}

#[async_trait]
impl ChatEventSink for RecordingEvents {
    async fn emit(&self, event: ChatEvent) -> Result<(), AppError> {
        if let ChatEvent::Complete { message, .. } = &event {
            let messages = self.memory.recent_messages(10).await?;
            assert!(
                messages
                    .iter()
                    .any(|persisted| persisted.content == *message),
                "the assistant reply must be persisted before the terminal event"
            );
        }
        self.events.lock().unwrap().push(event);
        Ok(())
    }
}

fn memory() -> (TempDir, MemoryRepository) {
    let directory = tempfile::tempdir().unwrap();
    let database = Database::open(directory.path().join("aibb.sqlite3")).unwrap();
    (directory, MemoryRepository::new(database))
}

#[tokio::test]
async fn clearing_memory_while_chat_is_in_flight_prevents_late_reply_repopulation() {
    let (_directory, memory) = memory();
    let events = RecordingEvents::new(memory.clone());
    let llm = Arc::new(DelayedLlm::new());
    let service = ChatService::new(
        memory.clone(),
        llm.clone(),
        Arc::new(events.clone()),
        Some("sk-secret".to_string()),
    );
    let chat = tokio::spawn(async move {
        service
            .run("清空前的问题".to_string(), "request-clear-race".to_string())
            .await
    });
    llm.wait_until_started().await;

    memory.clear_memory().await.unwrap();
    llm.release();
    let error = chat.await.unwrap().unwrap_err();

    assert_eq!(error.code, "cancelled");
    assert!(memory.recent_messages(10).await.unwrap().is_empty());
    let recorded = events.snapshot();
    assert!(recorded.iter().any(|event| matches!(
        event,
        ChatEvent::Delta { request_id, delta }
            if request_id == "request-clear-race" && delta == "迟到的回复"
    )));
    assert!(recorded.iter().any(|event| matches!(
        event,
        ChatEvent::Error { request_id, code, .. }
            if request_id == "request-clear-race" && code == "cancelled"
    )));
    assert!(!recorded.iter().any(|event| matches!(
        event,
        ChatEvent::Complete { request_id, .. } if request_id == "request-clear-race"
    )));
}

#[tokio::test]
async fn split_secrets_are_redacted_from_deltas_completion_and_persistence() {
    let (_directory, memory) = memory();
    let events = RecordingEvents::new(memory.clone());
    let llm = Arc::new(FakeLlm::new(
        &[
            "你好 sk-sec",
            "ret\nAuthoriza",
            "tion: Bearer old-secret\n完成",
        ],
        Ok("unused"),
    ));
    let service = ChatService::new(
        memory.clone(),
        llm,
        Arc::new(events.clone()),
        Some("sk-secret".to_string()),
    );

    service
        .run("聊聊 sk-secret".to_string(), "request-1".to_string())
        .await
        .unwrap();

    let recorded = events.snapshot();
    let serialized = serde_json::to_string(&recorded).unwrap();
    assert!(!serialized.contains("sk-secret"));
    assert!(!serialized.contains("old-secret"));
    assert!(recorded.iter().any(|event| matches!(
        event,
        ChatEvent::Delta { request_id, .. } if request_id == "request-1"
    )));
    assert!(recorded.iter().any(|event| matches!(
        event,
        ChatEvent::Complete { request_id, message }
            if request_id == "request-1"
                && message.contains("[REDACTED]")
                && message.contains("完成")
    )));

    let persisted = memory.recent_messages(10).await.unwrap();
    assert_eq!(persisted.len(), 2);
    assert_eq!(persisted[0].content, "聊聊 [REDACTED]");
    assert!(!persisted[1].content.contains("sk-secret"));
    assert!(!persisted[1].content.contains("old-secret"));
}

#[tokio::test]
async fn summary_failure_is_non_fatal_and_does_not_erase_the_reply() {
    let (_directory, memory) = memory();
    for index in 0..81 {
        memory
            .append(
                aibb_desktop_pet_lib::domain::Role::User,
                format!("old-{index}-{}", "x".repeat(300)),
            )
            .await
            .unwrap();
    }
    let events = RecordingEvents::new(memory.clone());
    let service = ChatService::new(
        memory.clone(),
        Arc::new(FakeLlm::new(
            &["回复仍然存在"],
            Err(ErrorCode::ProviderUnavailable),
        )),
        Arc::new(events.clone()),
        Some("sk-secret".to_string()),
    );

    service
        .run("新问题".to_string(), "request-2".to_string())
        .await
        .unwrap();

    assert!(events.snapshot().iter().any(|event| matches!(
        event,
        ChatEvent::Complete { message, .. } if message == "回复仍然存在"
    )));
    let persisted = memory.recent_messages(2).await.unwrap();
    assert_eq!(persisted[0].content, "新问题");
    assert_eq!(persisted[1].content, "回复仍然存在");
    let context = ContextBuilder::new(memory).build("next").await.unwrap();
    assert_eq!(context.summary, None);
}

#[tokio::test]
async fn successful_summary_uses_the_same_task_key_and_persists_only_sanitized_text() {
    let (_directory, memory) = memory();
    for index in 0..81 {
        memory
            .append(
                aibb_desktop_pet_lib::domain::Role::User,
                format!("old-{index}-{}", "x".repeat(300)),
            )
            .await
            .unwrap();
    }
    let events = RecordingEvents::new(memory.clone());
    let service = ChatService::new(
        memory.clone(),
        Arc::new(FakeLlm::new(
            &["新回复"],
            Ok("摘要 sk-secret\nAuthorization: Bearer old-secret"),
        )),
        Arc::new(events),
        Some("sk-secret".to_string()),
    );

    service
        .run("新问题".to_string(), "request-3".to_string())
        .await
        .unwrap();

    let summary = ContextBuilder::new(memory)
        .build("next")
        .await
        .unwrap()
        .summary
        .expect("eligible old memory must be summarized");
    assert!(summary.contains("摘要 [REDACTED]"));
    assert!(summary.contains("Authorization: [REDACTED]"));
    assert!(!summary.contains("sk-secret"));
    assert!(!summary.contains("old-secret"));
}

#[tokio::test]
async fn assistant_persistence_failure_emits_one_scoped_terminal_error() {
    let (directory, memory) = memory();
    let connection = rusqlite::Connection::open(directory.path().join("aibb.sqlite3")).unwrap();
    connection
        .execute_batch(
            "CREATE TRIGGER reject_assistant BEFORE INSERT ON messages \
             WHEN NEW.role = 'assistant' BEGIN SELECT RAISE(FAIL, 'blocked'); END;",
        )
        .unwrap();
    let events = RecordingEvents::new(memory.clone());
    let service = ChatService::new(
        memory,
        Arc::new(FakeLlm::new(&["reply"], Ok("unused"))),
        Arc::new(events.clone()),
        Some("sk-secret".to_string()),
    );

    let error = service
        .run("question".to_string(), "request-failure".to_string())
        .await
        .unwrap_err();

    assert_eq!(error.code, "storageUnavailable");
    assert_eq!(
        events
            .snapshot()
            .iter()
            .filter(|event| matches!(
                event,
                ChatEvent::Error { request_id, code, .. }
                    if request_id == "request-failure" && code == "storageUnavailable"
            ))
            .count(),
        1
    );
}
