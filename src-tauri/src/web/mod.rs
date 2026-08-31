mod extract;
mod fetch;
mod guard;
mod search;

pub use crate::domain::WebMaterial;
pub use extract::ExtractedPage;
pub use fetch::{
    BodyStream, FetchBudget, FetchedPage, HttpConnector, HttpResponse, PageFetcher,
    ReqwestConnector, SafePageFetcher,
};
pub use guard::{
    resolve_public_target, validate_url, DnsResolver, ResolvedTarget, SystemDnsResolver,
};
pub use search::{DuckDuckGoHtmlSearch, SearchProvider};
