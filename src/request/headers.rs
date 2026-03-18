use http::header::HeaderName;

pub(crate) const ACCEPT: HeaderName = HeaderName::from_static("accept");
pub(crate) const AUTHORIZATION: HeaderName = HeaderName::from_static("authorization");
pub(crate) const CONTENT_TYPE: HeaderName = HeaderName::from_static("content-type");
pub(crate) const DATA_SERVICE_VERSION: HeaderName = HeaderName::from_static("dataserviceversion");
pub(crate) const DATE: HeaderName = HeaderName::from_static("date");
pub(crate) const IF_MATCH: HeaderName = HeaderName::from_static("if-match");
pub(crate) const MAX_DATA_SERVICE_VERSION: HeaderName =
    HeaderName::from_static("maxdataserviceversion");
pub(crate) const X_MS_DATE: HeaderName = HeaderName::from_static("x-ms-date");
pub(crate) const X_MS_VERSION: HeaderName = HeaderName::from_static("x-ms-version");
