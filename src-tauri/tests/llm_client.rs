use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use aibb_desktop_pet_lib::{
    domain::{OutingSource, WebMode},
    error::{AppError, ErrorCode},
    llm::{
        parse_sse_text, ChatMessage, ChatRequest, DeltaSink, LlmTransport, NativeWebOutcome,
        NativeWebRequest, OpenAiClient, SseDecoder, TransportTimeouts,
    },
    settings::{ApiSettings, CredentialStore, SaveSettings, SettingsService},
    storage::Database,
};
use async_trait::async_trait;
use serde_json::json;
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;
use wiremock::{
    matchers::{body_json, header, method, path},
    Mock, MockServer, ResponseTemplate,
};

const SECRET: &str = "sk-task-five-secret";

#[derive(Clone)]
struct FakeCredentialStore {
    value: Arc<tokio::sync::Mutex<Option<String>>>,
}

impl FakeCredentialStore {
    fn with_key(key: &str) -> Self {
        Self {
            value: Arc::new(tokio::sync::Mutex::new(Some(key.to_owned()))),
        }
    }
}

#[async_trait]
impl CredentialStore for FakeCredentialStore {
    async fn get(&self) -> Result<Option<String>, AppError> {
        Ok(self.value.lock().await.clone())
    }

    async fn set(&self, api_key: &str) -> Result<(), AppError> {
        *self.value.lock().await = Some(api_key.to_owned());
        Ok(())
    }

    async fn clear(&self) -> Result<(), AppError> {
        *self.value.lock().await = None;
        Ok(())
    }
}

#[derive(Default)]
struct CollectingSink {
    deltas: Mutex<Vec<String>>,
}

#[async_trait]
impl DeltaSink for CollectingSink {
    async fn send(&self, delta: &str) -> Result<(), AppError> {
        self.deltas.lock().unwrap().push(delta.to_owned());
        Ok(())
    }
}

impl CollectingSink {
    fn values(&self) -> Vec<String> {
        self.deltas.lock().unwrap().clone()
    }
}

struct ConnectionTransport {
    error: Option<ErrorCode>,
}

#[async_trait]
impl LlmTransport for ConnectionTransport {
    async fn stream_chat(
        &self,
        _request: ChatRequest,
        _sink: &dyn DeltaSink,
        _cancellation: CancellationToken,
    ) -> Result<(), AppError> {
        unreachable!("connection verification does not stream chat")
    }

    async fn complete(
        &self,
        _request: ChatRequest,
        _cancellation: CancellationToken,
    ) -> Result<String, AppError> {
        unreachable!("connection verification does not complete chat")
    }

    async fn try_native_web(
        &self,
        _request: NativeWebRequest,
        _cancellation: CancellationToken,
    ) -> Result<NativeWebOutcome, AppError> {
        unreachable!("connection verification does not use native web")
    }

    async fn test_connection(&self, _cancellation: CancellationToken) -> Result<(), AppError> {
        self.error.map(AppError::from_code).map_or(Ok(()), Err)
    }
}

fn settings(api_base: String) -> ApiSettings {
    ApiSettings {
        api_base,
        model: "configured-model".into(),
        web_mode: WebMode::Auto,
        always_on_top: false,
        autostart: false,
        api_configured: true,
    }
}

fn client_for(server: &MockServer) -> OpenAiClient {
    OpenAiClient::new(
        settings(format!("{}/v1", server.uri())),
        FakeCredentialStore::with_key(SECRET),
    )
}

fn request() -> ChatRequest {
    ChatRequest {
        messages: vec![ChatMessage::user("你好")],
    }
}

fn native_request() -> NativeWebRequest {
    NativeWebRequest {
        input: "找一件有趣的事".into(),
    }
}

#[test]
fn parses_chat_completion_sse_and_stops_at_done() {
    let input = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"你\"}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"好\"}}]}\n\n",
        "data: [DONE]\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"不应出现\"}}]}\n\n"
    );

    assert_eq!(parse_sse_text(input).unwrap(), vec!["你", "好"]);
}

