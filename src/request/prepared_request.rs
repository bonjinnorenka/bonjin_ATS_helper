use std::fmt;

use bytes::Bytes;
use http::{HeaderMap, Method};
use url::Url;

use crate::request::headers::AUTHORIZATION;

#[derive(Clone)]
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

impl fmt::Debug for PreparedRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut url = self.url.clone();
        url.set_query(None);
        url.set_fragment(None);

        let mut header_names = self
            .headers
            .keys()
            .map(|name| name.as_str())
            .collect::<Vec<_>>();
        header_names.sort_unstable();

        f.debug_struct("PreparedRequest")
            .field("method", &self.method)
            .field("url", &url)
            .field("header_names", &header_names)
            .field(
                "has_authorization",
                &self.headers.contains_key(AUTHORIZATION),
            )
            .field("body_len", &self.body.len())
            .field("content_type", &self.content_type)
            .field("signing_date", &self.signing_date)
            .field("canonicalized_resource", &self.canonicalized_resource)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;
    use http::{HeaderMap, HeaderValue, Method};
    use url::Url;

    use crate::request::headers::AUTHORIZATION;

    use super::PreparedRequest;

    #[test]
    fn debug_redacts_sensitive_request_parts() {
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_static("SharedKey account:secret"),
        );
        let prepared = PreparedRequest {
            method: Method::GET,
            url: Url::parse("https://example.table.core.windows.net/Tables?sig=secret").unwrap(),
            headers,
            body: Bytes::from_static(b"secret"),
            content_md5: None,
            content_type: Some("application/json".to_owned()),
            signing_date: "Thu, 18 Mar 2026 03:04:05 GMT".to_owned(),
            canonicalized_resource: "/account/Tables".to_owned(),
        };

        let debug = format!("{prepared:?}");

        assert!(debug.contains("Tables"));
        assert!(debug.contains("has_authorization"));
        assert!(!debug.contains("sig=secret"));
        assert!(!debug.contains("SharedKey account:secret"));
        assert!(!debug.contains("secret"));
    }
}
