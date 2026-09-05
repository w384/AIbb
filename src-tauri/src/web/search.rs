use std::{collections::HashSet, time::Duration};

use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD_NO_PAD as BASE64_STANDARD_NO_PAD, Engine as _};
use futures_util::StreamExt;
use scraper::{Html, Selector};
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::error::{AppError, ErrorCode};

use super::validate_url;

const SEARCH_ENDPOINT: &str = "https://html.duckduckgo.com/html/";
const FALLBACK_SEARCH_ENDPOINT: &str = "https://www.bing.com/search";
const USER_AGENT: &str = "AIbb/0.1 public-read-only";
const SEARCH_TIMEOUT: Duration = Duration::from_secs(12);
const MAX_SEARCH_RESPONSE_BYTES: usize = 2_000_000;

#[async_trait]
pub trait SearchProvider: Send + Sync {
    async fn search(&self, query: &str, limit: usize) -> Result<Vec<String>, AppError>;
}

pub struct DuckDuckGoHtmlSearch {
    http: reqwest::Client,
    endpoint: Url,
    fallback_endpoint: Option<Url>,
    cancellation: CancellationToken,
}

impl DuckDuckGoHtmlSearch {
    pub fn new(cancellation: CancellationToken) -> Result<Self, AppError> {
        let http = build_search_client(reqwest::Client::builder())?;
        Self::with_endpoints(
            http,
            SEARCH_ENDPOINT.to_owned(),
            Some(FALLBACK_SEARCH_ENDPOINT.to_owned()),
            cancellation,
        )
    }

    #[doc(hidden)]
    pub fn with_endpoint(
        http: reqwest::Client,
        endpoint: String,
        cancellation: CancellationToken,
    ) -> Result<Self, AppError> {
        Self::with_endpoints(http, endpoint, None, cancellation)
    }

    fn with_endpoints(
        http: reqwest::Client,
        endpoint: String,
        fallback_endpoint: Option<String>,
        cancellation: CancellationToken,
    ) -> Result<Self, AppError> {
        let endpoint = Url::parse(&endpoint).map_err(|_| search_error("invalid endpoint"))?;
        let fallback_endpoint = fallback_endpoint
            .map(|endpoint| Url::parse(&endpoint).map_err(|_| search_error("invalid endpoint")))
            .transpose()?;
        Ok(Self {
            http,
            endpoint,
            fallback_endpoint,
            cancellation,
        })
    }

    async fn search_inner(&self, query: &str, limit: usize) -> Result<Vec<String>, AppError> {
        match self.search_endpoint(&self.endpoint, query, limit).await {
            Ok(results) => Ok(results),
            Err(error) if error.code == ErrorCode::PublicSearchUnavailable.as_str() => {
                let Some(fallback_endpoint) = self.fallback_endpoint.as_ref() else {
                    return Err(error);
                };
                self.search_endpoint(fallback_endpoint, query, limit).await
            }
            Err(error) => Err(error),
        }
    }

    async fn search_endpoint(
        &self,
        endpoint: &Url,
        query: &str,
        limit: usize,
    ) -> Result<Vec<String>, AppError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let mut url = endpoint.clone();
        url.query_pairs_mut().clear().append_pair("q", query);
        let response = tokio::select! {
            _ = self.cancellation.cancelled() => {
                return Err(AppError::from_code(ErrorCode::Cancelled));
            }
            response = self.http.get(url).header(reqwest::header::USER_AGENT, USER_AGENT).send() => {
                response.map_err(search_error)?
            }
        };
        if !response.status().is_success() {
            return Err(search_error("search status changed"));
        }
        let mut stream = response.bytes_stream();
        let mut body = Vec::new();
        loop {
            let chunk = tokio::select! {
                _ = self.cancellation.cancelled() => {
                    return Err(AppError::from_code(ErrorCode::Cancelled));
                }
                chunk = stream.next() => chunk,
            };
            let Some(chunk) = chunk else {
                break;
            };
            let chunk = chunk.map_err(search_error)?;
            if body
                .len()
                .checked_add(chunk.len())
                .is_none_or(|size| size > MAX_SEARCH_RESPONSE_BYTES)
            {
                return Err(search_error("search response too large"));
            }
            body.extend_from_slice(&chunk);
        }
        let body = String::from_utf8(body).map_err(search_error)?;