#[test]
fn parse_sse_text_rejects_truncated_input_at_eof() {
    let error = parse_sse_text("data: {\"choices\":[").unwrap_err();

    assert_eq!(error.code, ErrorCode::InvalidResponse.as_str());
}

#[test]
fn incremental_sse_accepts_cross_chunk_crlf_and_multiple_data_lines() {
    let mut decoder = SseDecoder::default();
    let chunks: [&[u8]; 4] = [
        b"data: {\"choices\":[\r\n",
        "data: {\"delta\":{\"content\":\"海".as_bytes(),
        "豚\"}}]}\r\n\r\ndata: [DO".as_bytes(),
        b"NE]\r\n\r\ndata: {\"choices\":[{\"delta\":{\"content\":\"ignored\"}}]}\r\n\r\n",
    ];
    let mut deltas = Vec::new();

    for chunk in chunks {
        deltas.extend(decoder.push(chunk).unwrap());
    }

    assert_eq!(deltas, vec!["海豚"]);
    assert!(decoder.is_done());
}

#[test]
fn sse_finish_accepts_a_done_event_without_a_trailing_blank_line() {
    let mut decoder = SseDecoder::default();

    let deltas = decoder
        .push(
            concat!(
                "data: {\"choices\":[{\"delta\":{\"content\":\"尾\"}}]}\n\n",
                "data: [DONE]"
            )
            .as_bytes(),
        )
        .unwrap();
    let final_deltas = decoder.finish().unwrap();

    assert_eq!(deltas, vec!["尾"]);
    assert!(final_deltas.is_empty());
    assert!(decoder.is_done());
}

#[tokio::test]
async fn stream_chat_rejects_truncated_sse_at_eof() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string("data: {\"choices\":[{\"delta\":{\"content\":\"cut"),
        )
        .mount(&server)
        .await;

    let error = client_for(&server)
        .stream_chat(
            request(),
            &CollectingSink::default(),
            CancellationToken::new(),
        )
        .await
        .unwrap_err();

    assert_eq!(error.code, ErrorCode::InvalidResponse.as_str());
}

#[tokio::test]
async fn stream_chat_rejects_a_non_sse_success_body() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"error": {"message": "not a stream"}})),
        )
        .mount(&server)
        .await;

    let error = client_for(&server)
        .stream_chat(
            request(),
            &CollectingSink::default(),
            CancellationToken::new(),
        )
        .await
        .unwrap_err();

    assert_eq!(error.code, ErrorCode::InvalidResponse.as_str());
}

#[tokio::test]
async fn stream_chat_uses_one_trimmed_trailing_slash_and_emits_sse_deltas() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1//chat/completions"))
        .and(header("authorization", format!("Bearer {SECRET}")))
        .and(header("content-type", "application/json"))
        .and(body_json(json!({
            "model": "configured-model",
            "messages": [{"role": "user", "content": "你好"}],
            "stream": true
        })))
        .respond_with(ResponseTemplate::new(200).set_body_string(concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"你\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"好\"}}]}\n\n",
            "data: [DONE]\n\n"
        )))
        .expect(1)
        .mount(&server)
        .await;
    let client = OpenAiClient::new(
        settings(format!("{}/v1//", server.uri())),
        FakeCredentialStore::with_key(SECRET),
    );
    let sink = CollectingSink::default();

    client
        .stream_chat(request(), &sink, CancellationToken::new())
        .await
        .unwrap();

    assert_eq!(sink.values(), vec!["你", "好"]);
}

