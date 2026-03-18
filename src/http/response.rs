use bytes::Bytes;
use http::{HeaderMap, StatusCode};

#[derive(Debug, Clone)]
pub(crate) struct Response {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: Bytes,
}
