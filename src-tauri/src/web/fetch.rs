use std::{
    net::SocketAddr,
    pin::Pin,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

use crate::error::{AppError, ErrorCode};
use async_trait::async_trait;
use encoding_rs::{Encoding, UTF_8};
use futures_util::{Stream, StreamExt};
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

use super::{
    extract::extract_page, resolve_public_target, validate_url, DnsResolver, ResolvedTarget,
    SystemDnsResolver,
};

const USER_AGENT: &str = "AIbb/0.1 public-read-only";
const MAX_RESPONSE_BYTES: usize = 2_000_000;
const MAX_REDIRECTS: usize = 3;
const MAX_PAGES: usize = 8;
const DEFAULT_PAGE_TIMEOUT: Duration = Duration::from_secs(12);

pub type BodyStream =
    Pin<Box<dyn Stream<Item = Result<Vec<u8>, AppError>> + Send + Sync + 'static>>;

pub struct HttpResponse {
    status: u16,
    content_type: Option<String>,
    location: Option<String>,
    body: BodyStream,
}

impl HttpResponse {
    pub fn new(
        status: u16,
        content_type: Option<String>,
        location: Option<String>,
        body: BodyStream,
    ) -> Self {
        Self {
            status,
            content_type,
            location,
            body,
        }
    }
}

#[async_trait]
pub trait HttpConnector: Send + Sync {
    async fn get(
        &self,
        target: &ResolvedTarget,
        cancellation: &CancellationToken,
    ) -> Result<HttpResponse, AppError>;
}

#[derive(Debug, Default)]
pub struct ReqwestConnector;

#[async_trait]
impl HttpConnector for ReqwestConnector {
    async fn get(
        &self,
        target: &ResolvedTarget,
        cancellation: &CancellationToken,
    ) -> Result<HttpResponse, AppError> {
        let pinned_addresses = target
            .addresses()
            .iter()
            .map(|address| SocketAddr::new(*address, target.port()))
            .collect::<Vec<_>>();
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .user_agent(USER_AGENT)
            .resolve_to_addrs(target.host(), &pinned_addresses)
            .build()
            .map_err(public_page_error)?;
        let request = client
            .get(target.url().clone())
            .header(
                reqwest::header::ACCEPT,
                "text/html, text/plain, application/xhtml+xml",
            )
            .send();
        let response = tokio::select! {
            _ = cancellation.cancelled() => {
                return Err(AppError::from_code(ErrorCode::Cancelled));
            }
            result = request => result.map_err(public_page_error)?,
        };
        let status = response.status().as_u16();
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(ToOwned::to_owned);
        let location = response
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|value| value.to_str().ok())
            .map(ToOwned::to_owned);
        let stream = response
            .bytes_stream()
            .map(|chunk| chunk.map(|bytes| bytes.to_vec()).map_err(public_page_error));

        Ok(HttpResponse::new(
            status,
            content_type,
            location,
            Box::pin(stream),
        ))
    }
}

#[derive(Debug, Clone, Default)]
pub struct FetchBudget {
    used: Arc<AtomicUsize>,
}

impl FetchBudget {
    pub fn new() -> Self {
        Self::default()
    }

    fn reserve(&self) -> Result<(), AppError> {
        self.used
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                (used < MAX_PAGES).then_some(used + 1)
            })
            .map(|_| ())
            .map_err(|_| AppError::from_code(ErrorCode::PageBudgetExceeded))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchedPage {
    pub title: String,
    pub canonical_url: String,
    pub text: String,
}

#[async_trait]
pub trait PageFetcher: Send + Sync {
    async fn fetch(&self, url: &str) -> Result<FetchedPage, AppError>;
}

pub struct SafePageFetcher {
    resolver: Arc<dyn DnsResolver>,
    connector: Arc<dyn HttpConnector>,
    cancellation: CancellationToken,
    budget: FetchBudget,
    page_timeout: Duration,
}

impl SafePageFetcher {
    pub fn new(cancellation: CancellationToken, budget: FetchBudget) -> Self {
        Self {
            resolver: Arc::new(SystemDnsResolver),
            connector: Arc::new(ReqwestConnector),
            cancellation,
            budget,
            page_timeout: DEFAULT_PAGE_TIMEOUT,
        }
    }

    #[doc(hidden)]
    pub fn with_components(
        resolver: Arc<dyn DnsResolver>,
        connector: Arc<dyn HttpConnector>,
        cancellation: CancellationToken,
        budget: FetchBudget,
        page_timeout: Duration,
    ) -> Self {
        Self {
            resolver,
            connector,
            cancellation,
            budget,
            page_timeout,
        }
    }

