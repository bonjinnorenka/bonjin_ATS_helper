use crate::error::ValidationError;

pub(crate) fn validate_partition_key(value: &str) -> Result<(), ValidationError> {
    validate_key("PartitionKey", value)
}

pub(crate) fn validate_row_key(value: &str) -> Result<(), ValidationError> {
    validate_key("RowKey", value)
}

fn validate_key(field_name: &'static str, value: &str) -> Result<(), ValidationError> {
    if value.is_empty() {
        return Err(ValidationError::InvalidKey(format!(
            "{field_name} cannot be empty"
        )));
    }

    if value.chars().count() > 1024 {
        return Err(ValidationError::InvalidKey(format!(
            "{field_name} cannot exceed 1024 characters"
        )));
    }

    if value
        .chars()
        .any(|ch| matches!(ch, '/' | '\\' | '#' | '?') || is_forbidden_control_char(ch))
    {
        return Err(ValidationError::InvalidKey(format!(
            "{field_name} contains forbidden characters"
        )));
    }

    Ok(())
}

fn is_forbidden_control_char(ch: char) -> bool {
    matches!(ch as u32, 0x00..=0x1F | 0x7F..=0x9F)
}

#[cfg(test)]
mod tests {
    use super::{validate_partition_key, validate_row_key};

    #[test]
    fn rejects_invalid_keys() {
        assert!(validate_partition_key("").is_err());
        assert!(validate_row_key("bad/key").is_err());
    }
}
