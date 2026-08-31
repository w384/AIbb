use std::{
    collections::{HashMap, VecDeque},
    future::pending,
    net::IpAddr,
    sync::{Arc, Mutex},
    time::Duration,
};

use aibb_desktop_pet_lib::{
    error::{AppError, ErrorCode},
    web::{
        resolve_public_target, validate_url, BodyStream, DnsResolver, DuckDuckGoHtmlSearch,
        FetchBudget, FetchedPage, HttpConnector, HttpResponse, PageFetcher, ResolvedTarget,
        SafePageFetcher, SearchProvider,
    },
};
use async_trait::async_trait;
use futures_util::stream;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
use wiremock::{
    matchers::{header, method, path, query_param},
    Mock, MockServer, ResponseTemplate,
};

#[derive(Default)]
struct FakeResolver {
    answers: HashMap<String, Vec<IpAddr>>,
    calls: Mutex<Vec<String>>,
}

impl FakeResolver {
    fn with_answer(host: &str, addresses: &[&str]) -> Self {
        Self {
            answers: HashMap::from([(
                host.to_owned(),
                addresses
                    .iter()
                    .map(|address| address.parse().unwrap())
                    .collect(),
            )]),
            calls: Mutex::new(Vec::new()),
        }
    }

    fn with_answers(answers: &[(&str, &[&str])]) -> Self {
        Self {
            answers: answers
                .iter()
                .map(|(host, addresses)| {
                    (
                        (*host).to_owned(),
                        addresses
                            .iter()
                            .map(|address| address.parse().unwrap())
                            .collect(),
                    )
                })
                .collect(),
            calls: Mutex::new(Vec::new()),
        }
    }

    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
}

#[async_trait]
impl DnsResolver for FakeResolver {
    async fn resolve(
        &self,
        host: &str,
        _port: u16,
        _cancellation: &CancellationToken,
    ) -> Result<Vec<IpAddr>, aibb_desktop_pet_lib::error::AppError> {
        self.calls.lock().unwrap().push(host.to_owned());
        Ok(self.answers.get(host).cloned().unwrap_or_default())
    }
}

struct PendingResolver {
    entered: Arc<Notify>,
}

struct SequenceResolver {
    answers: Mutex<VecDeque<Vec<IpAddr>>>,
}

impl SequenceResolver {
    fn new(answers: &[&[&str]]) -> Self {
        Self {
            answers: Mutex::new(
                answers
                    .iter()
                    .map(|addresses| {
                        addresses
                            .iter()
                            .map(|address| address.parse().unwrap())
                            .collect()
                    })
                    .collect(),
            ),
        }
    }
}

#[async_trait]
impl DnsResolver for SequenceResolver {
    async fn resolve(
        &self,
        _host: &str,
        _port: u16,
        _cancellation: &CancellationToken,
    ) -> Result<Vec<IpAddr>, AppError> {
        Ok(self.answers.lock().unwrap().pop_front().unwrap())
    }
}

#[async_trait]
impl DnsResolver for PendingResolver {
    async fn resolve(
        &self,
        _host: &str,
        _port: u16,
        _cancellation: &CancellationToken,
    ) -> Result<Vec<IpAddr>, AppError> {
        self.entered.notify_one();
        pending().await
    }
}

enum ConnectorStep {
    Response(HttpResponse),
    Pending(Arc<Notify>),
}

#[derive(Default)]
struct FakeConnector {
    steps: Mutex<VecDeque<ConnectorStep>>,
    targets: Mutex<Vec<ResolvedTarget>>,
}

impl FakeConnector {
    fn with_steps(steps: Vec<ConnectorStep>) -> Self {
        Self {
            steps: Mutex::new(steps.into()),
            targets: Mutex::new(Vec::new()),
        }
    }

    fn targets(&self) -> Vec<ResolvedTarget> {
        self.targets.lock().unwrap().clone()
    }
}

#[async_trait]
impl HttpConnector for FakeConnector {
    async fn get(
        &self,
        target: &ResolvedTarget,
        _cancellation: &CancellationToken,
    ) -> Result<HttpResponse, AppError> {
        self.targets.lock().unwrap().push(target.clone());
        let step = self.steps.lock().unwrap().pop_front().unwrap();
        match step {
            ConnectorStep::Response(response) => Ok(response),
            ConnectorStep::Pending(entered) => {
                entered.notify_one();
                pending().await
            }
        }
    }
}

