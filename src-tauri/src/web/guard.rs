use std::{
    collections::HashSet,
    net::IpAddr,
    str::FromStr,
    sync::{Arc, LazyLock},
};

use async_trait::async_trait;
use ipnet::IpNet;
use tokio_util::sync::CancellationToken;
use url::{Host, Url};

use crate::error::{AppError, ErrorCode};

static BLOCKED_NETWORKS: LazyLock<Vec<IpNet>> = LazyLock::new(|| {
    [
        "0.0.0.0/8",
        "10.0.0.0/8",
        "100.64.0.0/10",
        "127.0.0.0/8",
        "169.254.0.0/16",
        "168.63.129.16/32",
        "172.16.0.0/12",
        "192.0.0.0/24",
        "192.0.2.0/24",
        "192.88.99.0/24",
        "192.168.0.0/16",
        "198.18.0.0/15",
        "198.51.100.0/24",
        "203.0.113.0/24",
        "224.0.0.0/4",
        "240.0.0.0/4",
        "::/96",
        "::ffff:0:0/96",
        "64:ff9b::/96",
        "64:ff9b:1::/48",
        "100::/64",
        "2001::/23",
        "2001:2::/48",
        "2001:db8::/32",
        "2002::/16",
        "3fff::/20",
        "5f00::/16",
        "fc00::/7",
        "fe80::/10",
        "fec0::/10",
        "ff00::/8",
    ]
    .into_iter()
    .map(|network| IpNet::from_str(network).expect("blocked network constants must be valid"))
    .collect()
});

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedTarget {
    url: Url,
    host: String,
    port: u16,
    addresses: Vec<IpAddr>,
}

impl ResolvedTarget {
    pub fn url(&self) -> &Url {
        &self.url
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn addresses(&self) -> &[IpAddr] {
        &self.addresses
    }

    #[cfg(test)]
    pub(crate) fn for_connector_test(url: Url, address: IpAddr) -> Self {
        let host = url.host_str().unwrap().to_owned();
        let port = url.port_or_known_default().unwrap();
        Self {
            url,
            host,
            port,
            addresses: vec![address],
        }
    }
}

#[async_trait]
pub trait DnsResolver: Send + Sync {
    async fn resolve(
        &self,
        host: &str,
        port: u16,
        cancellation: &CancellationToken,
    ) -> Result<Vec<IpAddr>, AppError>;
}

#[derive(Debug, Default)]
pub struct SystemDnsResolver;

#[async_trait]
impl DnsResolver for SystemDnsResolver {
    async fn resolve(
        &self,
        host: &str,
        port: u16,
        cancellation: &CancellationToken,
    ) -> Result<Vec<IpAddr>, AppError> {
        tokio::select! {
            _ = cancellation.cancelled() => Err(AppError::from_code(ErrorCode::Cancelled)),
            result = tokio::net::lookup_host((host, port)) => {
                let addresses = result
                    .map_err(|error| {
                        AppError::from_code(ErrorCode::PublicPageUnavailable)
                            .with_diagnostic(error.to_string(), None)
                    })?
                    .map(|address| address.ip())
                    .collect();
                Ok(addresses)
            }
        }
    }
}

pub fn validate_url(raw_url: &str) -> Result<Url, AppError> {
    let url = Url::parse(raw_url).map_err(|_| unsafe_url())?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || url.host().is_none()
    {
        return Err(unsafe_url());
    }

    if let Some(address) = literal_address(&url) {
        ensure_public_address(address)?;
    }

    Ok(url)
}

pub async fn resolve_public_target(
    url: Url,
    resolver: Arc<dyn DnsResolver>,
    cancellation: &CancellationToken,
) -> Result<ResolvedTarget, AppError> {
    let port = url.port_or_known_default().ok_or_else(unsafe_url)?;
    let host = url.host_str().ok_or_else(unsafe_url)?.to_owned();
    let mut addresses = if let Some(address) = literal_address(&url) {
        vec![address]
    } else {
        resolver.resolve(&host, port, cancellation).await?
    };

    let mut seen = HashSet::new();
    addresses.retain(|address| seen.insert(*address));
    if addresses.is_empty() {
        return Err(AppError::from_code(ErrorCode::PublicPageUnavailable));
    }
    for address in &addresses {
        ensure_public_address(*address)?;
    }

    Ok(ResolvedTarget {
        url,
        host,
        port,
        addresses,
    })
}

fn literal_address(url: &Url) -> Option<IpAddr> {
    match url.host()? {
        Host::Ipv4(address) => Some(IpAddr::V4(address)),
        Host::Ipv6(address) => Some(IpAddr::V6(address)),
        Host::Domain(_) => None,
    }
}

fn ensure_public_address(address: IpAddr) -> Result<(), AppError> {
    if BLOCKED_NETWORKS
        .iter()
        .any(|network| network.contains(&address))
    {
        return Err(unsafe_url());
    }

    Ok(())
}

fn unsafe_url() -> AppError {
    AppError::from_code(ErrorCode::UnsafeUrl)
}