        parse_result_links(&body, limit)
    }
}

fn build_search_client(builder: reqwest::ClientBuilder) -> Result<reqwest::Client, AppError> {
    builder
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .build()
        .map_err(search_error)
}

#[async_trait]
impl SearchProvider for DuckDuckGoHtmlSearch {
    async fn search(&self, query: &str, limit: usize) -> Result<Vec<String>, AppError> {
        tokio::select! {
            biased;
            _ = self.cancellation.cancelled() => {
                Err(AppError::from_code(ErrorCode::Cancelled))
            }
            result = timeout(SEARCH_TIMEOUT, self.search_inner(query, limit)) => {
                result.unwrap_or_else(|_| Err(search_error("search timeout")))
            }
        }
    }
}

fn parse_result_links(body: &str, limit: usize) -> Result<Vec<String>, AppError> {
    let document = Html::parse_document(body);
    let selectors = [
        Selector::parse("a.result__a[href]").expect("static search selector must be valid"),
        Selector::parse("li.b_algo h2 a[href]").expect("static search selector must be valid"),
    ];
    let mut seen = HashSet::new();
    let mut results = Vec::new();

    for selector in &selectors {
        for element in document.select(selector) {
            let Some(href) = element.value().attr("href") else {
                continue;
            };
            let Some(url) = result_url(href) else {
                continue;
            };
            let normalized = url.as_str().to_owned();
            if seen.insert(normalized.clone()) {
                results.push(normalized);
                if results.len() == limit {
                    return Ok(results);
                }
            }
        }
    }

    if results.is_empty() {
        Err(search_error("search markup changed"))
    } else {
        Ok(results)
    }
}

fn result_url(href: &str) -> Option<Url> {
    let candidate = if href.starts_with("//") {
        Url::parse(&format!("https:{href}")).ok()?
    } else if href.starts_with('/') {
        Url::parse(SEARCH_ENDPOINT).ok()?.join(href).ok()?
    } else {
        Url::parse(href).ok()?
    };
    let host = candidate.host_str().unwrap_or_default();
    let is_duckduckgo = host.eq_ignore_ascii_case("duckduckgo.com")
        || host.to_ascii_lowercase().ends_with(".duckduckgo.com");
    let is_bing =
        host.eq_ignore_ascii_case("bing.com") || host.to_ascii_lowercase().ends_with(".bing.com");
    let target = if is_duckduckgo && candidate.path() == "/l/" {
        let encoded = candidate.query_pairs().find(|(name, _)| name == "uddg")?.1;
        Url::parse(encoded.as_ref()).ok()?
    } else if is_bing && candidate.path() == "/ck/a" {
        bing_tracking_target(&candidate)?
    } else {
        candidate
    };
    let mut target = validate_url(target.as_str()).ok()?;
    target.set_fragment(None);
    Some(target)
}

fn bing_tracking_target(candidate: &Url) -> Option<Url> {
    let encoded = candidate.query_pairs().find(|(name, _)| name == "u")?.1;
    let payload = encoded.strip_prefix("a1")?;
    let decoded = BASE64_STANDARD_NO_PAD.decode(payload).ok()?;
    let destination = std::str::from_utf8(&decoded).ok()?;
    Url::parse(destination).ok()
}

fn search_error(error: impl std::fmt::Display) -> AppError {
    AppError::from_code(ErrorCode::PublicSearchUnavailable).with_diagnostic(error.to_string(), None)
}

