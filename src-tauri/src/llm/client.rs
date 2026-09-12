use std::sync::Arc;

use async_trait::async_trait;
use futures_util::StreamExt;
use reqwest::{RequestBuilder, Response, StatusCode};
use serde_json::{json, Value};
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

use crate::{
    domain::OutingSource,
    error::{AppError, ErrorCode},
    settings::{ApiSettings, CredentialStore},
};

use super::{
    ChatRequest, DeltaSink, LlmTransport, NativeWebOutcome, NativeWebRequest, SseDecoder,
    TransportTimeouts,
};

/// How many extra attempts `test_connection` makes after a transient failure
/// (5xx, transport error, timeout, or rate limit) before giving up.
const MAX_CONNECTION_ATTEMPTS: usize = 2;
const CONNECTION_RETRY_DELAY: std::time::Duration = std::time::Duration::from_millis(600);

fn is_transient_error(error: &AppError) -> bool {
    let code = error.code.as_str();
    code == ErrorCode::ProviderUnavailable.as_str()
        || code == ErrorCode::RequestTimeout.as_str()
        || code == ErrorCode::RateLimited.as_str()
}

pub struct OpenAiClient {
    http: reqwest::Client,
    settings: ApiSettings,
    credentials: Arc<dyn CredentialStore>,
    timeouts: TransportTimeouts,
}

impl OpenAiClient {
    pub fn new<C>(settings: ApiSettings, credentials: C) -> Self
    where
        C: CredentialStore + 'static,
    {
        Self::with_timeouts(settings, credentials, TransportTimeouts::default())
    }

    async fn test_connection_once(
        &self,
        cancellation: &CancellationToken,
    ) -> Result<(), AppError> {
        let operation = async {
            let api_key = self.api_key().await?;
            let request = self.http.get(self.endpoint("models")).bearer_auth(&api_key);
            let response = self
                .send_with_header_timeout(request, cancellation, &api_key)
                .await?;
            let status = response.status();
            let body = response.text().await.map_err(|error| {
                AppError::from_code(ErrorCode::ProviderUnavailable)
                    .with_diagnostic(error.to_string(), Some(&api_key))
            })?;

            if status.is_success() {
                // Parse the catalog to reject a 200 that is not a model list
                // (e.g. a website login page behind a wrong API base). A valid
                // catalog is advisory only: the chat endpoint is authoritative,
                // because some providers (e.g. DeepSeek) serve checkpoint ids
                // that differ from the chat-accepted alias. Probe it with a
                // 1-token completion and let IT decide whether the model
                // exists.
                let _catalog = parse_models_ids(&body)?;
                return self.connection_fallback(&api_key, cancellation).await;
            }
            if is_unsupported(status, &body) {
                return self.connection_fallback(&api_key, cancellation).await;
            }

            Err(AppError::from_http_body(
                status.as_u16(),
                &body,
                Some(&api_key),
                true,
            ))
        };

        self.execute_with_total(cancellation.clone(), operation)
            .await
    }

    pub fn with_timeouts<C>(
        settings: ApiSettings,
        credentials: C,
        timeouts: TransportTimeouts,
    ) -> Self
    where
        C: CredentialStore + 'static,
    {
        Self::from_shared_with_timeouts(settings, Arc::new(credentials), timeouts)
    }

    fn from_shared_with_timeouts(
        settings: ApiSettings,
        credentials: Arc<dyn CredentialStore>,
        timeouts: TransportTimeouts,
    ) -> Self {
        Self {
            http: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .expect("reqwest client configuration must be valid"),
            settings,
            credentials,
            timeouts,
        }
    }

    fn endpoint(&self, suffix: &str) -> String {
        let base = self
            .settings
            .api_base
            .strip_suffix('/')
            .unwrap_or(&self.settings.api_base);
        format!("{base}/{suffix}")
    }

    async fn api_key(&self) -> Result<String, AppError> {
        self.credentials
            .get()
            .await
            .map_err(|_| {
                AppError::new(
                    "credentialStoreUnavailable",
                    "The protected API credential could not be accessed.",
                )
            })?
            .filter(|key| !key.is_empty())
            .ok_or_else(|| AppError::from_code(ErrorCode::AuthenticationFailed))
    }

