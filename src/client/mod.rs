mod if_match;
mod options;
mod service_client;
mod table_client;

pub use if_match::IfMatch;
pub(crate) use options::validate_storage_api_version;
pub use options::{
    ClientOptions, DEFAULT_CONNECT_TIMEOUT, DEFAULT_REQUEST_TIMEOUT, DEFAULT_STORAGE_API_VERSION,
    MetadataLevel,
};
pub use service_client::TableServiceClient;
pub use table_client::TableClient;
