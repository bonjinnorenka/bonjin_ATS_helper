use http::HeaderMap;

use super::ContinuationToken;

#[derive(Debug)]
pub struct QueryPage<T> {
    pub items: Vec<T>,
    pub continuation: Option<ContinuationToken>,
    pub request_id: Option<String>,
    pub raw_headers: HeaderMap,
}
