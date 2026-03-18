use crate::{entity::DynamicEntity, error::ValidationError};

pub(crate) fn validate_entity_limit_constraints(
    entity: &DynamicEntity,
    estimated_payload_size: usize,
) -> Result<(), ValidationError> {
    let property_count = entity.properties.len() + 3;
    if property_count > 255 {
        return Err(ValidationError::EntityLimit(
            "entity cannot contain more than 255 properties including system properties".to_owned(),
        ));
    }

    if estimated_payload_size > 1024 * 1024 {
        return Err(ValidationError::EntityLimit(
            "entity payload cannot exceed 1 MiB".to_owned(),
        ));
    }

    Ok(())
}
