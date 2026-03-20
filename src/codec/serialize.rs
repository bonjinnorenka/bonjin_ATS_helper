use base64::{Engine as _, engine::general_purpose::STANDARD};
use bytes::Bytes;
use serde_json::{Map, Number, Value};
use uuid::Uuid;

use crate::{
    entity::{DynamicEntity, EntityProperty, TableEntity},
    error::{Result, SerializationError, ValidationError},
    validation::{
        key::{validate_partition_key, validate_row_key},
        limits::validate_entity_limit_constraints,
        property::validate_property_name,
    },
};

use super::{datetime::format_datetime, edm::EdmType};

pub(crate) fn dynamic_entity_to_body(entity: &DynamicEntity) -> Result<Bytes> {
    validate_partition_key(&entity.partition_key)?;
    validate_row_key(&entity.row_key)?;

    let body =
        serde_json::to_vec(&dynamic_entity_to_value(entity)?).map_err(SerializationError::from)?;
    validate_entity_limit_constraints(entity, body.len())?;
    Ok(Bytes::from(body))
}

pub(crate) fn typed_entity_to_dynamic<T>(entity: &T) -> Result<DynamicEntity>
where
    T: TableEntity,
{
    let value = serde_json::to_value(entity).map_err(SerializationError::from)?;
    let mut object = match value {
        Value::Object(object) => object,
        _ => {
            return Err(SerializationError::UnsupportedShape(
                "typed entity must serialize into a JSON object".to_owned(),
            )
            .into());
        }
    };

    remove_system_fields(&mut object);

    let mut dynamic = DynamicEntity::new(entity.partition_key(), entity.row_key());
    dynamic.etag = entity.etag().map(ToOwned::to_owned);
    for (key, value) in object {
        if let Some(property) = infer_property_from_typed_json(value)? {
            dynamic.insert_property(key, property);
        }
    }
    Ok(dynamic)
}

pub(crate) fn dynamic_entity_to_value(entity: &DynamicEntity) -> Result<Value> {
    let mut object = Map::new();
    object.insert(
        "PartitionKey".to_owned(),
        Value::String(entity.partition_key.clone()),
    );
    object.insert("RowKey".to_owned(), Value::String(entity.row_key.clone()));

    for (name, property) in &entity.properties {
        validate_property_name(name)?;
        let (value, edm_type) = entity_property_to_json(property)?;
        object.insert(name.clone(), value);
        if let Some(annotation) = edm_type.annotation() {
            object.insert(
                format!("{name}@odata.type"),
                Value::String(annotation.to_owned()),
            );
        }
    }

    Ok(Value::Object(object))
}

pub(crate) fn entity_property_to_json(property: &EntityProperty) -> Result<(Value, EdmType)> {
    match property {
        EntityProperty::String(value) => Ok((Value::String(value.clone()), EdmType::String)),
        EntityProperty::Bool(value) => Ok((Value::Bool(*value), EdmType::Boolean)),
        EntityProperty::Int32(value) => Ok((Value::Number(Number::from(*value)), EdmType::Int32)),
        EntityProperty::Int64(value) => Ok((Value::String(value.to_string()), EdmType::Int64)),
        EntityProperty::Double(value) => Ok((
            Value::Number(Number::from_f64(*value).ok_or_else(|| {
                SerializationError::UnsupportedShape(
                    "non-finite floating point values are not supported".to_owned(),
                )
            })?),
            EdmType::Double,
        )),
        EntityProperty::Binary(value) => {
            Ok((Value::String(STANDARD.encode(value)), EdmType::Binary))
        }
        EntityProperty::Guid(value) => Ok((Value::String(value.to_string()), EdmType::Guid)),
        EntityProperty::DateTime(value) => {
            Ok((Value::String(format_datetime(value)?), EdmType::DateTime))
        }
    }
}

fn infer_property_from_typed_json(value: Value) -> Result<Option<EntityProperty>> {
    match value {
        Value::Null => Ok(None),
        Value::Bool(value) => Ok(Some(EntityProperty::Bool(value))),
        Value::Number(number) => {
            if let Some(value) = number.as_i64() {
                if let Ok(value) = i32::try_from(value) {
                    return Ok(Some(EntityProperty::Int32(value)));
                }
                return Ok(Some(EntityProperty::Int64(value)));
            }

            if let Some(value) = number.as_u64() {
                if let Ok(value) = i32::try_from(value) {
                    return Ok(Some(EntityProperty::Int32(value)));
                }
                if let Ok(value) = i64::try_from(value) {
                    return Ok(Some(EntityProperty::Int64(value)));
                }
                return Err(ValidationError::EntityLimit(
                    "unsigned integers larger than i64 are not supported by Azure Table Storage"
                        .to_owned(),
                )
                .into());
            }

            Ok(Some(EntityProperty::Double(number.as_f64().ok_or_else(
                || SerializationError::UnsupportedShape("invalid number".to_owned()),
            )?)))
        }
        Value::String(value) => {
            if let Ok(uuid) = Uuid::parse_str(&value) {
                return Ok(Some(EntityProperty::Guid(uuid)));
            }
            if let Ok(datetime) = super::datetime::parse_datetime(&value) {
                return Ok(Some(EntityProperty::DateTime(datetime)));
            }
            Ok(Some(EntityProperty::String(value)))
        }
        Value::Array(values) => {
            let mut bytes = Vec::with_capacity(values.len());
            for value in values {
                let Some(byte) = value.as_u64().and_then(|value| u8::try_from(value).ok()) else {
                    return Err(SerializationError::UnsupportedShape(
                        "only byte arrays are supported; nested arrays are not".to_owned(),
                    )
                    .into());
                };
                bytes.push(byte);
            }
            Ok(Some(EntityProperty::Binary(bytes)))
        }
        Value::Object(_) => Err(SerializationError::UnsupportedShape(
            "nested objects are not supported by Azure Table Storage entities".to_owned(),
        )
        .into()),
    }
}

fn remove_system_fields(object: &mut Map<String, Value>) {
    for key in [
        "PartitionKey",
        "RowKey",
        "Timestamp",
        "partition_key",
        "row_key",
        "timestamp",
        "etag",
        "odata.etag",
    ] {
        object.remove(key);
    }
}

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};

    use crate::{entity::TableEntity, query::Query};

    use super::typed_entity_to_dynamic;

    #[derive(Debug, Serialize, Deserialize)]
    struct SampleEntity {
        partition_key: String,
        row_key: String,
        count: i64,
        deleted_at: Option<String>,
    }

    impl TableEntity for SampleEntity {
        fn partition_key(&self) -> &str {
            &self.partition_key
        }

        fn row_key(&self) -> &str {
            &self.row_key
        }
    }

    #[test]
    fn removes_none_fields_from_typed_entities() {
        let entity = SampleEntity {
            partition_key: "pk".to_owned(),
            row_key: "rk".to_owned(),
            count: 100,
            deleted_at: None,
        };

        let dynamic = typed_entity_to_dynamic(&entity).unwrap();
        assert_eq!(dynamic.properties.len(), 1);
        assert!(dynamic.properties.contains_key("count"));
    }

    #[test]
    fn top_validation_is_publicly_reachable() {
        assert!(Query::new().with_top(1001).is_err());
    }
}