    async fn send_with_header_timeout(
        &self,
        request: RequestBuilder,
        cancellation: &CancellationToken,
        api_key: &str,
    ) -> Result<Response, AppError> {
        tokio::select! {
            _ = cancellation.cancelled() => Err(AppError::from_code(ErrorCode::Cancelled)),
            result = timeout(self.timeouts.response_headers, request.send()) => {
                match result {
                    Err(_) => Err(AppError::from_code(ErrorCode::RequestTimeout)),
                    Ok(Err(error)) if error.is_timeout() => {
                        Err(AppError::from_code(ErrorCode::RequestTimeout))
                    }
                    Ok(Err(error)) => Err(
                        AppError::from_code(ErrorCode::ProviderUnavailable)
                            .with_diagnostic(error.to_string(), Some(api_key)),
                    ),
                    Ok(Ok(response)) => Ok(response),
                }
            }
        }
    }

    async fn execute_with_total<T, F>(
        &self,
        cancellation: CancellationToken,
        operation: F,
    ) -> Result<T, AppError>
    where
        F: std::future::Future<Output = Result<T, AppError>>,
    {
        tokio::select! {
            _ = cancellation.cancelled() => Err(AppError::from_code(ErrorCode::Cancelled)),
            result = timeout(self.timeouts.total_stream, operation) => {
                result.unwrap_or_else(|_| Err(AppError::from_code(ErrorCode::RequestTimeout)))
            }
        }
    }

    async fn checked_body(
        &self,
        response: Response,
        api_key: &str,
        chat_or_model_endpoint: bool,
    ) -> Result<String, AppError> {
        let status = response.status();
        let body = response.text().await.map_err(|error| {
            AppError::from_code(ErrorCode::ProviderUnavailable)
                .with_diagnostic(error.to_string(), Some(api_key))
        })?;

        if status.is_success() {
            Ok(body)
        } else {
            Err(AppError::from_http_body(
                status.as_u16(),
                &body,
                Some(api_key),
                chat_or_model_endpoint,
            ))
        }
    }

    async fn complete_inner(
        &self,
        request: ChatRequest,
        cancellation: &CancellationToken,
    ) -> Result<String, AppError> {
        let api_key = self.api_key().await?;
        let payload = json!({
            "model": self.settings.model,
            "messages": request.messages,
            "stream": false,
        });
        let request = self
            .http
            .post(self.endpoint("chat/completions"))
            .bearer_auth(&api_key)
            .json(&payload);
        let response = self
            .send_with_header_timeout(request, cancellation, &api_key)
            .await?;
        let body = self.checked_body(response, &api_key, true).await?;
        parse_chat_completion(&body)
    }

    async fn connection_fallback(
        &self,
        api_key: &str,
        cancellation: &CancellationToken,
    ) -> Result<(), AppError> {
        let payload = json!({
            "model": self.settings.model,
            "messages": [{"role": "user", "content": "回复 OK"}],
            "stream": false,
            "max_tokens": 1,
        });
        let request = self
            .http
            .post(self.endpoint("chat/completions"))
            .bearer_auth(api_key)
            .json(&payload);
        let response = self
            .send_with_header_timeout(request, cancellation, api_key)
            .await?;
        let body = self.checked_body(response, api_key, true).await?;
        parse_connection_response(&body)
    }
}