fn body(chunks: Vec<Result<Vec<u8>, AppError>>) -> BodyStream {
    Box::pin(stream::iter(chunks))
}

fn response(
    status: u16,
    content_type: Option<&str>,
    location: Option<&str>,
    chunks: Vec<Result<Vec<u8>, AppError>>,
) -> HttpResponse {
    HttpResponse::new(
        status,
        content_type.map(ToOwned::to_owned),
        location.map(ToOwned::to_owned),
        body(chunks),
    )
}

fn html_response(html: &str) -> HttpResponse {
    response(
        200,
        Some("text/html; charset=utf-8"),
        None,
        vec![Ok(html.as_bytes().to_vec())],
    )
}

async fn fetch_encoded_page(content_type: &str, bytes: Vec<u8>) -> FetchedPage {
    let resolver = Arc::new(FakeResolver::with_answer(
        "encoding.example",
        &["93.184.216.34"],
    ));
    let connector = Arc::new(FakeConnector::with_steps(vec![ConnectorStep::Response(
        response(200, Some(content_type), None, vec![Ok(bytes)]),
    )]));

    fetcher(resolver, connector, CancellationToken::new())
        .fetch("https://encoding.example/article")
        .await
        .unwrap()
}

fn fetcher(
    resolver: Arc<dyn DnsResolver>,
    connector: Arc<dyn HttpConnector>,
    cancellation: CancellationToken,
) -> SafePageFetcher {
    SafePageFetcher::with_components(
        resolver,
        connector,
        cancellation,
        FetchBudget::new(),
        Duration::from_secs(12),
    )
}

#[test]
fn blocks_literal_local_private_link_local_metadata_and_non_http_targets() {
    for url in [
        "http://0.1.2.3/",
        "http://10.0.0.8/",
        "http://100.64.0.1/",
        "http://127.0.0.1/",
        "http://127.1/",
        "http://0177.0.0.1/",
        "http://0x7f000001/",
        "http://2130706433/",
        "http://100.100.100.200/latest/meta-data/",
        "http://168.63.129.16/metadata/instance",
        "http://169.254.169.254/latest/meta-data/",
        "http://172.16.0.1/",
        "http://192.0.2.1/",
        "http://192.168.0.1/",
        "http://198.18.0.1/",
        "http://198.51.100.1/",
        "http://203.0.113.1/",
        "http://224.0.0.1/",
        "http://240.0.0.1/",
        "http://[::]/",
        "http://[::1]/",
        "http://[::ffff:127.0.0.1]/",
        "http://[64:ff9b::7f00:1]/",
        "http://[100:0:0:1::1]/",
        "http://[100::1]/",
        "http://[2001:2::1]/",
        "http://[2001:db8::1]/",
        "http://[2002:7f00:1::]/",
        "http://[3fff::1]/",
        "http://[400::1]/",
        "http://[fc00::1]/",
        "http://[fd00:ec2::254]/latest/meta-data/",
        "http://[fe80::1]/",
        "http://[ff00::1]/",
        "file:///etc/passwd",
        "ftp://example.com/file",
    ] {
        let error = validate_url(url).unwrap_err();
        assert_eq!(error.code, ErrorCode::UnsafeUrl.as_str(), "{url}");
    }
}

#[test]
fn rejects_userinfo_and_non_default_ports_but_accepts_public_default_targets() {
    for url in [
        "https://user@example.com/",
        "https://user:password@example.com/",
        "https://example.com:444/",
        "http://example.com:8080/",
    ] {
        assert_eq!(
            validate_url(url).unwrap_err().code,
            ErrorCode::UnsafeUrl.as_str(),
            "{url}"
        );
    }

    assert_eq!(
        validate_url("https://example.com:443/path").unwrap().port(),
        None
    );
    assert_eq!(
        validate_url("http://8.8.8.8/").unwrap().host_str(),
        Some("8.8.8.8")
    );
}

