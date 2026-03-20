use crate::error::ValidationError;

pub(crate) fn validate_table_name(table_name: &str) -> Result<(), ValidationError> {
    let length = table_name.chars().count();
    if !(3..=63).contains(&length) {
        return Err(ValidationError::InvalidTableName(
            "table name length must be between 3 and 63 characters".to_owned(),
        ));
    }

    let mut chars = table_name.chars();
    let first = chars.next().expect("length check guards this");
    if !first.is_ascii_alphabetic() {
        return Err(ValidationError::InvalidTableName(
            "table name must start with an ASCII letter".to_owned(),
        ));
    }

    if !table_name.chars().all(|ch| ch.is_ascii_alphanumeric()) {
        return Err(ValidationError::InvalidTableName(
            "table name must be ASCII alphanumeric".to_owned(),
        ));
    }

    if table_name.eq_ignore_ascii_case("tables") {
        return Err(ValidationError::InvalidTableName(
            "table name 'tables' is reserved".to_owned(),
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::validate_table_name;

    #[test]
    fn rejects_invalid_table_names() {
        assert!(validate_table_name("ab").is_err());
        assert!(validate_table_name("1table").is_err());
        assert!(validate_table_name("bad-name").is_err());
        assert!(validate_table_name("tables").is_err());
    }
}
