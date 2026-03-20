use std::{future::Future, pin::Pin};

use crate::{
    entity::DynamicEntity,
    error::Result,
    query::{ContinuationToken, OriginalQuery, QueryPage},
};

pub(crate) mod http;
pub(crate) mod mock;

pub(crate) type BackendFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T>> + Send + 'a>>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MatchCondition {
    Any,
    Etag(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UpdateMode {
    Replace,
    Merge,
}

pub(crate) trait Backend: Send + Sync {
    fn create_table(&self, table_name: &str) -> BackendFuture<'_, ()>;
    fn delete_table(&self, table_name: &str) -> BackendFuture<'_, ()>;
    fn list_tables(&self) -> BackendFuture<'_, Vec<String>>;
    fn table_exists(&self, table_name: &str) -> BackendFuture<'_, bool>;
    fn insert_entity(&self, table_name: &str, entity: DynamicEntity) -> BackendFuture<'_, ()>;
    fn get_entity(
        &self,
        table_name: &str,
        partition_key: &str,
        row_key: &str,
    ) -> BackendFuture<'_, DynamicEntity>;
    fn update_entity(
        &self,
        table_name: &str,
        entity: DynamicEntity,
        if_match: MatchCondition,
        mode: UpdateMode,
    ) -> BackendFuture<'_, ()>;
    fn upsert_entity(
        &self,
        table_name: &str,
        entity: DynamicEntity,
        mode: UpdateMode,
    ) -> BackendFuture<'_, ()>;
    fn delete_entity(
        &self,
        table_name: &str,
        partition_key: &str,
        row_key: &str,
        if_match: MatchCondition,
    ) -> BackendFuture<'_, ()>;
    fn query_entities(
        &self,
        table_name: &str,
        query: OriginalQuery,
        continuation: Option<ContinuationToken>,
    ) -> BackendFuture<'_, QueryPage<DynamicEntity>>;
    fn flush(&self) -> BackendFuture<'_, ()>;
}
