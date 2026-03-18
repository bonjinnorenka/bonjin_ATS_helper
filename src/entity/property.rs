use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq)]
pub enum EntityProperty {
    String(String),
    Bool(bool),
    Int32(i32),
    Int64(i64),
    Double(f64),
    Binary(Vec<u8>),
    Guid(Uuid),
    DateTime(OffsetDateTime),
}
