use time::OffsetDateTime;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntitySystemProperties {
    pub partition_key: String,
    pub row_key: String,
    pub etag: Option<String>,
    pub timestamp: Option<OffsetDateTime>,
}
