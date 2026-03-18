use std::time::Duration;

use http::HeaderValue;

use crate::error::{Result, ValidationError};

pub const DEFAULT_STORAGE_API_VERSION: &str = "2026-02-06";
pub const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
pub const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetadataLevel {
    NoMetadata,
    MinimalMetadata,
    FullMetadata,
}

impl MetadataLevel {
    pub(crate) fn accept_header(self) -> &'static str {
        match self {
            Self::NoMetadata => "application/json;odata=nometadata",
            Self::MinimalMetadata => "application/json;odata=minimalmetadata",
            Self::FullMetadata => "application/json;odata=fullmetadata",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ClientOptions {
    pub(crate) storage_api_version: String,
    pub(crate) metadata_level: MetadataLevel,
    pub(crate) user_agent: Option<String>,
    pub(crate) timeout: Option<Duration>,
    pub(crate) connect_timeout: Option<Duration>,
    pub(crate) allow_insecure_http: bool,
}

impl Default for ClientOptions {
    fn default() -> Self {
        Self {
            storage_api_version: DEFAULT_STORAGE_API_VERSION.to_owned(),
            metadata_level: MetadataLevel::NoMetadata,
            user_agent: None,
            timeout: Some(DEFAULT_REQUEST_TIMEOUT),
            connect_timeout: Some(DEFAULT_CONNECT_TIMEOUT),
            allow_insecure_http: false,
        }
    }
}

impl ClientOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_service_version(mut self, version: impl Into<String>) -> Self {
        self.storage_api_version = version.into();
        self
    }

    pub fn try_with_service_version(mut self, version: impl Into<String>) -> Result<Self> {
        let version = version.into();
        validate_storage_api_version(&version)?;
        self.storage_api_version = version;
        Ok(self)
    }

    pub fn with_metadata_level(mut self, metadata_level: MetadataLevel) -> Self {
        self.metadata_level = metadata_level;
        self
    }

    pub fn with_user_agent(mut self, user_agent: impl Into<String>) -> Self {
        self.user_agent = Some(user_agent.into());
        self
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    pub fn without_timeout(mut self) -> Self {
        self.timeout = None;
        self
    }

    pub fn with_connect_timeout(mut self, timeout: Duration) -> Self {
        self.connect_timeout = Some(timeout);
        self
    }

    pub fn without_connect_timeout(mut self) -> Self {
        self.connect_timeout = None;
        self
    }

    pub fn with_insecure_http_allowed(mut self, allowed: bool) -> Self {
        self.allow_insecure_http = allowed;
        self
    }
}

pub(crate) fn validate_storage_api_version(version: &str) -> Result<()> {
    HeaderValue::from_str(version).map_err(|_| {
        crate::error::Error::from(ValidationError::InvalidClientOption(
            "storage API version must be a valid HTTP header value".to_owned(),
        ))
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{ClientOptions, DEFAULT_CONNECT_TIMEOUT, DEFAULT_REQUEST_TIMEOUT};

    #[test]
    fn default_timeouts_are_enabled() {
        let options = ClientOptions::default();

        assert_eq!(options.timeout, Some(DEFAULT_REQUEST_TIMEOUT));
        assert_eq!(options.connect_timeout, Some(DEFAULT_CONNECT_TIMEOUT));
    }

    #[test]
    fn timeouts_can_be_disabled_explicitly() {
        let options = ClientOptions::default()
            .without_timeout()
            .without_connect_timeout();

        assert_eq!(options.timeout, None);
        assert_eq!(options.connect_timeout, None);
    }

    #[test]
    fn try_with_service_version_rejects_invalid_header_values() {
        let error = ClientOptions::default()
            .try_with_service_version("2026-02-06\r\nx-bad: 1")
            .unwrap_err();

        assert!(matches!(
            error,
            crate::error::Error::Validation(crate::error::ValidationError::InvalidClientOption(_))
        ));
    }

    #[test]
    fn with_connect_timeout_overrides_default() {
        let options = ClientOptions::default().with_connect_timeout(Duration::from_secs(2));

        assert_eq!(options.connect_timeout, Some(Duration::from_secs(2)));
    }
}