#[tokio::test]
async fn rejects_the_entire_dns_answer_when_any_candidate_is_dangerous() {
    let resolver = Arc::new(FakeResolver::with_answer(
        "mixed.example",
        &["93.184.216.34", "10.0.0.9"],
    ));
    let url = validate_url("https://mixed.example/article").unwrap();

    let error = resolve_public_target(url, resolver, &CancellationToken::new())
        .await
        .unwrap_err();

    assert_eq!(error.code, ErrorCode::UnsafeUrl.as_str());
}

#[tokio::test]
async fn rejects_unallocated_or_reserved_ipv6_returned_by_dns() {
    for address in ["100:0:0:1::1", "400::1"] {
        let resolver = Arc::new(FakeResolver::with_answer("reserved.example", &[address]));
        let url = validate_url("https://reserved.example/article").unwrap();

        let error = resolve_public_target(url, resolver, &CancellationToken::new())
            .await
            .unwrap_err();

        assert_eq!(error.code, ErrorCode::UnsafeUrl.as_str(), "{address}");
    }
}

#[tokio::test]
async fn accepts_all_public_dns_answers_and_preserves_them_for_pinning() {
    let resolver = Arc::new(FakeResolver::with_answer(
        "public.example",
        &["93.184.216.34", "2606:4700:4700::1111"],
    ));
    let url = validate_url("https://public.example/article").unwrap();

    let target = resolve_public_target(url, resolver, &CancellationToken::new())
        .await
        .unwrap();

    assert_eq!(target.host(), "public.example");
    assert_eq!(target.port(), 443);
    assert_eq!(
        target.addresses(),
        &[
            "93.184.216.34".parse::<IpAddr>().unwrap(),
            "2606:4700:4700::1111".parse::<IpAddr>().unwrap(),
        ]
    );
}

#[tokio::test]
async fn resolves_before_request_and_passes_every_validated_address_to_the_connector() {
    let resolver = Arc::new(FakeResolver::with_answer(
        "pin.example",
        &["93.184.216.34", "2606:4700:4700::1111"],
    ));
    let connector = Arc::new(FakeConnector::with_steps(vec![ConnectorStep::Response(
        html_response("<html><title>Pinned</title><body>safe page</body></html>"),
    )]));
    let fetcher = fetcher(resolver, connector.clone(), CancellationToken::new());

    let page = fetcher.fetch("https://pin.example/article").await.unwrap();

    assert_eq!(page.title, "Pinned");
    assert_eq!(connector.targets().len(), 1);
    assert_eq!(
        connector.targets()[0].addresses(),
        &[
            "93.184.216.34".parse::<IpAddr>().unwrap(),
            "2606:4700:4700::1111".parse::<IpAddr>().unwrap(),
        ]
    );
}

#[tokio::test]
async fn revalidates_dns_before_following_each_redirect() {
    let resolver = Arc::new(FakeResolver::with_answers(&[
        ("start.example", &["93.184.216.34"]),
        ("mixed.example", &["93.184.216.35", "10.0.0.7"]),
    ]));
    let connector = Arc::new(FakeConnector::with_steps(vec![ConnectorStep::Response(
        response(
            302,
            Some("text/html"),
            Some("https://mixed.example/private"),
            Vec::new(),
        ),
    )]));
    let fetcher = fetcher(
        resolver.clone(),
        connector.clone(),
        CancellationToken::new(),
    );

    let error = fetcher
        .fetch("https://start.example/article")
        .await
        .unwrap_err();

    assert_eq!(error.code, ErrorCode::UnsafeUrl.as_str());
    assert_eq!(resolver.calls(), vec!["start.example", "mixed.example"]);
    assert_eq!(connector.targets().len(), 1);
}

#[tokio::test]
async fn re_resolves_the_same_hostname_after_a_redirect_to_prevent_rebinding() {
    let resolver = Arc::new(SequenceResolver::new(&[&["93.184.216.34"], &["127.0.0.1"]]));
    let connector = Arc::new(FakeConnector::with_steps(vec![ConnectorStep::Response(
        response(
            302,
            Some("text/html"),
            Some("/private-after-rebind"),
            Vec::new(),
        ),
    )]));
    let fetcher = fetcher(resolver, connector.clone(), CancellationToken::new());

    let error = fetcher
        .fetch("https://same-host.example/start")
        .await
        .unwrap_err();

    assert_eq!(error.code, ErrorCode::UnsafeUrl.as_str());
    assert_eq!(connector.targets().len(), 1);
}

