use bytes::Bytes;
use http::{HeaderMap, Method};
use url::Url;

#[derive(Debug, Clone)]
pub struct PreparedRequest {
    pub method: Method,
    pub url: Url,
    pub headers: HeaderMap,
    pub body: Bytes,
    pub content_md5: Option<String>,
    pub content_type: Option<String>,
    pub signing_date: String,
    pub canonicalized_resource: String,
}
