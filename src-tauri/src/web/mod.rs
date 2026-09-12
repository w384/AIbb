mod extract;
mod fetch;
mod guard;
mod search;
pub mod source_preference;

pub use crate::domain::WebMaterial;
pub use extract::ExtractedPage;
pub use fetch::{
    BodyStream, FetchBudget, FetchedPage, HttpConnector, HttpResponse, ImagePayload, PageFetcher,
    ReqwestConnector, SafePageFetcher,
};
pub use guard::{
    resolve_public_target, validate_url, DnsResolver, ResolvedTarget, SystemDnsResolver,
};
pub use search::{DuckDuckGoHtmlSearch, SearchProvider};
pub use source_preference::{explicitly_wants_foreign, order_for_chinese};
