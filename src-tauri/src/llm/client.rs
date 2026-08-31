use std::sync::Arc;

use async_trait::async_trait;
use futures_util::StreamExt;
use reqwest::{RequestBuilder, Response, StatusCode};
use serde_json::{json, Value};
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

use crate::{
    error::{AppError, ErrorCode},
    settings::{ApiSettings, CredentialStore},
};

use super::{
    ChatRequest, DeltaSink, LlmTransport, NativeWebOutcome, NativeWebRequest, SseDecoder,
    TransportTimeouts,
};

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

    pub(crate) fn from_shared(
        settings: ApiSettings,
        credentials: Arc<dyn CredentialStore>,
    ) -> Self {
        Self::from_shared_with_timeouts(settings, credentials, TransportTimeouts::default())
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
        parse_chat_completion(&body)?;
        Ok(())
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

            Ok(NativeWebOutcome::Completed(parse_response_text(&body)?))
        };

        self.execute_with_total(cancellation.clone(), operation)
            .await
    }

    async fn test_connection(&self, cancellation: CancellationToken) -> Result<(), AppError> {
        let operation = async {
            let api_key = self.api_key().await?;
            let request = self.http.get(self.endpoint("models")).bearer_auth(&api_key);
            let response = self
                .send_with_header_timeout(request, &cancellation, &api_key)
                .await?;
            let status = response.status();
            let body = response.text().await.map_err(|error| {
                AppError::from_code(ErrorCode::ProviderUnavailable)
                    .with_diagnostic(error.to_string(), Some(&api_key))
            })?;

            if status.is_success() {
                return parse_models_response(&body);
            }
            if is_unsupported(status, &body) {
                return self.connection_fallback(&api_key, &cancellation).await;
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

fn parse_models_response(body: &str) -> Result<(), AppError> {
    let value: Value = serde_json::from_str(body).map_err(invalid_response)?;
    value
        .get("data")
        .and_then(Value::as_array)
        .map(|_| ())
        .ok_or_else(|| invalid_response("missing models data array"))
}

fn parse_response_text(body: &str) -> Result<String, AppError> {
    let value: Value = serde_json::from_str(body).map_err(invalid_response)?;
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

fn invalid_response(error: impl std::fmt::Display) -> AppError {
    AppError::from_code(ErrorCode::InvalidResponse)
        .with_diagnostic(format!("invalid provider response: {error}"), None)
}