#[tokio::test]
async fn follows_at_most_three_redirects() {
    let resolver = Arc::new(FakeResolver::with_answers(&[
        ("one.example", &["93.184.216.31"]),
        ("two.example", &["93.184.216.32"]),
        ("three.example", &["93.184.216.33"]),
        ("four.example", &["93.184.216.34"]),
    ]));
    let connector = Arc::new(FakeConnector::with_steps(vec![
        ConnectorStep::Response(response(
            301,
            Some("text/html"),
            Some("https://two.example/"),
            Vec::new(),
        )),
        ConnectorStep::Response(response(
            302,
            Some("text/html"),
            Some("https://three.example/"),
            Vec::new(),
        )),
        ConnectorStep::Response(response(
            307,
            Some("text/html"),
            Some("https://four.example/"),
            Vec::new(),
        )),
        ConnectorStep::Response(response(
            308,
            Some("text/html"),
            Some("https://five.example/"),
            Vec::new(),
        )),
    ]));
    let fetcher = fetcher(resolver, connector.clone(), CancellationToken::new());

    let error = fetcher.fetch("https://one.example/").await.unwrap_err();

    assert_eq!(error.code, ErrorCode::RedirectLimitExceeded.as_str());
    assert_eq!(connector.targets().len(), 4);
}

#[tokio::test]
async fn enforces_streamed_body_limit_without_trusting_content_length() {
    let resolver = Arc::new(FakeResolver::with_answer(
        "large.example",
        &["93.184.216.34"],
    ));
    let connector = Arc::new(FakeConnector::with_steps(vec![ConnectorStep::Response(
        response(
            200,
            Some("text/plain"),
            None,
            vec![Ok(vec![b'a'; 1_999_999]), Ok(vec![b'b'; 2])],
        ),
    )]));
    let fetcher = fetcher(resolver, connector, CancellationToken::new());

    let error = fetcher.fetch("https://large.example/").await.unwrap_err();

    assert_eq!(error.code, ErrorCode::ResponseTooLarge.as_str());
}

#[tokio::test]
async fn accepts_allowed_content_types_case_insensitively_with_parameters() {
    for content_type in [
        "TeXt/HtMl; Charset=UTF-8",
        "TEXT/PLAIN; charset=utf-8",
        "Application/XHtml+Xml; CHARSET=UTF-8",
    ] {
        let resolver = Arc::new(FakeResolver::with_answer(
            "content.example",
            &["93.184.216.34"],
        ));
        let connector = Arc::new(FakeConnector::with_steps(vec![ConnectorStep::Response(
            response(200, Some(content_type), None, vec![Ok(b"allowed".to_vec())]),
        )]));
        let fetcher = fetcher(resolver, connector, CancellationToken::new());

        assert_eq!(
            fetcher
                .fetch("https://content.example/")
                .await
                .unwrap()
                .text,
            "allowed",
            "{content_type}"
        );
    }
}

#[tokio::test]
async fn rejects_missing_binary_or_unlisted_content_types() {
    for content_type in [
        None,
        Some("application/octet-stream"),
        Some("image/svg+xml"),
    ] {
        let resolver = Arc::new(FakeResolver::with_answer(
            "binary.example",
            &["93.184.216.34"],
        ));
        let connector = Arc::new(FakeConnector::with_steps(vec![ConnectorStep::Response(
            response(200, content_type, None, vec![Ok(vec![0, 1, 2])]),
        )]));
        let fetcher = fetcher(resolver, connector, CancellationToken::new());

        assert_eq!(
            fetcher
                .fetch("https://binary.example/")
                .await
                .unwrap_err()
                .code,
            ErrorCode::UnsupportedContent.as_str()
        );
    }
}

