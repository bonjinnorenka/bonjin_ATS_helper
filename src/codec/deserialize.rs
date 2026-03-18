use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::de::DeserializeOwned;
use serde_json::{Map, Number, Value};
use uuid::Uuid;

use crate::{
    entity::{DynamicEntity, EntityProperty},
    error::{Result, SerializationError},
};

use super::{
    datetime::{format_datetime, parse_datetime},
    edm::EdmType,
};

pub(crate) fn table_names_from_body(body: &[u8]) -> Result<Vec<String>> {
    #[derive(serde::Deserialize)]
    struct TableEntry {
        #[serde(rename = "TableName")]
        table_name: String,
    }

    #[derive(serde::Deserialize)]
    struct Envelope {
        value: Vec<TableEntry>,
    }

    let envelope: Envelope = serde_json::from_slice(body).map_err(SerializationError::from)?;
    Ok(envelope
        .value
        .into_iter()
        .map(|entry| entry.table_name)
        .collect())
}

pub(crate) fn extract_query_values(body: &[u8]) -> Result<Vec<Value>> {
    #[derive(serde::Deserialize)]
    struct Envelope {
        value: Vec<Value>,
    }

    let envelope: Envelope = serde_json::from_slice(body).map_err(SerializationError::from)?;
    Ok(envelope.value)
}

pub(crate) fn dynamic_entity_from_body(body: &[u8], etag: Option<String>) -> Result<DynamicEntity> {
    let value: Value = serde_json::from_slice(body).map_err(SerializationError::from)?;
    dynamic_entity_from_value(value, etag)
}

pub(crate) fn dynamic_entity_from_value(
    value: Value,
    etag: Option<String>,
) -> Result<DynamicEntity> {
    let object = match value {
        Value::Object(object) => object,
        _ => {
            return Err(SerializationError::UnsupportedShape(
                "entity payload must be a JSON object".to_owned(),
            )
            .into());
        }
    };

    let annotations = object
        .iter()
        .filter_map(|(key, value)| {
            key.strip_suffix("@odata.type").and_then(|property_name| {
                value
                    .as_str()
                    .and_then(EdmType::from_annotation)
                    .map(|edm_type| (property_name.to_owned(), edm_type))
            })
        })
        .collect::<std::collections::HashMap<_, _>>();

    let partition_key = object
        .get("PartitionKey")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            SerializationError::UnsupportedShape(
                "entity payload is missing PartitionKey".to_owned(),
            )
        })?
        .to_owned();
    let row_key = object
        .get("RowKey")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            SerializationError::UnsupportedShape("entity payload is missing RowKey".to_owned())
        })?
        .to_owned();
    let timestamp = object
        .get("Timestamp")
        .and_then(Value::as_str)
        .map(parse_datetime)
        .transpose()?;

    let mut entity = DynamicEntity::new(partition_key, row_key);
    entity.etag = etag;
    entity.timestamp = timestamp;

    for (name, value) in object {
        if matches!(name.as_str(), "PartitionKey" | "RowKey" | "Timestamp") {
            continue;
        }
        if name.ends_with("@odata.type") || name == "odata.metadata" {
            continue;
        }
        entity.insert_property(
            name.clone(),
            json_to_entity_property(value, annotations.get(&name).copied())?,
        );
    }

    Ok(entity)
}

pub(crate) fn typed_entity_from_dynamic<T>(entity: DynamicEntity) -> Result<T>
where
    T: DeserializeOwned,
{
    let mut object = Map::new();
    object.insert(
        "PartitionKey".to_owned(),
        Value::String(entity.partition_key.clone()),
    );
    object.insert(
        "partition_key".to_owned(),
        Value::String(entity.partition_key.clone()),
    );
    object.insert("RowKey".to_owned(), Value::String(entity.row_key.clone()));
    object.insert("row_key".to_owned(), Value::String(entity.row_key.clone()));
    if let Some(etag) = &entity.etag {
        object.insert("etag".to_owned(), Value::String(etag.clone()));
    }
    if let Some(timestamp) = entity.timestamp {
        let timestamp = format_datetime(&timestamp)?;
        object.insert("Timestamp".to_owned(), Value::String(timestamp.clone()));
        object.insert("timestamp".to_owned(), Value::String(timestamp));
    }

    for (name, property) in entity.properties {
        object.insert(name, entity_property_to_typed_json(property)?);
    }

    serde_json::from_value(Value::Object(object))
        .map_err(SerializationError::from)
        .map_err(Into::into)
}

