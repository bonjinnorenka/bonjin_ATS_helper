use crate::{
    backend::{MatchCondition, UpdateMode},
    codec::{deserialize::typed_entity_from_dynamic, serialize::typed_entity_to_dynamic},
    entity::{DynamicEntity, TableEntity},
    error::Result,
    query::{ContinuationToken, OriginalQuery, Query, QueryPage},
    validation::key::{validate_partition_key, validate_row_key},
};

use super::IfMatch;
use super::TableServiceClient;

#[derive(Clone)]
pub struct TableClient {
    service: TableServiceClient,
    table_name: String,
}

impl TableClient {
    pub(crate) fn new(service: TableServiceClient, table_name: String) -> Self {
        Self {
            service,
            table_name,
        }
    }

    pub fn table_name(&self) -> &str {
        &self.table_name
    }

    pub async fn create_if_not_exists(&self) -> Result<bool> {
        self.service
            .create_table_if_not_exists(&self.table_name)
            .await
    }

    pub async fn delete(&self) -> Result<()> {
        self.service.delete_table(&self.table_name).await
    }

    pub async fn exists(&self) -> Result<bool> {
        self.service.backend().table_exists(&self.table_name).await
    }

    pub async fn insert_entity<T>(&self, entity: &T) -> Result<()>
    where
        T: TableEntity,
    {
        let entity = typed_entity_to_dynamic(entity)?;
        self.service
            .backend()
            .insert_entity(&self.table_name, entity)
            .await
    }

    pub async fn insert_dynamic_entity(&self, entity: &DynamicEntity) -> Result<()> {
        self.service
            .backend()
            .insert_entity(&self.table_name, entity.clone())
            .await
    }

    pub async fn get_entity<T>(&self, partition_key: &str, row_key: &str) -> Result<T>
    where
        T: TableEntity,
    {
        let entity = self.get_dynamic_entity(partition_key, row_key).await?;
        typed_entity_from_dynamic(entity)
    }

    pub async fn get_dynamic_entity(
        &self,
        partition_key: &str,
        row_key: &str,
    ) -> Result<DynamicEntity> {
        validate_partition_key(partition_key)?;
        validate_row_key(row_key)?;
        self.service
            .backend()
            .get_entity(&self.table_name, partition_key, row_key)
            .await
    }

    pub async fn update_entity<T>(&self, entity: &T, if_match: IfMatch) -> Result<()>
    where
        T: TableEntity,
    {
        let entity = typed_entity_to_dynamic(entity)?;
        self.service
            .backend()
            .update_entity(
                &self.table_name,
                entity,
                if_match.into_match_condition(),
                UpdateMode::Replace,
            )
            .await
    }

    pub async fn update_dynamic_entity(
        &self,
        entity: &DynamicEntity,
        if_match: IfMatch,
    ) -> Result<()> {
        self.service
            .backend()
            .update_entity(
                &self.table_name,
                entity.clone(),
                if_match.into_match_condition(),
                UpdateMode::Replace,
            )
            .await
    }

    pub async fn merge_entity<T>(&self, entity: &T, if_match: IfMatch) -> Result<()>
    where
        T: TableEntity,
    {
        let entity = typed_entity_to_dynamic(entity)?;
        self.service
            .backend()
            .update_entity(
                &self.table_name,
                entity,
                if_match.into_match_condition(),
                UpdateMode::Merge,
            )
            .await
    }

    pub async fn merge_dynamic_entity(
        &self,
        entity: &DynamicEntity,
        if_match: IfMatch,
    ) -> Result<()> {
        self.service
            .backend()
            .update_entity(
                &self.table_name,
                entity.clone(),
                if_match.into_match_condition(),
                UpdateMode::Merge,
            )
            .await
    }

    pub async fn upsert_replace<T>(&self, entity: &T) -> Result<()>
    where
        T: TableEntity,
    {
        let entity = typed_entity_to_dynamic(entity)?;
        self.service
            .backend()
            .upsert_entity(&self.table_name, entity, UpdateMode::Replace)
            .await
    }

    pub async fn upsert_replace_dynamic(&self, entity: &DynamicEntity) -> Result<()> {
        self.service
            .backend()
            .upsert_entity(&self.table_name, entity.clone(), UpdateMode::Replace)
            .await
    }

    pub async fn upsert_merge<T>(&self, entity: &T) -> Result<()>
    where
        T: TableEntity,
    {
        let entity = typed_entity_to_dynamic(entity)?;
        self.service
            .backend()
            .upsert_entity(&self.table_name, entity, UpdateMode::Merge)
            .await
    }

    pub async fn upsert_merge_dynamic(&self, entity: &DynamicEntity) -> Result<()> {
        self.service
            .backend()
            .upsert_entity(&self.table_name, entity.clone(), UpdateMode::Merge)
            .await
    }

    pub async fn delete_entity(
        &self,
        partition_key: &str,
        row_key: &str,
        if_match: IfMatch,
    ) -> Result<()> {
        self.service
            .backend()
            .delete_entity(
                &self.table_name,
                partition_key,
                row_key,
                if_match.into_match_condition(),
            )
            .await
    }

    pub async fn query_entities<T>(&self, query: Query) -> Result<QueryPage<T>>
    where
        T: TableEntity,
    {
        self.query_page(query.original_query().clone(), None).await
    }

    pub async fn query_entities_next<T>(
        &self,
        continuation: &ContinuationToken,
    ) -> Result<QueryPage<T>>
    where
        T: TableEntity,
    {
        self.query_page(continuation.original_query.clone(), Some(continuation))
            .await
    }

    pub async fn query_dynamic_entities(&self, query: Query) -> Result<QueryPage<DynamicEntity>> {
        self.query_dynamic_page(query.original_query().clone(), None)
            .await
    }

    pub async fn query_dynamic_entities_next(
        &self,
        continuation: &ContinuationToken,
    ) -> Result<QueryPage<DynamicEntity>> {
        self.query_dynamic_page(continuation.original_query.clone(), Some(continuation))
            .await
    }

    async fn query_page<T>(
        &self,
        original_query: OriginalQuery,
        continuation: Option<&ContinuationToken>,
    ) -> Result<QueryPage<T>>
    where
        T: TableEntity,
    {
        let page = self
            .query_dynamic_page(original_query, continuation)
            .await?;
        let items = page
            .items
            .into_iter()
            .map(typed_entity_from_dynamic)
            .collect::<Result<Vec<_>>>()?;

        Ok(QueryPage {
            items,
            continuation: page.continuation,
            request_id: page.request_id,
            raw_headers: page.raw_headers,
        })
    }

    async fn query_dynamic_page(
        &self,
        original_query: OriginalQuery,
        continuation: Option<&ContinuationToken>,
    ) -> Result<QueryPage<DynamicEntity>> {
        self.service
            .backend()
            .query_entities(&self.table_name, original_query, continuation.cloned())
            .await
    }
}

impl IfMatch {
    fn into_match_condition(self) -> MatchCondition {
        match self {
            Self::Any => MatchCondition::Any,
            Self::Etag(value) => MatchCondition::Etag(value),
        }
    }
}
