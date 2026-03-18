use std::time::Duration;

pub const DEFAULT_STORAGE_API_VERSION: &str = "2026-02-06";

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
}

impl Default for ClientOptions {
    fn default() -> Self {
        Self {
            storage_api_version: DEFAULT_STORAGE_API_VERSION.to_owned(),
            metadata_level: MetadataLevel::NoMetadata,
            user_agent: None,
            timeout: None,
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
}