fn json_to_entity_property(value: Value, edm_type: Option<EdmType>) -> Result<EntityProperty> {
    match edm_type {
        Some(EdmType::Guid) => {
            let value = value.as_str().ok_or_else(|| {
                SerializationError::UnsupportedShape("Edm.Guid must be a string".to_owned())
            })?;
            Ok(EntityProperty::Guid(Uuid::parse_str(value).map_err(
                |error| SerializationError::UnsupportedShape(error.to_string()),
            )?))
        }
        Some(EdmType::Binary) => {
            let value = value.as_str().ok_or_else(|| {
                SerializationError::UnsupportedShape("Edm.Binary must be a string".to_owned())
            })?;
            Ok(EntityProperty::Binary(STANDARD.decode(value).map_err(
                |error| SerializationError::UnsupportedShape(error.to_string()),
            )?))
        }
        Some(EdmType::DateTime) => {
            let value = value.as_str().ok_or_else(|| {
                SerializationError::UnsupportedShape("Edm.DateTime must be a string".to_owned())
            })?;
            Ok(EntityProperty::DateTime(parse_datetime(value)?))
        }
        Some(EdmType::Int64) => {
            let value = if let Some(value) = value.as_str() {
                value
                    .parse::<i64>()
                    .map_err(|error| SerializationError::UnsupportedShape(error.to_string()))?
            } else if let Some(value) = value.as_i64() {
                value
            } else {
                return Err(SerializationError::UnsupportedShape(
                    "Edm.Int64 must be a string or integer".to_owned(),
                )
                .into());
            };
            Ok(EntityProperty::Int64(value))
        }
        _ => match value {
            Value::Bool(value) => Ok(EntityProperty::Bool(value)),
            Value::Number(number) => {
                if let Some(value) = number.as_i64() {
                    if let Ok(value) = i32::try_from(value) {
                        return Ok(EntityProperty::Int32(value));
                    }
                    return Ok(EntityProperty::Int64(value));
                }
                Ok(EntityProperty::Double(number.as_f64().ok_or_else(
                    || SerializationError::UnsupportedShape("invalid number".to_owned()),
                )?))
            }
            Value::String(value) => Ok(EntityProperty::String(value)),
            Value::Null => Err(SerializationError::UnsupportedShape(
                "null properties are not persisted by Azure Table Storage".to_owned(),
            )
            .into()),
            Value::Array(_) | Value::Object(_) => Err(SerializationError::UnsupportedShape(
                "unsupported property value in entity payload".to_owned(),
            )
            .into()),
        },
    }
}

fn entity_property_to_typed_json(property: EntityProperty) -> Result<Value> {
    match property {
        EntityProperty::String(value) => Ok(Value::String(value)),
        EntityProperty::Bool(value) => Ok(Value::Bool(value)),
        EntityProperty::Int32(value) => Ok(Value::Number(Number::from(value))),
        EntityProperty::Int64(value) => Ok(Value::Number(Number::from(value))),
        EntityProperty::Double(value) => {
            Ok(Value::Number(Number::from_f64(value).ok_or_else(|| {
                SerializationError::UnsupportedShape(
                    "non-finite floating point values are not supported".to_owned(),
                )
            })?))
        }
        EntityProperty::Binary(value) => Ok(Value::Array(
            value
                .into_iter()
                .map(|byte| Value::Number(Number::from(byte)))
                .collect(),
        )),
        EntityProperty::Guid(value) => Ok(Value::String(value.to_string())),
        EntityProperty::DateTime(value) => Ok(Value::String(format_datetime(&value)?)),
    }
}