#[async_trait]
impl LlmTransport for OpenAiClient {
    async fn stream_chat(
        &self,
        request: ChatRequest,
        sink: &dyn DeltaSink,
        cancellation: CancellationToken,
    ) -> Result<(), AppError> {
        let operation = async {
            let api_key = self.api_key().await?;
            let payload = json!({
                "model": self.settings.model,
                "messages": request.messages,
                "stream": true,
            });
            let request = self
                .http
                .post(self.endpoint("chat/completions"))
                .bearer_auth(&api_key)
                .json(&payload);
            let response = self
                .send_with_header_timeout(request, &cancellation, &api_key)
                .await?;
            if !response.status().is_success() {
                return self
                    .checked_body(response, &api_key, true)
                    .await
                    .map(|_| ());
            }

            let mut decoder = SseDecoder::default();
            let mut stream = response.bytes_stream();
            loop {
                let chunk = tokio::select! {
                    _ = cancellation.cancelled() => {
                        return Err(AppError::from_code(ErrorCode::Cancelled));
                    }
                    chunk = stream.next() => chunk,
                };
                let Some(chunk) = chunk else {
                    break;
                };
                let chunk = chunk.map_err(|error| {
                    AppError::from_code(ErrorCode::ProviderUnavailable)
                        .with_diagnostic(error.to_string(), Some(&api_key))
                })?;
                for delta in decoder.push(&chunk)? {
                    sink.send(&delta).await?;
                }
                if decoder.is_done() {
                    break;
                }
            }
            for delta in decoder.finish()? {
                sink.send(&delta).await?;
            }
            Ok(())
        };

        self.execute_with_total(cancellation.clone(), operation)
            .await
    }

    async fn complete(
        &self,
        request: ChatRequest,
        cancellation: CancellationToken,
    ) -> Result<String, AppError> {
        let operation = self.complete_inner(request, &cancellation);
        self.execute_with_total(cancellation.clone(), operation)
            .await
    }

    async fn try_native_web(
        &self,
        request: NativeWebRequest,
        cancellation: CancellationToken,
    ) -> Result<NativeWebOutcome, AppError> {
        let operation = async {
            let api_key = self.api_key().await?;
            let payload = json!({
                "model": self.settings.model,
                "input": request.input,
                "tools": [{"type": "web_search"}],
                "include": ["web_search_call.action.sources"],
            });
            let request = self
                .http
                .post(self.endpoint("responses"))
                .bearer_auth(&api_key)
                .json(&payload);
            let response = self
                .send_with_header_timeout(request, &cancellation, &api_key)
                .await?;
            let status = response.status();
            let body = response.text().await.map_err(|error| {
                AppError::from_code(ErrorCode::ProviderUnavailable)
                    .with_diagnostic(error.to_string(), Some(&api_key))
            })?;

            if is_unsupported(status, &body) {
                return Ok(NativeWebOutcome::Unsupported);
            }
            if !status.is_success() {
                return Err(AppError::from_http_body(
                    status.as_u16(),
                    &body,
                    Some(&api_key),
                    false,
                ));
            }

            parse_native_web_response(&body)
        };

        self.execute_with_total(cancellation.clone(), operation)
            .await
    }

    async fn test_connection(&self, cancellation: CancellationToken) -> Result<(), AppError> {
        let mut retried = 0usize;
        loop {
            match self.test_connection_once(&cancellation).await {
                Ok(()) => return Ok(()),
                Err(error) if retried < MAX_CONNECTION_ATTEMPTS && is_transient_error(&error) => {
                    retried += 1;
                    tokio::select! {
                        _ = cancellation.cancelled() => {
                            return Err(AppError::from_code(ErrorCode::Cancelled));
                        }
                        _ = tokio::time::sleep(CONNECTION_RETRY_DELAY) => {}
                    }
                }
                Err(error) => return Err(error),
            }
        }
    }

    async fn list_models(&self) -> Result<Vec<String>, AppError> {
        let operation = async {
            let api_key = self.api_key().await?;
            let request = self
                .http
                .get(self.endpoint("models"))
                .bearer_auth(&api_key);
            let response = self
                .send_with_header_timeout(request, &CancellationToken::new(), &api_key)
                .await?;
            let status = response.status();
            let body = response.text().await.map_err(|error| {
                AppError::from_code(ErrorCode::ProviderUnavailable)
                    .with_diagnostic(error.to_string(), Some(&api_key))
            })?;

            if status.is_success() {
                let value: Value = serde_json::from_str(&body).map_err(invalid_response)?;
                let models = value
                    .get("data")
                    .and_then(Value::as_array)
                    .ok_or_else(|| invalid_response("missing models data array"))?;
                let mut ids: Vec<String> = models
                    .iter()
                    .filter_map(|model| model.get("id").and_then(Value::as_str).map(str::to_owned))
                    .collect();
                ids.sort();
                ids.dedup();
                return Ok(ids);
            }
            if is_unsupported(status, &body) {
                return Err(AppError::new(
                    "modelsUnsupported",
                    "This provider does not expose a model list.",
                ));
            }
            Err(AppError::from_http_body(
                status.as_u16(),
                &body,
                Some(&api_key),
                true,
            ))
        };

        self.execute_with_total(CancellationToken::new(), operation)
            .await
    }
}