#[tokio::test]
async fn complete_posts_a_non_streaming_chat_completion() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(body_json(json!({
            "model": "configured-model",
            "messages": [{"role": "user", "content": "你好"}],
            "stream": false
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{"message": {"content": "完成"}}]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let output = client_for(&server)
        .complete(request(), CancellationToken::new())
        .await
        .unwrap();

    assert_eq!(output, "完成");
}

#[tokio::test]
async fn native_web_posts_the_required_tool_and_source_include() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .and(body_json(json!({
            "model": "configured-model",
            "input": "找一件有趣的事",
            "tools": [{"type": "web_search"}],
            "include": ["web_search_call.action.sources"]
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "output_text": "结果",
            "output": [{
                "type": "web_search_call",
                "action": {
                    "sources": [
                        {"type": "url", "title": "  安全来源  ", "url": "https://example.com/article"},
                        {"type": "url", "title": "不安全协议", "url": "http://example.com/plain"},
                        {"type": "url", "title": "脚本", "url": "javascript:alert(1)"},
                        {"type": "url", "title": "   ", "url": "https://example.com/empty-title"}
                    ]
                }
            }]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let outcome = client_for(&server)
        .try_native_web(native_request(), CancellationToken::new())
        .await
        .unwrap();

    assert_eq!(
        outcome,
        NativeWebOutcome::Completed {
            text: "结果".into(),
            sources: vec![OutingSource {
                title: "安全来源".into(),
                url: "https://example.com/article".into(),
            }],
        }
    );
}

#[tokio::test]
async fn responses_endpoint_capability_failures_return_unsupported() {
    for (status, body) in [
        (404, "missing"),
        (405, "method not allowed"),
        (400, "This endpoint is unsupported"),
        (422, "The web_search tool is not supported"),
        (
            400,
            "{\"error\":{\"message\":\"web_search endpoint is not implemented\"}}",
        ),
    ] {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/responses"))
            .respond_with(ResponseTemplate::new(status).set_body_string(body))
            .mount(&server)
            .await;

        let outcome = client_for(&server)
            .try_native_web(native_request(), CancellationToken::new())
            .await
            .unwrap();

        assert_eq!(outcome, NativeWebOutcome::Unsupported, "status {status}");
    }
}

#[tokio::test]
async fn unsupported_input_parameter_is_not_a_capability_failure() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .respond_with(ResponseTemplate::new(422).set_body_json(json!({
            "error": {
                "message": "unsupported input parameter for Responses endpoint"
            }
        })))
        .mount(&server)
        .await;

    let error = client_for(&server)
        .try_native_web(native_request(), CancellationToken::new())
        .await
        .unwrap_err();

    assert_eq!(error.code, ErrorCode::InvalidRequest.as_str());
}

#[tokio::test]
async fn unrelated_bad_request_is_not_misclassified_as_unsupported() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .respond_with(ResponseTemplate::new(400).set_body_string("input is too long"))
        .mount(&server)
        .await;

    let error = client_for(&server)
        .try_native_web(native_request(), CancellationToken::new())
        .await
        .unwrap_err();

    assert_eq!(error.code, ErrorCode::InvalidRequest.as_str());
}

#[tokio::test]
async fn chat_http_failures_have_stable_public_error_codes() {
    for (status, expected) in [
        (401, ErrorCode::AuthenticationFailed),
        (403, ErrorCode::AuthenticationFailed),
        (404, ErrorCode::ModelNotFound),
        (429, ErrorCode::RateLimited),
        (500, ErrorCode::ProviderUnavailable),
        (503, ErrorCode::ProviderUnavailable),
    ] {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(status).set_body_string("provider detail"))
            .mount(&server)
            .await;

        let error = client_for(&server)
            .complete(request(), CancellationToken::new())
            .await
            .unwrap_err();

        assert_eq!(error.code, expected.as_str(), "status {status}");
        assert!(!error.message.contains("provider detail"));
    }
}

#[test]
fn sanitized_http_error_never_serializes_provider_details_or_bearer_secret() {
    let error = AppError::from_http_body(
        401,
        "Authorization: Bearer sk-task-five-secret\nprovider-private-detail",
        Some(SECRET),
        false,
    );
    let serialized = serde_json::to_string(&error).unwrap();

    assert_eq!(error.code, ErrorCode::AuthenticationFailed.as_str());
    assert!(!serialized.contains(SECRET));
    assert!(!serialized.contains("Bearer"));
    assert!(!serialized.contains("provider-private-detail"));
    assert!(error
        .diagnostic()
        .is_some_and(|value| value.contains("[REDACTED]")));
}

#[tokio::test]
async fn test_connection_verifies_the_configured_model_after_models_succeeds() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .and(header("authorization", format!("Bearer {SECRET}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [{"id": "configured-model"}]
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(body_json(json!({
            "model": "configured-model",
            "messages": [{"role": "user", "content": "回复 OK"}],
            "stream": false,
            "max_tokens": 1
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{"message": {"content": "OK"}}]
        })))
        .expect(1)
        .mount(&server)
        .await;

    client_for(&server)
        .test_connection(CancellationToken::new())
        .await
        .unwrap();
}

#[tokio::test]
async fn test_connection_rejects_a_model_missing_from_the_provider_catalog() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [{"id": "other-model"}]
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&server)
        .await;

    let error = client_for(&server)
        .test_connection(CancellationToken::new())
        .await
        .unwrap_err();

    assert_eq!(error.code, ErrorCode::ModelNotFound.as_str());
}

#[tokio::test]
async fn test_connection_rejects_a_non_openai_models_success_body() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .respond_with(ResponseTemplate::new(200).set_body_string("<html>sign in</html>"))
        .expect(1)
        .mount(&server)
        .await;

    let error = client_for(&server)
        .test_connection(CancellationToken::new())
        .await
        .unwrap_err();

    assert_eq!(error.code, ErrorCode::InvalidResponse.as_str());
}