#[tokio::test]
async fn cancellation_interrupts_dns_resolution() {
    let entered = Arc::new(Notify::new());
    let cancellation = CancellationToken::new();
    let fetcher = fetcher(
        Arc::new(PendingResolver {
            entered: entered.clone(),
        }),
        Arc::new(FakeConnector::default()),
        cancellation.clone(),
    );
    let task = tokio::spawn(async move { fetcher.fetch("https://dns.example/").await });
    entered.notified().await;

    cancellation.cancel();
    let error = tokio::time::timeout(Duration::from_millis(200), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap_err();

    assert_eq!(error.code, ErrorCode::Cancelled.as_str());
}

#[tokio::test]
async fn cancellation_interrupts_the_http_request() {
    let entered = Arc::new(Notify::new());
    let cancellation = CancellationToken::new();
    let fetcher = fetcher(
        Arc::new(FakeResolver::with_answer(
            "request.example",
            &["93.184.216.34"],
        )),
        Arc::new(FakeConnector::with_steps(vec![ConnectorStep::Pending(
            entered.clone(),
        )])),
        cancellation.clone(),
    );
    let task = tokio::spawn(async move { fetcher.fetch("https://request.example/").await });
    entered.notified().await;

    cancellation.cancel();
    let error = tokio::time::timeout(Duration::from_millis(200), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap_err();

    assert_eq!(error.code, ErrorCode::Cancelled.as_str());
}

#[tokio::test]
async fn cancellation_interrupts_response_body_streaming() {
    let entered = Arc::new(Notify::new());
    let body_entered = entered.clone();
    let pending_body: BodyStream = Box::pin(stream::once(async move {
        body_entered.notify_one();
        pending::<Result<Vec<u8>, AppError>>().await
    }));
    let cancellation = CancellationToken::new();
    let fetcher = fetcher(
        Arc::new(FakeResolver::with_answer(
            "body.example",
            &["93.184.216.34"],
        )),
        Arc::new(FakeConnector::with_steps(vec![ConnectorStep::Response(
            HttpResponse::new(200, Some("text/html".into()), None, pending_body),
        )])),
        cancellation.clone(),
    );
    let task = tokio::spawn(async move { fetcher.fetch("https://body.example/").await });
    entered.notified().await;

    cancellation.cancel();
    let error = tokio::time::timeout(Duration::from_millis(200), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap_err();

    assert_eq!(error.code, ErrorCode::Cancelled.as_str());
}

#[tokio::test]
async fn page_timeout_interrupts_dns_resolution() {
    let fetcher = SafePageFetcher::with_components(
        Arc::new(PendingResolver {
            entered: Arc::new(Notify::new()),
        }),
        Arc::new(FakeConnector::default()),
        CancellationToken::new(),
        FetchBudget::new(),
        Duration::from_millis(25),
    );

    let error = fetcher.fetch("https://timeout.example/").await.unwrap_err();

    assert_eq!(error.code, ErrorCode::RequestTimeout.as_str());
}

#[tokio::test]
async fn page_timeout_interrupts_response_body_streaming() {
    let pending_body: BodyStream = Box::pin(stream::pending());
    let fetcher = SafePageFetcher::with_components(
        Arc::new(FakeResolver::with_answer(
            "slow-body.example",
            &["93.184.216.34"],
        )),
        Arc::new(FakeConnector::with_steps(vec![ConnectorStep::Response(
            HttpResponse::new(200, Some("text/html".into()), None, pending_body),
        )])),
        CancellationToken::new(),
        FetchBudget::new(),
        Duration::from_millis(25),
    );

    let error = fetcher
        .fetch("https://slow-body.example/")
        .await
        .unwrap_err();

    assert_eq!(error.code, ErrorCode::RequestTimeout.as_str());
}

#[tokio::test]
async fn one_exploration_budget_allows_at_most_eight_page_fetches() {
    let resolver = Arc::new(FakeResolver::with_answer(
        "budget.example",
        &["93.184.216.34"],
    ));
    let connector = Arc::new(FakeConnector::with_steps(
        (0..8)
            .map(|_| ConnectorStep::Response(html_response("<body>page</body>")))
            .collect(),
    ));
    let fetcher = fetcher(resolver, connector.clone(), CancellationToken::new());

    for index in 0..8 {
        fetcher
            .fetch(&format!("https://budget.example/{index}"))
            .await
            .unwrap();
    }
    let error = fetcher
        .fetch("https://budget.example/ninth")
        .await
        .unwrap_err();

    assert_eq!(error.code, ErrorCode::PageBudgetExceeded.as_str());
    assert_eq!(connector.targets().len(), 8);
}

#[tokio::test]
async fn extracts_title_safe_canonical_and_only_visible_page_text() {
    let resolver = Arc::new(FakeResolver::with_answer(
        "extract.example",
        &["93.184.216.34"],
    ));
    let html = r#"
        <html>
          <head>
            <title>  Visible   title </title>
            <link rel="canonical alternate" href="/canonical-page">
            <style>.secret { display: block; }</style>
            <script>script secret</script>
          </head>
          <body>
            leading   visible
            <nav>navigation secret</nav>
            <form>form secret</form>
            <section hidden>hidden secret</section>
            <section aria-hidden="TRUE">aria secret</section>
            <section style="DISPLAY: none">display secret</section>
            <section style="display: none !important">important display secret</section>
            <section style="visibility: hidden">visibility secret</section>
            <section style="visibility: collapse!important">important visibility secret</section>
            <template>template secret</template>
            <noscript>noscript secret</noscript>
            <iframe>iframe secret</iframe>
            trailing visible
          </body>
        </html>
    "#;
    let connector = Arc::new(FakeConnector::with_steps(vec![ConnectorStep::Response(
        html_response(html),
    )]));
    let fetcher = fetcher(resolver, connector, CancellationToken::new());

    let page = fetcher
        .fetch("https://extract.example/original")
        .await
        .unwrap();

    assert_eq!(page.title, "Visible title");
    assert_eq!(page.canonical_url, "https://extract.example/canonical-page");
    assert_eq!(page.text, "leading visible trailing visible");
}

#[tokio::test]
async fn ignores_non_http_canonical_urls() {
    let resolver = Arc::new(FakeResolver::with_answer(
        "canonical.example",
        &["93.184.216.34"],
    ));
    let connector = Arc::new(FakeConnector::with_steps(vec![ConnectorStep::Response(
        html_response(
            "<html><head><link rel='canonical' href='javascript:alert(1)'></head><body>safe</body></html>",
        ),
    )]));
    let fetcher = fetcher(resolver, connector, CancellationToken::new());

    let page = fetcher
        .fetch("https://canonical.example/original")
        .await
        .unwrap();

    assert_eq!(page.canonical_url, "https://canonical.example/original");
}

#[tokio::test]
async fn html_uses_meta_charset_when_the_http_header_has_none() {
    let page = fetch_encoded_page(
        "text/html",
        b"<html><head><meta charset='windows-1252'></head><body>caf\xe9</body></html>".to_vec(),
    )
    .await;

    assert_eq!(page.text, "café");
}

#[tokio::test]
async fn html_bom_takes_priority_over_http_and_meta_charsets() {
    let mut bytes =
        b"\xef\xbb\xbf<html><head><meta charset='windows-1252'></head><body>caf".to_vec();
    bytes.extend_from_slice("é</body></html>".as_bytes());

    let page = fetch_encoded_page("text/html; charset=windows-1252", bytes).await;

    assert_eq!(page.text, "café");
}

#[tokio::test]
async fn html_http_charset_takes_priority_over_meta_charset() {
    let page = fetch_encoded_page(
        "text/html; charset=windows-1252",
        b"<html><head><meta charset='utf-8'></head><body>caf\xe9</body></html>".to_vec(),
    )
    .await;

    assert_eq!(page.text, "café");
}

#[tokio::test]
async fn html_uses_http_equiv_meta_charset() {
    let page = fetch_encoded_page(
        "text/html",
        b"<html><head><meta http-equiv='content-type' content='text/html; charset=windows-1252'></head><body>caf\xe9</body></html>".to_vec(),
    )
    .await;

    assert_eq!(page.text, "café");
}

#[tokio::test]
async fn html_without_an_encoding_declaration_defaults_to_windows_1252() {
    let page = fetch_encoded_page("text/html", b"<html><body>caf\xe9</body></html>".to_vec()).await;

    assert_eq!(page.text, "café");
}

#[tokio::test]
async fn html_meta_sniff_reads_only_the_first_1024_bytes() {
    let mut bytes = b"<html><head>".to_vec();
    bytes.extend(std::iter::repeat_n(b' ', 1_024));
    bytes.extend_from_slice(b"<meta charset='utf-8'></head><body>caf\xe9</body></html>");

    let page = fetch_encoded_page("text/html", bytes).await;

    assert_eq!(page.text, "café");
}

#[tokio::test]
async fn plain_text_and_xhtml_without_charset_keep_utf8_defaults() {
    for (content_type, bytes) in [
        ("text/plain", "café".as_bytes().to_vec()),
        (
            "application/xhtml+xml",
            "<html><body>café</body></html>".as_bytes().to_vec(),
        ),
    ] {
        let page = fetch_encoded_page(content_type, bytes).await;

        assert_eq!(page.text, "café", "{content_type}");
    }
}

#[tokio::test]
async fn decodes_declared_charset_and_caps_text_at_twelve_thousand_unicode_characters() {
    let resolver = Arc::new(FakeResolver::with_answer(
        "unicode.example",
        &["93.184.216.34"],
    ));
    let long_unicode = "界".repeat(12_001);
    let connector = Arc::new(FakeConnector::with_steps(vec![
        ConnectorStep::Response(response(
            200,
            Some("text/plain; charset=windows-1252"),
            None,
            vec![Ok(vec![b'c', b'a', b'f', 0xe9])],
        )),
        ConnectorStep::Response(response(
            200,
            Some("text/plain; charset=utf-8"),
            None,
            vec![Ok(long_unicode.into_bytes())],
        )),
    ]));
    let fetcher = fetcher(resolver, connector, CancellationToken::new());

    assert_eq!(
        fetcher
            .fetch("https://unicode.example/latin")
            .await
            .unwrap()
            .text,
        "café"
    );
    let capped = fetcher
        .fetch("https://unicode.example/long")
        .await
        .unwrap()
        .text;
    assert_eq!(capped.chars().count(), 12_000);
    assert!(capped.chars().all(|character| character == '界'));
}

#[tokio::test]
async fn public_search_uses_encoded_get_parses_links_deduplicates_and_honors_limit() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/html/"))
        .and(query_param("q", "rust 安全 & dns?"))
        .and(header("user-agent", "AIbb/0.1 public-read-only"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"
                <html><body>
                  <a class="result__a" href="https://one.example/a">one</a>
                  <a class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Ftwo.example%2Fb%3Fx%3D1">two</a>
                  <a class="result__a" href="https://one.example/a">duplicate</a>
                  <a class="result__a" href="https://three.example/c">three</a>
                </body></html>
            "#,
        ))
        .expect(1)
        .mount(&server)
        .await;
    let provider = DuckDuckGoHtmlSearch::with_endpoint(
        reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap(),
        format!("{}/html/", server.uri()),
        CancellationToken::new(),
    )
    .unwrap();

    let results = provider.search("rust 安全 & dns?", 2).await.unwrap();

    assert_eq!(
        results,
        vec![
            "https://one.example/a".to_owned(),
            "https://two.example/b?x=1".to_owned(),
        ]
    );
    let requests = server.received_requests().await.unwrap();
    assert!(requests[0]
        .url
        .as_str()
        .contains("?q=rust+%E5%AE%89%E5%85%A8+%26+dns%3F"));
    assert!(requests[0].body.is_empty());
}

#[tokio::test]
async fn public_search_returns_one_stable_error_for_status_or_markup_changes() {
    for response in [
        ResponseTemplate::new(503).set_body_string("upstream-secret-status-body"),
        ResponseTemplate::new(200)
            .set_body_string("<html><body>upstream-secret-new-markup</body></html>"),
    ] {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/html/"))
            .respond_with(response)
            .mount(&server)
            .await;
        let provider = DuckDuckGoHtmlSearch::with_endpoint(
            reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .unwrap(),
            format!("{}/html/", server.uri()),
            CancellationToken::new(),
        )
        .unwrap();

        let error = provider
            .search("renderer-secret-query", 3)
            .await
            .unwrap_err();
        let renderer_visible = serde_json::to_string(&error).unwrap();

        assert_eq!(error.code, ErrorCode::PublicSearchUnavailable.as_str());
        assert!(!renderer_visible.contains("renderer-secret-query"));
        assert!(!renderer_visible.contains("upstream-secret"));
        assert!(!renderer_visible.contains(&server.uri()));
    }
}

#[tokio::test]
async fn public_search_rejects_an_oversized_streamed_response() {
    let server = MockServer::start().await;
    let mut oversized = "<a class='result__a' href='https://one.example/'>one</a>".to_owned();
    oversized.push_str(&"x".repeat(2_000_001));
    Mock::given(method("GET"))
        .and(path("/html/"))
        .respond_with(ResponseTemplate::new(200).set_body_string(oversized))
        .mount(&server)
        .await;
    let provider = DuckDuckGoHtmlSearch::with_endpoint(
        reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap(),
        format!("{}/html/", server.uri()),
        CancellationToken::new(),
    )
    .unwrap();

    let error = provider.search("oversized", 1).await.unwrap_err();

    assert_eq!(error.code, ErrorCode::PublicSearchUnavailable.as_str());
}

#[tokio::test]
async fn public_search_decodes_relative_duckduckgo_result_redirects() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/html/"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            "<a class='result__a' href='/l/?uddg=https%3A%2F%2Frelative.example%2Farticle%3Fx%3D1'>relative</a>",
        ))
        .mount(&server)
        .await;
    let provider = DuckDuckGoHtmlSearch::with_endpoint(
        reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap(),
        format!("{}/html/", server.uri()),
        CancellationToken::new(),
    )
    .unwrap();

    let results = provider.search("relative", 1).await.unwrap();

    assert_eq!(
        results,
        vec!["https://relative.example/article?x=1".to_owned()]
    );
}