fn is_unsupported(status: StatusCode, body: &str) -> bool {
    if matches!(status.as_u16(), 404 | 405) {
        return true;
    }
    if !matches!(status.as_u16(), 400 | 422) {
        return false;
    }

    let message = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|value| {
            value
                .pointer("/error/message")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
        .unwrap_or_else(|| body.to_owned())
        .to_ascii_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let subjects = [
        "endpoint",
        "tool",
        "web_search",
        "web search",
        "responses api",
        "responses endpoint",
    ];

    subjects.iter().any(|subject| {
        [
            format!("unsupported {subject}"),
            format!("{subject} is unsupported"),
            format!("{subject} is not supported"),
            format!("{subject} is not implemented"),
            format!("does not support {subject}"),
            format!("does not support the {subject}"),
        ]
        .iter()
        .any(|phrase| message.contains(phrase))
    })
}

fn parse_chat_completion(body: &str) -> Result<String, AppError> {
    let value: Value = serde_json::from_str(body).map_err(invalid_response)?;
    value
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .ok_or_else(|| invalid_response("missing chat completion content"))
}

fn parse_connection_response(body: &str) -> Result<(), AppError> {
    let value: Value = serde_json::from_str(body).map_err(invalid_response)?;
    value
        .pointer("/choices/0/message")
        .and_then(Value::as_object)
        .map(|_| ())
        .ok_or_else(|| invalid_response("missing chat completion message"))
}

fn parse_models_ids(body: &str) -> Result<Vec<String>, AppError> {
    let value: Value = serde_json::from_str(body).map_err(invalid_response)?;
    let models = value
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid_response("missing models data array"))?;
    Ok(models
        .iter()
        .filter_map(|model| model.get("id").and_then(Value::as_str).map(str::to_owned))
        .collect())
}

fn parse_response_text_value(value: &Value) -> Result<String, AppError> {
    if let Some(text) = value.get("output_text").and_then(Value::as_str) {
        return Ok(text.to_owned());
    }

    let text = value
        .get("output")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .flat_map(|item| {
            item.get("content")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
        })
        .filter_map(|content| content.get("text").and_then(Value::as_str))
        .collect::<String>();
    if text.is_empty() {
        Err(invalid_response("missing response output text"))
    } else {
        Ok(text)
    }
}

fn parse_native_web_response(body: &str) -> Result<NativeWebOutcome, AppError> {
    let value: Value = serde_json::from_str(body).map_err(invalid_response)?;
    let text = parse_response_text_value(&value)?;
    let mut sources = Vec::new();

    for source in value
        .get("output")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| item.get("action"))
        .filter_map(|action| action.get("sources"))
        .filter_map(Value::as_array)
        .flatten()
    {
        let Some(title) = source.get("title").and_then(Value::as_str) else {
            continue;
        };
        let Some(url) = source.get("url").and_then(Value::as_str) else {
            continue;
        };
        let Some(source) = OutingSource::from_untrusted(title, url) else {
            continue;
        };
        if sources
            .iter()
            .all(|existing: &OutingSource| existing.url != source.url)
        {
            sources.push(source);
        }
    }

    Ok(NativeWebOutcome::Completed { text, sources })
}

fn invalid_response(error: impl std::fmt::Display) -> AppError {
    AppError::from_code(ErrorCode::InvalidResponse)
        .with_diagnostic(format!("invalid provider response: {error}"), None)
}
