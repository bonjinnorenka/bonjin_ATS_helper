use indexmap::IndexMap;
use time::OffsetDateTime;

use super::{EntityProperty, EntitySystemProperties};

#[derive(Debug, Clone, PartialEq)]
pub struct DynamicEntity {
    pub partition_key: String,
    pub row_key: String,
    pub properties: IndexMap<String, EntityProperty>,
    pub etag: Option<String>,
    pub timestamp: Option<OffsetDateTime>,
}

impl DynamicEntity {
    pub fn new(partition_key: impl Into<String>, row_key: impl Into<String>) -> Self {
        Self {
            partition_key: partition_key.into(),
            row_key: row_key.into(),
            properties: IndexMap::new(),
            etag: None,
            timestamp: None,
        }
    }

    pub fn insert_property(
        &mut self,
        name: impl Into<String>,
        property: EntityProperty,
    ) -> Option<EntityProperty> {
        self.properties.insert(name.into(), property)
    }

    pub fn system_properties(&self) -> EntitySystemProperties {
        EntitySystemProperties {
            partition_key: self.partition_key.clone(),
            row_key: self.row_key.clone(),
            etag: self.etag.clone(),
            timestamp: self.timestamp,
        }
    }
}
