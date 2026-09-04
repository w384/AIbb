use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde::Serialize;
use tauri::Emitter;
use tokio_util::sync::CancellationToken;

use crate::{
    app_state::AppState,
    domain::{MemoryContext, Role, SummaryCandidate},
    error::{sanitize_sensitive_text, AppError, ErrorCode},
    exploration::{parse_outing_command, ExplorationRequest, UserInputIntent},
    llm::{ChatMessage, ChatRequest, DeltaSink, LlmTransport, OpenAiClient},
    memory::{ContextBuilder, MemoryRepository},
    prompts::{build_chat_prompt, ModelPrompt, SUMMARIZATION_INSTRUCTION},
    settings::{FixedCredentialStore, SettingsService},
};

pub const CHAT_DELTA_EVENT: &str = "chat://delta";
pub const CHAT_COMPLETE_EVENT: &str = "chat://complete";
pub const CHAT_ERROR_EVENT: &str = "chat://error";

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ChatEvent {
    Delta {
        #[serde(rename = "requestId")]
        request_id: String,
        delta: String,
    },
    Complete {
        #[serde(rename = "requestId")]
        request_id: String,
        message: String,
    },
    Error {
        #[serde(rename = "requestId")]
        request_id: String,
        code: String,
        message: String,
    },
}

impl ChatEvent {
    fn name(&self) -> &'static str {
        match self {
            Self::Delta { .. } => CHAT_DELTA_EVENT,
            Self::Complete { .. } => CHAT_COMPLETE_EVENT,
            Self::Error { .. } => CHAT_ERROR_EVENT,
        }
    }
}

#[async_trait]
pub trait ChatEventSink: Send + Sync {
    async fn emit(&self, event: ChatEvent) -> Result<(), AppError>;
}

pub struct ChatTaskRuntime {
    llm: Arc<dyn LlmTransport>,
    exact_key: Option<String>,
}

impl ChatTaskRuntime {
    pub fn new(llm: Arc<dyn LlmTransport>, exact_key: Option<String>) -> Self {
        Self { llm, exact_key }
    }
}

#[async_trait]
pub trait ChatRuntimeFactory: Send + Sync {
    async fn create(&self) -> Result<ChatTaskRuntime, AppError>;
}

struct StaticChatRuntimeFactory {
    llm: Arc<dyn LlmTransport>,
    exact_key: Option<String>,
}

#[async_trait]
impl ChatRuntimeFactory for StaticChatRuntimeFactory {
    async fn create(&self) -> Result<ChatTaskRuntime, AppError> {
        Ok(ChatTaskRuntime::new(
            self.llm.clone(),
            self.exact_key.clone(),
        ))
    }
}

#[derive(Clone)]
pub struct ChatService {
    memory: MemoryRepository,
    runtime_factory: Arc<dyn ChatRuntimeFactory>,
    events: Arc<dyn ChatEventSink>,
}

impl ChatService {
    pub fn new(
        memory: MemoryRepository,
        llm: Arc<dyn LlmTransport>,
        events: Arc<dyn ChatEventSink>,
        exact_key: Option<String>,
    ) -> Self {
        Self::with_runtime_factory(
            memory,
            Arc::new(StaticChatRuntimeFactory { llm, exact_key }),
            events,
        )
    }

    pub fn with_runtime_factory(
        memory: MemoryRepository,
        runtime_factory: Arc<dyn ChatRuntimeFactory>,
        events: Arc<dyn ChatEventSink>,
    ) -> Self {
        Self {
            memory,
            runtime_factory,
            events,
        }
    }

    pub async fn start(&self, message: String, request_id: String) -> Result<(), AppError> {
        let prepared = self.prepare(message, request_id).await?;
        let service = self.clone();
        tokio::spawn(async move {
            let _ = service.execute(prepared).await;
        });
        Ok(())
    }

    pub async fn run(&self, message: String, request_id: String) -> Result<(), AppError> {
        let prepared = self.prepare(message, request_id).await?;
        self.execute(prepared).await
    }

    async fn prepare(&self, message: String, request_id: String) -> Result<PreparedChat, AppError> {
        let message = message.trim().to_string();
        if message.is_empty() || request_id.trim().is_empty() || request_id.len() > 128 {
            return Err(AppError::from_code(ErrorCode::InvalidRequest));
        }

        let runtime = self.runtime_factory.create().await?;
        let message = sanitize_sensitive_text(&message, runtime.exact_key.as_deref());
        let memory_generation = self.memory.generation();
        let context = ContextBuilder::new(self.memory.clone())
            .build(message.clone())
            .await?;
        if self
            .memory
            .append_if_generation(memory_generation, Role::User, message)
            .await?
            .is_none()
        {
            return Err(AppError::from_code(ErrorCode::Cancelled));
        }

        Ok(PreparedChat {
            request_id,
            context,
            runtime,
            memory_generation,
        })
    }

