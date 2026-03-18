mod options;
mod service_client;
mod table_client;

pub use options::{ClientOptions, DEFAULT_STORAGE_API_VERSION, MetadataLevel};
pub use service_client::TableServiceClient;
pub use table_client::{IfMatch, TableClient};