#[tokio::test]
async fn test_connection_falls_back_to_one_token_chat_when_models_is_unsupported() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .respond_with(ResponseTemplate::new(405))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(body_json(json!({
            "model": "configured-model",
            "messages": [{"role": "user", "content": "回复 OK"}],
            "stream": false,
            "max_tokens": 1
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{"message": {"content": "OK"}}]
        })))
        .expect(1)
        .mount(&server)
        .await;

    client_for(&server)
        .test_connection(CancellationToken::new())
        .await
        .unwrap();
}

#[tokio::test]
async fn test_connection_accepts_a_truncated_thinking_response_without_final_text() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [{"id": "configured-model"}]
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{
                "finish_reason": "length",
                "message": {
                    "content": null,
                    "reasoning_content": "正在思考"
                }
            }]
        })))
        .expect(1)
        .mount(&server)
        .await;

    client_for(&server)
        .test_connection(CancellationToken::new())
        .await
        .unwrap();
}

#[tokio::test]
async fn test_connection_rejects_an_invalid_fallback_success_body() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .respond_with(ResponseTemplate::new(404))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "error": {"message": "proxy failure"}
        })))
        .expect(1)
        .mount(&server)
        .await;

    let error = client_for(&server)
        .test_connection(CancellationToken::new())
        .await
        .unwrap_err();

    assert_eq!(error.code, ErrorCode::InvalidResponse.as_str());
}

#[tokio::test]
async fn test_connection_does_not_follow_redirects_with_the_credential() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .respond_with(ResponseTemplate::new(302).insert_header("Location", "/login"))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/login"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": []})))
        .expect(0)
        .mount(&server)
        .await;

    let error = client_for(&server)
        .test_connection(CancellationToken::new())
        .await
        .unwrap_err();

    assert_eq!(error.code, ErrorCode::InvalidRequest.as_str());
}

#[tokio::test]
async fn cancellation_returns_a_stable_error_without_waiting_for_network_timeout() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(Duration::from_secs(2))
                .set_body_string("data: [DONE]\n\n"),
        )
        .mount(&server)
        .await;
    let token = CancellationToken::new();
    token.cancel();

    let result = timeout(
        Duration::from_millis(250),
        client_for(&server).complete(request(), token),
    )
    .await
    .expect("pre-cancelled request must finish promptly")
    .unwrap_err();

    assert_eq!(result.code, ErrorCode::Cancelled.as_str());
}

