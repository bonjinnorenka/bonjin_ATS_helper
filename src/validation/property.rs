use crate::error::ValidationError;

pub(crate) fn validate_property_name(name: &str) -> Result<(), ValidationError> {
    if matches!(name, "PartitionKey" | "RowKey" | "Timestamp") {
        return Err(ValidationError::EntityLimit(
            "custom properties cannot overwrite system properties".to_owned(),
        ));
    }
    if name.trim().is_empty() {
        return Err(ValidationError::EntityLimit(
            "property names cannot be empty".to_owned(),
        ));
    }
    if name.chars().count() > 255 {
        return Err(ValidationError::EntityLimit(
            "property names cannot exceed 255 characters".to_owned(),
        ));
    }
    if name.contains('-') {
        return Err(ValidationError::EntityLimit(
            "property names cannot contain '-'".to_owned(),
        ));
    }
    if name.chars().any(|ch| {
        matches!(ch as u32, 0x00..=0x1F | 0x7F..=0x9F) || matches!(ch, '/' | '\\' | '#' | '?')
    }) {
        return Err(ValidationError::EntityLimit(
            "property names contain unsupported characters".to_owned(),
        ));
    }

    Ok(())
}