    async fn fetch_inner(&self, raw_url: &str) -> Result<FetchedPage, AppError> {
        self.budget.reserve()?;
        let mut current_url = validate_url(raw_url)?;
        let mut redirects_followed = 0;

        loop {
            let target =
                resolve_public_target(current_url, self.resolver.clone(), &self.cancellation)
                    .await?;
            let mut response = self.connector.get(&target, &self.cancellation).await?;

            if is_redirect(response.status) {
                if redirects_followed == MAX_REDIRECTS {
                    return Err(AppError::from_code(ErrorCode::RedirectLimitExceeded));
                }
                let location = response
                    .location
                    .ok_or_else(|| AppError::from_code(ErrorCode::PublicPageUnavailable))?;
                let redirected = target
                    .url()
                    .join(&location)
                    .map_err(|_| AppError::from_code(ErrorCode::UnsafeUrl))?;
                current_url = validate_url(redirected.as_str())?;
                redirects_followed += 1;
                continue;
            }

            if !(200..300).contains(&response.status) {
                return Err(AppError::from_code(ErrorCode::PublicPageUnavailable));
            }
            let content_type = response
                .content_type
                .as_deref()
                .ok_or_else(|| AppError::from_code(ErrorCode::UnsupportedContent))?;
            let media_type = allowed_media_type(content_type)?;
            let encoding = content_encoding(content_type);
            let bytes = read_limited_body(&mut response.body, &self.cancellation).await?;
            let source = decode_body(&bytes, encoding);
            let final_url = target.url().clone();
            let media_type = media_type.to_owned();
            let extracted =
                tokio::task::spawn_blocking(move || extract_page(&final_url, &media_type, &source))
                    .await
                    .map_err(public_page_error)?;
            return Ok(FetchedPage {
                title: extracted.title,
                canonical_url: extracted.canonical_url,
                text: extracted.text,
            });
        }
    }
}

#[async_trait]
impl PageFetcher for SafePageFetcher {
    async fn fetch(&self, url: &str) -> Result<FetchedPage, AppError> {
        tokio::select! {
            biased;
            _ = self.cancellation.cancelled() => {
                Err(AppError::from_code(ErrorCode::Cancelled))
            }
            result = timeout(self.page_timeout, self.fetch_inner(url)) => {
                result.unwrap_or_else(|_| Err(AppError::from_code(ErrorCode::RequestTimeout)))
            }
        }
    }
}

fn is_redirect(status: u16) -> bool {
    matches!(status, 301 | 302 | 303 | 307 | 308)
}

fn allowed_media_type(content_type: &str) -> Result<&str, AppError> {
    let media_type = content_type.split(';').next().unwrap_or_default().trim();
    if ["text/html", "text/plain", "application/xhtml+xml"]
        .iter()
        .any(|allowed| media_type.eq_ignore_ascii_case(allowed))
    {
        Ok(match media_type.to_ascii_lowercase().as_str() {
            "text/html" => "text/html",
            "text/plain" => "text/plain",
            _ => "application/xhtml+xml",
        })
    } else {
        Err(AppError::from_code(ErrorCode::UnsupportedContent))
    }
}

fn content_encoding(content_type: &str) -> &'static Encoding {
    content_type
        .split(';')
        .skip(1)
        .filter_map(|parameter| parameter.split_once('='))
        .find(|(name, _)| name.trim().eq_ignore_ascii_case("charset"))
        .and_then(|(_, value)| Encoding::for_label(value.trim().trim_matches('"').as_bytes()))
        .unwrap_or(UTF_8)
}

async fn read_limited_body(
    body: &mut BodyStream,
    cancellation: &CancellationToken,
) -> Result<Vec<u8>, AppError> {
    let mut bytes = Vec::new();
    loop {
        let chunk = tokio::select! {
            _ = cancellation.cancelled() => {
                return Err(AppError::from_code(ErrorCode::Cancelled));
            }
            chunk = body.next() => chunk,
        };
        let Some(chunk) = chunk else {
            return Ok(bytes);
        };
        let chunk = chunk?;
        if bytes
            .len()
            .checked_add(chunk.len())
            .is_none_or(|size| size > MAX_RESPONSE_BYTES)
        {
            return Err(AppError::from_code(ErrorCode::ResponseTooLarge));
        }
        bytes.extend_from_slice(&chunk);
    }
}

fn decode_body(bytes: &[u8], encoding: &'static Encoding) -> String {
    let (decoded, _, _) = encoding.decode(bytes);
    decoded.into_owned()
}

fn public_page_error(error: impl std::fmt::Display) -> AppError {
    AppError::from_code(ErrorCode::PublicPageUnavailable).with_diagnostic(error.to_string(), None)
}

#[cfg(test)]
mod tests {
    use std::net::IpAddr;

    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };
    use url::Url;

    use super::{HttpConnector, ReqwestConnector};
    use crate::web::ResolvedTarget;

    #[tokio::test]
    async fn reqwest_connector_pins_the_address_and_sends_no_cookie_or_redirect() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let mut requests = Vec::new();
            for index in 0..2 {
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
                requests.push(String::from_utf8(request).unwrap());
                let response = if index == 0 {
                    "HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:1/must-not-follow\r\nSet-Cookie: session=secret\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                } else {
                    "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok"
                };
                socket.write_all(response.as_bytes()).await.unwrap();
            }
            requests
        });
        let target = ResolvedTarget::for_connector_test(
            Url::parse(&format!("http://pinned.invalid:{}/page", address.port())).unwrap(),
            "127.0.0.1".parse::<IpAddr>().unwrap(),
        );
        let connector = ReqwestConnector;

        let first = connector
            .get(&target, &tokio_util::sync::CancellationToken::new())
            .await
            .unwrap();
        let second = connector
            .get(&target, &tokio_util::sync::CancellationToken::new())
            .await
            .unwrap();
        let requests = server.await.unwrap();

        assert_eq!(first.status, 302, "connector must not follow redirects");
        assert_eq!(second.status, 200);
        for request in &requests {
            let lowercase = request.to_ascii_lowercase();
            assert!(request.contains("user-agent: AIbb/0.1 public-read-only\r\n"));
            assert!(lowercase.contains("host: pinned.invalid:"));
            assert!(!lowercase.contains("cookie:"));
        }
    }
}
