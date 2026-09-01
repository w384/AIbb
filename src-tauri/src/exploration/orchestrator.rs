use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::{
    domain::{ExplorationResult, MemoryContext, SummaryCandidate, WebMaterial, WebMode},
    error::{sanitize_sensitive_text, AppError, ErrorCode},
    llm::{ChatMessage, ChatRequest, LlmTransport, NativeWebOutcome, NativeWebRequest},
    memory::{ContextBuilder, MemoryRepository},
    prompts::{
        build_exploration_prompt, ModelPrompt, EXPLORATION_SYSTEM_INSTRUCTION,
        SUMMARIZATION_INSTRUCTION,
    },
    web::{DuckDuckGoHtmlSearch, FetchBudget, PageFetcher, SafePageFetcher, SearchProvider},
};

use super::{build_contract_correction, parse_exploration_result};

pub const EXPLORATION_PROGRESS_EVENT: &str = "exploration://progress";
pub const EXPLORATION_COMPLETE_EVENT: &str = "exploration://complete";
pub const EXPLORATION_ERROR_EVENT: &str = "exploration://error";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExplorationRequest {
    pub direction: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum UserInputIntent {
    Chat,
    Explore { direction: Option<String> },
}

pub fn parse_outing_command(input: &str) -> UserInputIntent {
    let input = input.trim();
    if matches!(input, "去玩" | "出去玩") {
        return UserInputIntent::Explore { direction: None };
    }
    if input.starts_with("去年") {
        return UserInputIntent::Chat;
    }

    if let Some(direction) = input
        .strip_prefix('往')
        .and_then(|value| value.strip_suffix("方向玩"))
        .map(str::trim)
        .filter(|value| is_safe_direction(value))
    {
        return UserInputIntent::Explore {
            direction: Some(direction.to_string()),
        };
    }

    if let Some(direction) = input
        .strip_prefix('去')
        .and_then(|value| value.strip_suffix('玩'))
        .filter(|value| *value == value.trim())
        .filter(|value| is_safe_direction(value))
        .filter(|value| !is_obvious_statement(value))
    {
        return UserInputIntent::Explore {
            direction: Some(direction.to_string()),
        };
    }

    UserInputIntent::Chat
}

fn is_obvious_statement(direction: &str) -> bool {
    direction.ends_with("很好") || direction.ends_with("真好")
}

fn is_safe_direction(direction: &str) -> bool {
    !direction.is_empty()
        && !direction
            .chars()
            .any(|character| character.is_control() || is_unicode_format(character))
}

fn is_unicode_format(character: char) -> bool {
    matches!(
        character,
        '\u{00ad}'
            | '\u{0600}'..='\u{0605}'
            | '\u{061c}'
            | '\u{06dd}'
            | '\u{070f}'
            | '\u{0890}'..='\u{0891}'
            | '\u{08e2}'
            | '\u{180e}'
            | '\u{200b}'..='\u{200f}'
            | '\u{202a}'..='\u{202e}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{206f}'
            | '\u{feff}'
            | '\u{fff9}'..='\u{fffb}'
            | '\u{110bd}'
            | '\u{110cd}'
            | '\u{13430}'..='\u{13455}'
            | '\u{1bca0}'..='\u{1bca3}'
            | '\u{1d173}'..='\u{1d17a}'
            | '\u{e0001}'
            | '\u{e0020}'..='\u{e007f}'
    )
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ExplorationStatus {
    Queued,
    Choosing,
    NativeSearching,
    PublicSearching,
    Reading,
    Writing,
    Correcting,
    Completed,
    Cancelled,
    Interrupted,
    Failed,
}

impl ExplorationStatus {
    pub fn as_storage_value(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Choosing => "choosing",
            Self::NativeSearching => "native_searching",
            Self::PublicSearching => "public_searching",
            Self::Reading => "reading",
            Self::Writing => "writing",
            Self::Correcting => "correcting",
            Self::Completed => "completed",
            Self::Cancelled => "cancelled",
            Self::Interrupted => "interrupted",
            Self::Failed => "failed",
        }
    }

    pub fn from_storage_value(value: &str) -> Option<Self> {
        Some(match value {
            "queued" => Self::Queued,
            "choosing" => Self::Choosing,
            "native_searching" => Self::NativeSearching,
            "public_searching" => Self::PublicSearching,
            "reading" => Self::Reading,
            "writing" => Self::Writing,
            "correcting" => Self::Correcting,
            "completed" => Self::Completed,
            "cancelled" => Self::Cancelled,
            "interrupted" => Self::Interrupted,
            "failed" => Self::Failed,
            _ => return None,
        })
    }

    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Cancelled | Self::Interrupted | Self::Failed
        )
    }

    pub fn can_transition_to(self, next: Self) -> bool {
        if next == Self::Cancelled || next == Self::Interrupted || next == Self::Failed {
            return !self.is_terminal();
        }
        matches!(
            (self, next),
            (Self::Queued, Self::Choosing)
                | (Self::Choosing, Self::NativeSearching)
                | (Self::Choosing, Self::PublicSearching)
                | (Self::NativeSearching, Self::PublicSearching)
                | (Self::NativeSearching, Self::Writing)
                | (Self::PublicSearching, Self::Reading)
                | (Self::Reading, Self::Writing)
                | (Self::Writing, Self::Correcting)
                | (Self::Writing, Self::Completed)
                | (Self::Correcting, Self::Completed)
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplorationRecord {
    pub id: Uuid,
    pub status: ExplorationStatus,
    pub user_direction: Option<String>,
    pub items: Option<[String; 4]>,
    pub next_outing_request: Option<String>,
    pub raw_response: Option<String>,
    pub error_code: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancelOutcome {
    Cancelled,
    AlreadyCancelled,
    NotCancellable,
    NotFound,
}

#[async_trait]
pub trait ExplorationStore: Send + Sync {
    async fn create_queued(&self, id: Uuid, direction: Option<&str>) -> Result<(), AppError>;
    async fn transition(&self, id: Uuid, next: ExplorationStatus) -> Result<(), AppError>;
    async fn complete(
        &self,
        id: Uuid,
        result: &ExplorationResult,
        safe_raw_response: &str,
    ) -> Result<(), AppError>;
    async fn fail(
        &self,
        id: Uuid,
        error_code: &str,
        raw_response: Option<&str>,
    ) -> Result<(), AppError>;
    async fn cancel(&self, id: Uuid) -> Result<CancelOutcome, AppError>;
    async fn recover_interrupted(&self) -> Result<usize, AppError>;
    async fn load(&self, id: Uuid) -> Result<Option<ExplorationRecord>, AppError>;
}

#[async_trait]
pub trait ExplorationMemory: Send + Sync {
    async fn build_context(&self, current_input: String) -> Result<MemoryContext, AppError>;
    async fn summary_candidate(&self) -> Result<Option<SummaryCandidate>, AppError>;
    async fn save_summary(
        &self,
        candidate: &SummaryCandidate,
        content: String,
    ) -> Result<(), AppError>;
}

#[async_trait]
impl ExplorationMemory for MemoryRepository {
    async fn build_context(&self, current_input: String) -> Result<MemoryContext, AppError> {
        ContextBuilder::new(self.clone()).build(current_input).await
    }

    async fn summary_candidate(&self) -> Result<Option<SummaryCandidate>, AppError> {
        ContextBuilder::new(self.clone()).summary_candidate().await
    }

    async fn save_summary(
        &self,
        candidate: &SummaryCandidate,
        content: String,
    ) -> Result<(), AppError> {
        MemoryRepository::save_summary(self, candidate, content).await
    }
}

pub struct PublicWebRuntime {
    search: Arc<dyn SearchProvider>,
    fetcher: Arc<dyn PageFetcher>,
}

impl PublicWebRuntime {
    pub fn new(search: Arc<dyn SearchProvider>, fetcher: Arc<dyn PageFetcher>) -> Self {
        Self { search, fetcher }
    }
}

pub trait PublicWebFactory: Send + Sync {
    fn create(&self, cancellation: CancellationToken) -> Result<PublicWebRuntime, AppError>;
}

#[derive(Clone)]
pub enum ExplorationTaskCredential {
    Exact(String),
    Missing,
    Unavailable,
}

impl ExplorationTaskCredential {
    pub fn exact(api_key: impl Into<String>) -> Self {
        let api_key = api_key.into();
        if api_key.is_empty() {
            Self::Missing
        } else {
            Self::Exact(api_key)
        }
    }

    pub const fn missing() -> Self {
        Self::Missing
    }

    pub const fn unavailable() -> Self {
        Self::Unavailable
    }

    fn exact_key(&self) -> Option<&str> {
        match self {
            Self::Exact(api_key) => Some(api_key),
            Self::Missing | Self::Unavailable => None,
        }
    }
}

pub struct ExplorationTaskRuntime {
    llm: Arc<dyn LlmTransport>,
    web_mode: WebMode,
    credential: ExplorationTaskCredential,
}

impl ExplorationTaskRuntime {
    pub fn new(
        llm: Arc<dyn LlmTransport>,
        web_mode: WebMode,
        credential: ExplorationTaskCredential,
    ) -> Self {
        Self {
            llm,
            web_mode,
            credential,
        }
    }
}

#[async_trait]
pub trait ExplorationRuntimeFactory: Send + Sync {
    async fn create(&self) -> Result<ExplorationTaskRuntime, AppError>;
}

struct StaticRuntimeFactory {
    llm: Arc<dyn LlmTransport>,
    web_mode: WebMode,
    credential: ExplorationTaskCredential,
}

#[async_trait]
impl ExplorationRuntimeFactory for StaticRuntimeFactory {
    async fn create(&self) -> Result<ExplorationTaskRuntime, AppError> {
        Ok(ExplorationTaskRuntime::new(
            self.llm.clone(),
            self.web_mode,
            self.credential.clone(),
        ))
    }
}

#[derive(Debug, Default)]
pub struct DefaultPublicWebFactory;

impl PublicWebFactory for DefaultPublicWebFactory {
    fn create(&self, cancellation: CancellationToken) -> Result<PublicWebRuntime, AppError> {
        let search = DuckDuckGoHtmlSearch::new(cancellation.clone())?;
        let fetcher = SafePageFetcher::new(cancellation, FetchBudget::new());
        Ok(PublicWebRuntime::new(Arc::new(search), Arc::new(fetcher)))
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ExplorationEvent {
    Progress {
        #[serde(rename = "taskId")]
        task_id: Uuid,
        status: ExplorationStatus,
    },
    Complete {
        #[serde(rename = "taskId")]
        task_id: Uuid,
        result: ExplorationResult,
    },
    Error {
        #[serde(rename = "taskId")]
        task_id: Uuid,
        code: String,
        message: String,
    },
}

impl ExplorationEvent {
    pub fn task_id(&self) -> Uuid {
        match self {
            Self::Progress { task_id, .. }
            | Self::Complete { task_id, .. }
            | Self::Error { task_id, .. } => *task_id,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Progress { .. } => EXPLORATION_PROGRESS_EVENT,
            Self::Complete { .. } => EXPLORATION_COMPLETE_EVENT,
            Self::Error { .. } => EXPLORATION_ERROR_EVENT,
        }
    }
}

#[async_trait]
pub trait ExplorationEventSink: Send + Sync {
    async fn emit(&self, event: ExplorationEvent) -> Result<(), AppError>;
}

#[derive(Debug, Default)]
pub struct NoopEventSink;

#[async_trait]
impl ExplorationEventSink for NoopEventSink {
    async fn emit(&self, _event: ExplorationEvent) -> Result<(), AppError> {
        Ok(())
    }
}

#[async_trait]
pub trait Notifier: Send + Sync {
    async fn exploration_complete(&self, task_id: Uuid) -> Result<(), AppError>;
}

#[derive(Debug, Default)]
pub struct NoopNotifier;

#[async_trait]
impl Notifier for NoopNotifier {
    async fn exploration_complete(&self, _task_id: Uuid) -> Result<(), AppError> {
        Ok(())
    }
}

#[derive(Clone)]
pub struct ExplorationOrchestrator {
    store: Arc<dyn ExplorationStore>,
    memory: Arc<dyn ExplorationMemory>,
    runtime_factory: Arc<dyn ExplorationRuntimeFactory>,
    web: Arc<dyn PublicWebFactory>,
    events: Arc<dyn ExplorationEventSink>,
    notifier: Arc<dyn Notifier>,
    cancellations: Arc<Mutex<HashMap<Uuid, CancellationToken>>>,
}

impl ExplorationOrchestrator {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        store: Arc<dyn ExplorationStore>,
        memory: Arc<dyn ExplorationMemory>,
        llm: Arc<dyn LlmTransport>,
        web: Arc<dyn PublicWebFactory>,
        events: Arc<dyn ExplorationEventSink>,
        notifier: Arc<dyn Notifier>,
        web_mode: WebMode,
        credential: ExplorationTaskCredential,
    ) -> Self {
        Self::with_runtime_factory(
            store,
            memory,
            Arc::new(StaticRuntimeFactory {
                llm,
                web_mode,
                credential,
            }),
            web,
            events,
            notifier,
        )
    }

    pub fn with_runtime_factory(
        store: Arc<dyn ExplorationStore>,
        memory: Arc<dyn ExplorationMemory>,
        runtime_factory: Arc<dyn ExplorationRuntimeFactory>,
        web: Arc<dyn PublicWebFactory>,
        events: Arc<dyn ExplorationEventSink>,
        notifier: Arc<dyn Notifier>,
    ) -> Self {
        Self {
            store,
            memory,
            runtime_factory,
            web,
            events,
            notifier,
            cancellations: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn start(&self, request: ExplorationRequest) -> Result<Uuid, AppError> {
        let (task_id, cancellation) = self.prepare(&request).await?;
        let orchestrator = self.clone();
        tokio::spawn(async move {
            let _ = orchestrator
                .execute_managed(task_id, request, cancellation)
                .await;
        });
        Ok(task_id)
    }

    pub async fn run(&self, request: ExplorationRequest) -> Result<ExplorationResult, AppError> {
        let (task_id, cancellation) = self.prepare(&request).await?;
        self.execute_managed(task_id, request, cancellation).await
    }

    pub async fn cancel(&self, task_id: Uuid) -> Result<(), AppError> {
        match self.store.cancel(task_id).await? {
            CancelOutcome::Cancelled => {
                if let Some(token) = self.cancellation(task_id)? {
                    token.cancel();
                }
                self.emit_error(task_id, AppError::from_code(ErrorCode::Cancelled))
                    .await;
                Ok(())
            }
            CancelOutcome::AlreadyCancelled => Ok(()),
            CancelOutcome::NotFound => Err(AppError::new(
                "exploration_not_found",
                "The exploration task was not found.",
            )),
            CancelOutcome::NotCancellable => Err(AppError::new(
                "exploration_not_cancellable",
                "The exploration task is already finished.",
            )),
        }
    }

    pub async fn recover_interrupted(&self) -> Result<usize, AppError> {
        self.store.recover_interrupted().await
    }

    async fn prepare(
        &self,
        request: &ExplorationRequest,
    ) -> Result<(Uuid, CancellationToken), AppError> {
        let task_id = Uuid::new_v4();
        self.store
            .create_queued(task_id, request.direction.as_deref())
            .await?;
        let cancellation = CancellationToken::new();
        self.cancellations
            .lock()
            .map_err(|_| state_error())?
            .insert(task_id, cancellation.clone());
        Ok((task_id, cancellation))
    }

    async fn execute_managed(
        &self,
        task_id: Uuid,
        request: ExplorationRequest,
        cancellation: CancellationToken,
    ) -> Result<ExplorationResult, AppError> {
        let result = self.execute(task_id, request, cancellation).await;
        self.cancellations
            .lock()
            .map_err(|_| state_error())?
            .remove(&task_id);

        if let Err(error) = &result {
            let persisted = self.store.load(task_id).await?;
            if persisted
                .as_ref()
                .is_some_and(|record| !record.status.is_terminal())
            {
                if error.code == ErrorCode::Cancelled.as_str() {
                    let _ = self.store.cancel(task_id).await?;
                } else {
                    self.store.fail(task_id, &error.code, None).await?;
                }
                self.emit_error(
                    task_id,
                    AppError::new(error.code.clone(), error.message.clone()),
                )
                .await;
            } else if persisted
                .as_ref()
                .is_some_and(|record| record.status == ExplorationStatus::Failed)
            {
                self.emit_error(
                    task_id,
                    AppError::new(error.code.clone(), error.message.clone()),
                )
                .await;
            }
        }
        result
    }

    async fn execute(
        &self,
        task_id: Uuid,
        request: ExplorationRequest,
        cancellation: CancellationToken,
    ) -> Result<ExplorationResult, AppError> {
        ensure_not_cancelled(&cancellation)?;
        let runtime = self.runtime_factory.create().await?;
        let context = self
            .memory
            .build_context(request.direction.clone().unwrap_or_default())
            .await?;
        self.progress(task_id, ExplorationStatus::Choosing).await?;

        let web_mode = runtime.web_mode;
        let raw = match web_mode {
            WebMode::Off => {
                self.public_exploration(task_id, context.clone(), &runtime, cancellation.clone())
                    .await?
            }
            WebMode::Auto | WebMode::Force => {
                self.progress(task_id, ExplorationStatus::NativeSearching)
                    .await?;
                let prompt = build_exploration_prompt(context.clone(), WebMaterial::empty());
                match runtime
                    .llm
                    .try_native_web(
                        NativeWebRequest {
                            input: prompt_as_text(&prompt),
                        },
                        cancellation.clone(),
                    )
                    .await
                {
                    Ok(NativeWebOutcome::Completed(raw)) => {
                        self.progress(task_id, ExplorationStatus::Writing).await?;
                        raw
                    }
                    Ok(NativeWebOutcome::Unsupported) if web_mode == WebMode::Auto => {
                        self.public_exploration(task_id, context, &runtime, cancellation.clone())
                            .await?
                    }
                    Err(error)
                        if web_mode == WebMode::Auto && is_native_capability_error(&error) =>
                    {
                        self.public_exploration(task_id, context, &runtime, cancellation.clone())
                            .await?
                    }
                    Ok(NativeWebOutcome::Unsupported) => {
                        return Err(AppError::from_code(ErrorCode::NativeWebUnsupported));
                    }
                    Err(error) if is_native_capability_error(&error) => {
                        return Err(AppError::from_code(ErrorCode::NativeWebUnsupported));
                    }
                    Err(error) => return Err(error),
                }
            }
        };

        ensure_not_cancelled(&cancellation)?;
        let result = match parse_exploration_result(&raw) {
            Ok(result) => result,
            Err(violation) => {
                self.progress(task_id, ExplorationStatus::Correcting)
                    .await?;
                let correction = build_contract_correction(&raw, violation);
                let corrected = runtime
                    .llm
                    .complete(
                        ChatRequest {
                            messages: vec![
                                ChatMessage::new("system", EXPLORATION_SYSTEM_INSTRUCTION),
                                ChatMessage::user(correction),
                            ],
                        },
                        cancellation.clone(),
                    )
                    .await?;
                match parse_exploration_result(&corrected) {
                    Ok(result) => result,
                    Err(_) => {
                        let safe_raw = sanitize_raw(&corrected, &runtime.credential);
                        self.store
                            .fail(task_id, "format_incomplete", Some(&safe_raw))
                            .await?;
                        return Err(AppError::new(
                            "format_incomplete",
                            "The exploration result did not match the required format.",
                        ));
                    }
                }
            }
        };

        let result = sanitize_result(result, &runtime.credential);
        self.store
            .complete(task_id, &result, &result.raw_response)
            .await?;
        self.summarize_once(&runtime, cancellation.clone()).await;
        let _ = self
            .events
            .emit(ExplorationEvent::Complete {
                task_id,
                result: result.clone(),
            })
            .await;
        let _ = self.notifier.exploration_complete(task_id).await;
        Ok(result)
    }

    async fn public_exploration(
        &self,
        task_id: Uuid,
        context: MemoryContext,
        task_runtime: &ExplorationTaskRuntime,
        cancellation: CancellationToken,
    ) -> Result<String, AppError> {
        self.progress(task_id, ExplorationStatus::PublicSearching)
            .await?;
        let query_raw = task_runtime
            .llm
            .complete(build_query_request(&context), cancellation.clone())
            .await?;
        let queries = parse_query_envelope(&query_raw)?;
        let public_web = self.web.create(cancellation.clone())?;
        let mut seen = HashSet::new();
        let mut urls = Vec::new();
        for query in queries {
            ensure_not_cancelled(&cancellation)?;
            for url in public_web
                .search
                .search(&query, 2)
                .await?
                .into_iter()
                .take(2)
            {
                if seen.insert(url.clone()) {
                    urls.push(url);
                    if urls.len() == 8 {
                        break;
                    }
                }
            }
            if urls.len() == 8 {
                break;
            }
        }

        self.progress(task_id, ExplorationStatus::Reading).await?;
        let mut pages = Vec::new();
        for url in urls.into_iter().take(8) {
            ensure_not_cancelled(&cancellation)?;
            match public_web.fetcher.fetch(&url).await {
                Ok(page) => pages.push(format!(
                    "标题：{}\n地址：{}\n正文：{}",
                    page.title, page.canonical_url, page.text
                )),
                Err(error) if error.code == ErrorCode::Cancelled.as_str() => return Err(error),
                Err(_) => continue,
            }
        }

        self.progress(task_id, ExplorationStatus::Writing).await?;
        let prompt = build_exploration_prompt(context, WebMaterial { pages });
        task_runtime
            .llm
            .complete(prompt_as_chat_request(&prompt), cancellation)
            .await
    }

    async fn progress(&self, task_id: Uuid, status: ExplorationStatus) -> Result<(), AppError> {
        self.store.transition(task_id, status).await?;
        let _ = self
            .events
            .emit(ExplorationEvent::Progress { task_id, status })
            .await;
        Ok(())
    }

    async fn summarize_once(
        &self,
        runtime: &ExplorationTaskRuntime,
        cancellation: CancellationToken,
    ) {
        let Some(exact_key) = runtime.credential.exact_key() else {
            return;
        };
        let Ok(Some(candidate)) = self.memory.summary_candidate().await else {
            return;
        };
        let messages = candidate
            .messages
            .iter()
            .map(|message| format!("{}: {}", message.role.as_storage_value(), message.content))
            .collect::<Vec<_>>()
            .join("\n");
        let request = ChatRequest {
            messages: vec![
                ChatMessage::new("system", SUMMARIZATION_INSTRUCTION),
                ChatMessage::user(messages),
            ],
        };
        let Ok(summary) = runtime.llm.complete(request, cancellation).await else {
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

    async fn emit_error(&self, task_id: Uuid, error: AppError) {
        let _ = self
            .events
            .emit(ExplorationEvent::Error {
                task_id,
                code: error.code,
                message: error.message,
            })
            .await;
    }

    fn cancellation(&self, task_id: Uuid) -> Result<Option<CancellationToken>, AppError> {
        Ok(self
            .cancellations
            .lock()
            .map_err(|_| state_error())?
            .get(&task_id)
            .cloned())
    }
}

fn sanitize_result(
    mut result: ExplorationResult,
    credential: &ExplorationTaskCredential,
) -> ExplorationResult {
    let exact_key = credential.exact_key();
    for item in &mut result.items {
        *item = sanitize_sensitive_text(item, exact_key);
    }
    result.next_outing_request = sanitize_sensitive_text(&result.next_outing_request, exact_key);
    if exact_key.is_some() {
        result.raw_response = sanitize_sensitive_text(&result.raw_response, exact_key);
    } else {
        result.raw_response = "[RAW RESPONSE OMITTED]".to_string();
    }
    result
}

fn sanitize_raw(raw: &str, credential: &ExplorationTaskCredential) -> String {
    credential
        .exact_key()
        .map(|api_key| sanitize_sensitive_text(raw, Some(api_key)))
        .unwrap_or_else(|| "[RAW RESPONSE OMITTED]".to_string())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct QueryEnvelope {
    queries: Vec<String>,
}

fn parse_query_envelope(raw: &str) -> Result<Vec<String>, AppError> {
    let envelope: QueryEnvelope = serde_json::from_str(raw.trim()).map_err(|_| query_error())?;
    if !(1..=4).contains(&envelope.queries.len()) {
        return Err(query_error());
    }
    let queries = envelope
        .queries
        .into_iter()
        .map(|query| query.trim().to_string())
        .collect::<Vec<_>>();
    if queries.iter().any(String::is_empty) {
        return Err(query_error());
    }
    Ok(queries)
}

fn build_query_request(context: &MemoryContext) -> ChatRequest {
    let choice = if context.current_input.trim().is_empty() {
        "用户没有指定方向，由你自由选择查询内容。"
    } else {
        "查询内容围绕用户给出的方向。"
    };
    ChatRequest {
        messages: vec![
            ChatMessage::new(
                "system",
                "只输出 JSON 对象 {\"queries\":[...]}；queries 必须是 1 至 4 个非空自由字符串。不要输出主题、类别、理由或其他字段。",
            ),
            ChatMessage::user(format!("{choice}\n用户方向：{}", context.current_input)),
        ],
    }
}

fn prompt_as_chat_request(prompt: &ModelPrompt) -> ChatRequest {
    ChatRequest {
        messages: vec![
            ChatMessage::new("system", prompt.system_instruction.clone()),
            ChatMessage::user(prompt_context_text(prompt)),
        ],
    }
}

fn prompt_as_text(prompt: &ModelPrompt) -> String {
    format!(
        "{}\n\n{}",
        prompt.system_instruction,
        prompt_context_text(prompt)
    )
}

fn prompt_context_text(prompt: &ModelPrompt) -> String {
    let recent = prompt
        .recent_messages
        .iter()
        .map(|message| format!("{}: {}", message.role.as_storage_value(), message.content))
        .collect::<Vec<_>>()
        .join("\n");
    let pages = prompt
        .web_material
        .as_ref()
        .map(|material| material.pages.join("\n\n"))
        .unwrap_or_default();
    format!(
        "当前输入：{}\n上一段：{}\n近期消息：{}\n旧摘要：{}\n公开网页材料：{}",
        prompt.current_input,
        prompt
            .last_assistant_paragraph
            .as_deref()
            .unwrap_or_default(),
        recent,
        prompt.summary.as_deref().unwrap_or_default(),
        pages
    )
}

fn ensure_not_cancelled(cancellation: &CancellationToken) -> Result<(), AppError> {
    if cancellation.is_cancelled() {
        Err(AppError::from_code(ErrorCode::Cancelled))
    } else {
        Ok(())
    }
}

fn is_native_capability_error(error: &AppError) -> bool {
    matches!(
        error.code.as_str(),
        "provider_capability_unsupported" | "native_web_unsupported"
    )
}

fn query_error() -> AppError {
    AppError::new(
        "invalid_query_envelope",
        "The model returned an invalid public-search query envelope.",
    )
}

fn state_error() -> AppError {
    AppError::new(
        "exploration_state_unavailable",
        "The exploration task state is unavailable.",
    )
}