#[tokio::test]
async fn configured_total_timeout_returns_a_stable_error_in_bounded_time() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(Duration::from_millis(300))
                .set_body_json(json!({"choices": []})),
        )
        .mount(&server)
        .await;
    let client = OpenAiClient::with_timeouts(
        settings(format!("{}/v1", server.uri())),
        FakeCredentialStore::with_key(SECRET),
        TransportTimeouts {
            response_headers: Duration::from_secs(1),
            total_stream: Duration::from_millis(40),
        },
    );

    let error = timeout(Duration::from_millis(500), async {
        client
            .stream_chat(
                request(),
                &CollectingSink::default(),
                CancellationToken::new(),
            )
            .await
    })
    .await
    .expect("configured timeout must bound the operation")
    .unwrap_err();

    assert_eq!(error.code, ErrorCode::RequestTimeout.as_str());
}

#[tokio::test]
async fn configured_response_header_timeout_returns_a_stable_error_in_bounded_time() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(Duration::from_millis(300))
                .set_body_json(json!({"choices": []})),
        )
        .mount(&server)
        .await;
    let client = OpenAiClient::with_timeouts(
        settings(format!("{}/v1", server.uri())),
        FakeCredentialStore::with_key(SECRET),
        TransportTimeouts {
            response_headers: Duration::from_millis(40),
            total_stream: Duration::from_secs(1),
        },
    );

    let error = timeout(
        Duration::from_millis(500),
        client.complete(request(), CancellationToken::new()),
    )
    .await
    .expect("configured header timeout must bound the operation")
    .unwrap_err();

    assert_eq!(error.code, ErrorCode::RequestTimeout.as_str());
}

#[test]
fn production_timeouts_are_twenty_seconds_for_headers_and_ninety_seconds_total() {
    assert_eq!(
        TransportTimeouts::default(),
        TransportTimeouts {
            response_headers: Duration::from_secs(20),
            total_stream: Duration::from_secs(90),
        }
    );
}

#[tokio::test]
async fn settings_does_not_mark_connection_verified_after_failed_authentication() {
    let directory = tempfile::tempdir().unwrap();
    let database = Database::open(directory.path().join("settings.sqlite3")).unwrap();
    let vault = FakeCredentialStore::with_key(SECRET);
    let service = SettingsService::new_with_transport_factory(
        database.clone(),
        vault,
        |_settings, _api_key| -> Arc<dyn LlmTransport> {
            Arc::new(ConnectionTransport {
                error: Some(ErrorCode::AuthenticationFailed),
            })
        },
    );
    service
        .save(SaveSettings {
            api_base: "https://provider.example/v1".into(),
            model: "configured-model".into(),
            api_key: None,
            web_mode: WebMode::Auto,
            always_on_top: false,
            autostart: false,
        })
        .await
        .unwrap();

    let error = service.test_connection().await.unwrap_err();

    assert_eq!(error.code, ErrorCode::AuthenticationFailed.as_str());
    assert!(!database.load_settings().unwrap().first_run_complete);
}

#[tokio::test]
async fn settings_marks_connection_verified_after_valid_models_response() {
    let directory = tempfile::tempdir().unwrap();
    let database = Database::open(directory.path().join("settings.sqlite3")).unwrap();
    let vault = FakeCredentialStore::with_key(SECRET);
    let service = SettingsService::new_with_transport_factory(
        database.clone(),
        vault,
        |_settings, _api_key| -> Arc<dyn LlmTransport> {
            Arc::new(ConnectionTransport { error: None })
        },
    );
    service
        .save(SaveSettings {
            api_base: "https://provider.example/v1".into(),
            model: "configured-model".into(),
            api_key: None,
            web_mode: WebMode::Auto,
            always_on_top: false,
            autostart: false,
        })
        .await
        .unwrap();

    service.test_connection().await.unwrap();

    assert!(database.load_settings().unwrap().first_run_complete);
}
