use indexmap::IndexMap;

use crate::entity::DynamicEntity;

pub(crate) fn apply_select(entity: &DynamicEntity, select: &[String]) -> DynamicEntity {
    if select.is_empty() {
        return entity.clone();
    }

    let mut projected = DynamicEntity::new(&entity.partition_key, &entity.row_key);
    projected.etag = entity.etag.clone();
    projected.timestamp = entity.timestamp;
    projected.properties = select
        .iter()
        .filter_map(|name| {
            entity
                .properties
                .get(name)
                .cloned()
                .map(|value| (name.clone(), value))
        })
        .collect::<IndexMap<_, _>>();
    projected
}