    async fn execute(&self, prepared: PreparedChat) -> Result<(), AppError> {
        let request_id = prepared.request_id.clone();
        let exact_key = prepared.runtime.exact_key.clone();
        let result = self.execute_inner(prepared).await;
        if let Err(error) = result {
            let safe = AppError::sanitized(error.code, error.message, exact_key.as_deref());
            let _ = self
                .events
                .emit(ChatEvent::Error {
                    request_id,
                    code: safe.code.clone(),
                    message: safe.message.clone(),
                })
                .await;
            return Err(safe);
        }
        Ok(())
    }

    async fn execute_inner(&self, prepared: PreparedChat) -> Result<(), AppError> {
        let request = chat_request(&build_chat_prompt(prepared.context));
        let sink = SafeChatStream::new(
            prepared.request_id.clone(),
            self.events.clone(),
            prepared.runtime.exact_key.clone(),
        );
        let cancellation = CancellationToken::new();

        prepared
            .runtime
            .llm
            .stream_chat(request, &sink, cancellation.clone())
            .await?;

        let reply = sink.finish().await?;
        if reply.trim().is_empty() {
            return Err(AppError::from_code(ErrorCode::InvalidResponse));
        }

        if self
            .memory
            .append_if_generation(prepared.memory_generation, Role::Assistant, reply.clone())
            .await?
            .is_none()
        {
            return Err(AppError::from_code(ErrorCode::Cancelled));
        }
        self.events
            .emit(ChatEvent::Complete {
                request_id: prepared.request_id,
                message: reply,
            })
            .await?;
        self.summarize_once(&prepared.runtime, cancellation).await;
        Ok(())
    }

    async fn summarize_once(&self, runtime: &ChatTaskRuntime, cancellation: CancellationToken) {
        let Some(exact_key) = runtime.exact_key.as_deref() else {
            return;
        };
        let Ok(Some(candidate)) = ContextBuilder::new(self.memory.clone())
            .summary_candidate()
            .await
        else {
            return;
        };
        let Ok(summary) = runtime
            .llm
            .complete(summary_request(&candidate), cancellation)
            .await
        else {
            return;
        };
        let summary = sanitize_sensitive_text(&summary, Some(exact_key));
        let summary = summary.trim();
        if summary.is_empty() {
            return;
        }
        let _ = self
            .memory
            .save_summary(&candidate, summary.to_string())
            .await;
    }
}

struct PreparedChat {
    request_id: String,
    context: MemoryContext,
    runtime: ChatTaskRuntime,
    memory_generation: u64,
}

fn chat_request(prompt: &ModelPrompt) -> ChatRequest {
    let recent = prompt
        .recent_messages
        .iter()
        .map(|message| format!("{}: {}", message.role.as_storage_value(), message.content))
        .collect::<Vec<_>>()
        .join("\n");
    ChatRequest {
        messages: vec![
            ChatMessage::new("system", prompt.system_instruction.clone()),
            ChatMessage::user(format!(
                "当前输入：{}\n上一段：{}\n近期消息：{}\n旧摘要：{}",
                prompt.current_input,
                prompt
                    .last_assistant_paragraph
                    .as_deref()
                    .unwrap_or_default(),
                recent,
                prompt.summary.as_deref().unwrap_or_default(),
            )),
        ],
    }
}

fn summary_request(candidate: &SummaryCandidate) -> ChatRequest {
    let messages = candidate
        .messages
        .iter()
        .map(|message| format!("{}: {}", message.role.as_storage_value(), message.content))
        .collect::<Vec<_>>()
        .join("\n");
    ChatRequest {
        messages: vec![
            ChatMessage::new("system", SUMMARIZATION_INSTRUCTION),
            ChatMessage::user(messages),
        ],
    }
}

struct SafeChatStream {
    request_id: String,
    events: Arc<dyn ChatEventSink>,
    sanitizer: Mutex<IncrementalSanitizer>,
}

impl SafeChatStream {
    fn new(request_id: String, events: Arc<dyn ChatEventSink>, exact_key: Option<String>) -> Self {
        Self {
            request_id,
            events,
            sanitizer: Mutex::new(IncrementalSanitizer::new(exact_key)),
        }
    }

    async fn finish(&self) -> Result<String, AppError> {
        let (delta, reply) = {
            let mut sanitizer = self.sanitizer.lock().map_err(|_| stream_error())?;
            let delta = sanitizer.push("", true);
            (delta, sanitizer.output.clone())
        };
        if !delta.is_empty() {
            self.events
                .emit(ChatEvent::Delta {
                    request_id: self.request_id.clone(),
                    delta,
                })
                .await?;
        }
        Ok(reply)
    }
}

