use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::error::SerializationError;

pub(crate) fn format_datetime(value: &OffsetDateTime) -> Result<String, SerializationError> {
    value
        .format(&Rfc3339)
        .map_err(|error| SerializationError::DateTime(error.to_string()))
}

pub(crate) fn parse_datetime(value: &str) -> Result<OffsetDateTime, SerializationError> {
    OffsetDateTime::parse(value, &Rfc3339)
        .map_err(|error| SerializationError::DateTime(error.to_string()))
}