#[tokio::test]
async fn renderer_visible_page_errors_never_include_url_credentials_or_raw_body() {
    let invalid_fetcher = fetcher(
        Arc::new(FakeResolver::default()),
        Arc::new(FakeConnector::default()),
        CancellationToken::new(),
    );
    let invalid = invalid_fetcher
        .fetch("https://user:credential-secret@example.com/")
        .await
        .unwrap_err();
    let resolver = Arc::new(FakeResolver::with_answer(
        "failure.example",
        &["93.184.216.34"],
    ));
    let unavailable_fetcher = fetcher(
        resolver,
        Arc::new(FakeConnector::with_steps(vec![ConnectorStep::Response(
            response(
                500,
                Some("text/html"),
                None,
                vec![Ok(b"raw-upstream-secret".to_vec())],
            ),
        )])),
        CancellationToken::new(),
    );
    let unavailable = unavailable_fetcher
        .fetch("https://failure.example/?token=url-secret")
        .await
        .unwrap_err();

    for renderer_visible in [
        serde_json::to_string(&invalid).unwrap(),
        serde_json::to_string(&unavailable).unwrap(),
    ] {
        assert!(!renderer_visible.contains("credential-secret"));
        assert!(!renderer_visible.contains("raw-upstream-secret"));
        assert!(!renderer_visible.contains("url-secret"));
        assert!(!renderer_visible.contains("failure.example"));
    }
}

#[tokio::test]
async fn cancellation_interrupts_public_search_request_and_body() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/html/"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(Duration::from_secs(5))
                .set_body_string("<a class='result__a' href='https://one.example/'>one</a>"),
        )
        .mount(&server)
        .await;
    let cancellation = CancellationToken::new();
    let provider = DuckDuckGoHtmlSearch::with_endpoint(
        reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap(),
        format!("{}/html/", server.uri()),
        cancellation.clone(),
    )
    .unwrap();
    let task = tokio::spawn(async move { provider.search("cancel me", 1).await });
    tokio::time::sleep(Duration::from_millis(25)).await;

    cancellation.cancel();
    let error = tokio::time::timeout(Duration::from_millis(200), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap_err();

    assert_eq!(error.code, ErrorCode::Cancelled.as_str());
}