#[cfg(test)]
mod tests {
    use std::{net::SocketAddr, time::Duration};

    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
        time::timeout,
    };
    use tokio_util::sync::CancellationToken;

    use super::{build_search_client, parse_result_links, DuckDuckGoHtmlSearch, SearchProvider};

    #[test]
    fn parses_bing_result_links_when_the_primary_search_markup_is_unavailable() {
        let body = r#"
            <li class="b_algo"><h2><a href="https://example.com/bing-result">Bing result</a></h2></li>
        "#;

        assert_eq!(
            parse_result_links(body, 1).unwrap(),
            vec!["https://example.com/bing-result".to_string()]
        );
    }

    #[test]
    fn resolves_bing_tracking_links_to_the_public_result_page() {
        let body = r#"
            <li class="b_algo"><h2><a href="https://www.bing.com/ck/a?u=a1aHR0cHM6Ly96aC53aWtpcGVkaWEub3JnL3poLWNuLyVFNyU4MSVBQiVFNiU5OCU5Rg&amp;ntb=1">Mars</a></h2></li>
        "#;

        assert_eq!(
            parse_result_links(body, 1).unwrap(),
            vec!["https://zh.wikipedia.org/zh-cn/%E7%81%AB%E6%98%9F".to_string()]
        );
    }

    async fn serve_once(listener: TcpListener, body: &'static str) {
        serve_once_with_status(listener, "200 OK", body).await;
    }

    async fn serve_once_with_status(listener: TcpListener, status: &str, body: &'static str) {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        let mut buffer = [0_u8; 1024];
        loop {
            let read = socket.read(&mut buffer).await.unwrap();
            if read == 0 {
                break;
            }
            request.extend_from_slice(&buffer[..read]);
            if request.windows(4).any(|window| window == b"\r\n\r\n") {
                break;
            }
        }
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        socket.write_all(response.as_bytes()).await.unwrap();
    }

    #[tokio::test]
    async fn public_search_falls_back_after_primary_search_markup_is_unavailable() {
        let primary = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let fallback = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let primary_endpoint = format!("http://{}/search", primary.local_addr().unwrap());
        let fallback_endpoint = format!("http://{}/search", fallback.local_addr().unwrap());
        let primary_task = tokio::spawn(serve_once_with_status(
            primary,
            "202 Accepted",
            "<html>verification required</html>",
        ));
        let fallback_task = tokio::spawn(serve_once(
            fallback,
            r#"<li class="b_algo"><h2><a href="https://example.com/fallback">Fallback</a></h2></li>"#,
        ));
        let search = DuckDuckGoHtmlSearch::with_endpoints(
            build_search_client(reqwest::Client::builder()).unwrap(),
            primary_endpoint,
            Some(fallback_endpoint),
            CancellationToken::new(),
        )
        .unwrap();

        assert_eq!(
            search.search("AIbb", 1).await.unwrap(),
            vec!["https://example.com/fallback".to_string()]
        );
        primary_task.await.unwrap();
        fallback_task.await.unwrap();
    }

    #[tokio::test]
    async fn production_search_client_clears_configured_proxies() {
        let origin = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin_address = origin.local_addr().unwrap();
        let proxy = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy_address = proxy.local_addr().unwrap();
        let mut origin_task = tokio::spawn(serve_once(origin, "origin"));
        let mut proxy_task = tokio::spawn(serve_once(proxy, "proxy"));
        let pinned_origin = SocketAddr::new(origin_address.ip(), origin_address.port());
        let client = build_search_client(
            reqwest::Client::builder()
                .proxy(reqwest::Proxy::all(format!("http://{proxy_address}")).unwrap())
                .resolve_to_addrs("direct.invalid", &[pinned_origin]),
        )
        .unwrap();

        let body = client
            .get(format!("http://direct.invalid:{}/", origin_address.port()))
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        let origin_result = timeout(Duration::from_millis(100), &mut origin_task).await;
        let proxy_result = timeout(Duration::from_millis(100), &mut proxy_task).await;
        origin_task.abort();
        proxy_task.abort();

        assert_eq!(body, "origin");
        assert!(
            origin_result.is_ok(),
            "origin must receive the direct request"
        );
        assert!(
            proxy_result.is_err(),
            "configured proxy must receive no request"
        );
    }
}