#[async_trait]
impl DeltaSink for SafeChatStream {
    async fn send(&self, delta: &str) -> Result<(), AppError> {
        let safe_delta = self
            .sanitizer
            .lock()
            .map_err(|_| stream_error())?
            .push(delta, false);
        if !safe_delta.is_empty() {
            self.events
                .emit(ChatEvent::Delta {
                    request_id: self.request_id.clone(),
                    delta: safe_delta,
                })
                .await?;
        }
        Ok(())
    }
}

struct IncrementalSanitizer {
    pending: String,
    output: String,
    exact_key: Option<String>,
}

impl IncrementalSanitizer {
    fn new(exact_key: Option<String>) -> Self {
        Self {
            pending: String::new(),
            output: String::new(),
            exact_key: exact_key.filter(|key| !key.is_empty()),
        }
    }

    fn push(&mut self, value: &str, final_chunk: bool) -> String {
        self.pending.push_str(value);
        let mut ready = String::new();

        loop {
            if self.pending.is_empty() {
                break;
            }
            let lower = self.pending.to_ascii_lowercase();
            let mut markers = Vec::new();
            if let Some(key) = self.exact_key.as_deref() {
                if let Some(position) = self.pending.find(key) {
                    markers.push((position, SensitiveMarker::Exact));
                }
            }
            if let Some(position) = lower.find("authorization") {
                markers.push((position, SensitiveMarker::Authorization));
            }
            if let Some(position) = lower.find("bearer") {
                markers.push((position, SensitiveMarker::Bearer));
            }
            markers.sort_by_key(|(position, marker)| (*position, marker.priority()));

            let Some((position, marker)) = markers.first().copied() else {
                if final_chunk {
                    ready.push_str(&sanitize_sensitive_text(
                        &self.pending,
                        self.exact_key.as_deref(),
                    ));
                    self.pending.clear();
                } else {
                    let hold = partial_marker_suffix(&self.pending, self.exact_key.as_deref());
                    let emit_len = self.pending.len().saturating_sub(hold);
                    ready.push_str(&self.pending[..emit_len]);
                    self.pending.drain(..emit_len);
                }
                break;
            };

            if position > 0 {
                ready.push_str(&self.pending[..position]);
                self.pending.drain(..position);
                continue;
            }

            match marker {
                SensitiveMarker::Exact => {
                    let key_len = self.exact_key.as_deref().map(str::len).unwrap_or(0);
                    self.pending.drain(..key_len);
                    ready.push_str("[REDACTED]");
                }
                SensitiveMarker::Authorization => {
                    if let Some(line_end) = self.pending.find('\n') {
                        ready.push_str(&sanitize_sensitive_text(
                            &self.pending[..line_end],
                            self.exact_key.as_deref(),
                        ));
                        ready.push('\n');
                        self.pending.drain(..=line_end);
                    } else if final_chunk {
                        ready.push_str(&sanitize_sensitive_text(
                            &self.pending,
                            self.exact_key.as_deref(),
                        ));
                        self.pending.clear();
                    } else {
                        break;
                    }
                }
                SensitiveMarker::Bearer => {
                    let keyword_end = "bearer".len();
                    let after_keyword = &self.pending[keyword_end..];
                    let whitespace_end = after_keyword
                        .char_indices()
                        .take_while(|(_, character)| character.is_whitespace())
                        .map(|(index, character)| index + character.len_utf8())
                        .last()
                        .unwrap_or(0);
                    if whitespace_end == 0 {
                        let first = self.pending.chars().next().unwrap();
                        let first_len = first.len_utf8();
                        ready.push(first);
                        self.pending.drain(..first_len);
                        continue;
                    }
                    let secret = &after_keyword[whitespace_end..];
                    let secret_end = secret
                        .char_indices()
                        .find(|(_, character)| {
                            character.is_whitespace()
                                || matches!(character, '"' | '\'' | ',' | '}' | ']' | '\\')
                        })
                        .map(|(index, _)| index);
                    if let Some(secret_end) = secret_end.filter(|end| *end > 0) {
                        let token_end = keyword_end + whitespace_end + secret_end;
                        ready.push_str(&sanitize_sensitive_text(
                            &self.pending[..token_end],
                            self.exact_key.as_deref(),
                        ));
                        self.pending.drain(..token_end);
                    } else if final_chunk {
                        ready.push_str(&sanitize_sensitive_text(
                            &self.pending,
                            self.exact_key.as_deref(),
                        ));
                        self.pending.clear();
                    } else {
                        break;
                    }
                }
            }
        }

        self.output.push_str(&ready);
        ready
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum SensitiveMarker {
    Exact,
    Authorization,
    Bearer,
}

impl SensitiveMarker {
    fn priority(self) -> u8 {
        match self {
            Self::Exact => 0,
            Self::Authorization => 1,
            Self::Bearer => 2,
        }
    }
}

fn partial_marker_suffix(value: &str, exact_key: Option<&str>) -> usize {
    let lower = value.to_ascii_lowercase();
    let mut longest = partial_suffix_len(&lower, "authorization");
    longest = longest.max(partial_suffix_len(&lower, "bearer"));
    if let Some(key) = exact_key {
        longest = longest.max(partial_suffix_len(value, key));
    }
    longest
}

fn partial_suffix_len(value: &str, marker: &str) -> usize {
    marker
        .char_indices()
        .skip(1)
        .map(|(index, _)| index)
        .filter(|index| *index < marker.len() && value.ends_with(&marker[..*index]))
        .max()
        .unwrap_or(0)
}

fn stream_error() -> AppError {
    AppError::new(
        "chatStreamUnavailable",
        "The chat stream could not be processed safely.",
    )
}

pub struct SettingsChatRuntimeFactory {
    settings: SettingsService,
}

impl SettingsChatRuntimeFactory {
    pub fn new(settings: SettingsService) -> Self {
        Self { settings }
    }
}

#[async_trait]
impl ChatRuntimeFactory for SettingsChatRuntimeFactory {
    async fn create(&self) -> Result<ChatTaskRuntime, AppError> {
        let snapshot = self.settings.exploration_task_snapshot().await?;
        let (settings, api_key) = snapshot.into_parts();
        let exact_key = api_key.clone().filter(|key| !key.is_empty());
        let llm = OpenAiClient::new(settings, FixedCredentialStore::new(api_key));
        Ok(ChatTaskRuntime::new(Arc::new(llm), exact_key))
    }
}

pub fn build_chat_service(
    app: tauri::AppHandle,
    memory: MemoryRepository,
    settings: SettingsService,
) -> ChatService {
    ChatService::with_runtime_factory(
        memory,
        Arc::new(SettingsChatRuntimeFactory::new(settings)),
        Arc::new(TauriChatEventSink { app }),
    )
}

struct TauriChatEventSink {
    app: tauri::AppHandle,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
struct ChatDeltaPayload<'a> {
    request_id: &'a str,
    delta: &'a str,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
struct ChatCompletePayload<'a> {
    request_id: &'a str,
    message: &'a str,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
struct ChatErrorPayload<'a> {
    request_id: &'a str,
    code: &'a str,
    message: &'a str,
}

#[async_trait]
impl ChatEventSink for TauriChatEventSink {
    async fn emit(&self, event: ChatEvent) -> Result<(), AppError> {
        let result = match &event {
            ChatEvent::Delta { request_id, delta } => {
                self.app
                    .emit_to("chat", event.name(), ChatDeltaPayload { request_id, delta })
            }
            ChatEvent::Complete {
                request_id,
                message,
            } => self.app.emit_to(
                "chat",
                event.name(),
                ChatCompletePayload {
                    request_id,
                    message,
                },
            ),
            ChatEvent::Error {
                request_id,
                code,
                message,
            } => self.app.emit_to(
                "chat",
                event.name(),
                ChatErrorPayload {
                    request_id,
                    code,
                    message,
                },
            ),
        };
        result.map_err(|_| {
            AppError::new(
                "chatEventUnavailable",
                "The chat event could not be delivered.",
            )
        })
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum InputDisposition {
    ChatStarted {
        #[serde(rename = "requestId")]
        request_id: String,
    },
    ExplorationStarted {
        #[serde(rename = "taskId")]
        task_id: String,
    },
}

pub async fn start_chat(
    state: tauri::State<'_, AppState>,
    message: String,
    request_id: String,
) -> Result<(), AppError> {
    state
        .chat
        .as_ref()
        .ok_or_else(chat_service_error)?
        .start(message, request_id)
        .await
}

#[tauri::command]
pub async fn submit_user_input(
    state: tauri::State<'_, AppState>,
    message: String,
    request_id: String,
) -> Result<InputDisposition, AppError> {
    match parse_outing_command(&message) {
        UserInputIntent::Chat => {
            start_chat(state, message, request_id.clone()).await?;
            Ok(InputDisposition::ChatStarted { request_id })
        }
        UserInputIntent::Explore { direction } => {
            let task_id = state
                .exploration
                .as_ref()
                .ok_or_else(exploration_service_error)?
                .start(ExplorationRequest { direction })
                .await?;
            Ok(InputDisposition::ExplorationStarted {
                task_id: task_id.to_string(),
            })
        }
    }
}

fn chat_service_error() -> AppError {
    AppError::new("chatServiceUnavailable", "The chat service is unavailable.")
}

fn exploration_service_error() -> AppError {
    AppError::new(
        "explorationServiceUnavailable",
        "The exploration service is unavailable.",
    )
}
