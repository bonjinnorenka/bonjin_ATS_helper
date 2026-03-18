use bytes::Bytes;
use http::{HeaderMap, HeaderValue, Method, StatusCode};

use crate::{
    codec::{
        deserialize::{
            dynamic_entity_from_body, dynamic_entity_from_value, extract_query_values,
            typed_entity_from_dynamic,
        },
        serialize::{dynamic_entity_to_body, typed_entity_to_body},
    },
    entity::{DynamicEntity, TableEntity},
    error::{Result, ensure_status},
    query::{ContinuationToken, OriginalQuery, Query, QueryPage},
    request::headers::IF_MATCH,
    validation::{
        key::{validate_partition_key, validate_row_key},
        table_name::validate_table_name,
    },
};

use super::TableServiceClient;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IfMatch {
    Any,
    Etag(String),
}

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
        let url = self
            .service
            .join_relative(&format!("Tables('{}')", self.table_name))?;
        let prepared =
            self.service
                .prepare_request(Method::GET, url, Bytes::new(), None, HeaderMap::new())?;
        let response = self.service.send(prepared).await?;
        match response.status {
            StatusCode::OK => Ok(true),
            StatusCode::NOT_FOUND => Ok(false),
            _ => {
                ensure_status(response, &[StatusCode::OK], "check table existence")?;
                Ok(true)
            }
        }
    }

    pub async fn insert_entity<T>(&self, entity: &T) -> Result<()>
    where
        T: TableEntity,
    {
        let body = typed_entity_to_body(entity)?;
        self.send_entity_write(Method::POST, self.table_collection_url()?, body, None, true)
            .await
    }

    pub async fn insert_dynamic_entity(&self, entity: &DynamicEntity) -> Result<()> {
        let body = dynamic_entity_to_body(entity)?;
        self.send_entity_write(Method::POST, self.table_collection_url()?, body, None, true)
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

        let url = self.entity_url(partition_key, row_key)?;
        let prepared =
            self.service
                .prepare_request(Method::GET, url, Bytes::new(), None, HeaderMap::new())?;
        let response = self.service.send(prepared).await?;
        let response = ensure_status(response, &[StatusCode::OK], "get entity")?;
        let etag = response
            .headers
            .get("etag")
            .and_then(|value| value.to_str().ok())
            .map(ToOwned::to_owned);
        dynamic_entity_from_body(&response.body, etag)
    }

    pub async fn update_entity<T>(&self, entity: &T, if_match: IfMatch) -> Result<()>
    where
        T: TableEntity,
    {
        let body = typed_entity_to_body(entity)?;
        self.send_entity_write(
            Method::PUT,
            self.entity_url(entity.partition_key(), entity.row_key())?,
            body,
            Some(if_match),
            false,
        )
        .await
    }

    pub async fn update_dynamic_entity(
        &self,
        entity: &DynamicEntity,
        if_match: IfMatch,
    ) -> Result<()> {
        let body = dynamic_entity_to_body(entity)?;
        self.send_entity_write(
            Method::PUT,
            self.entity_url(&entity.partition_key, &entity.row_key)?,
            body,
            Some(if_match),
            false,
        )
        .await
    }

    pub async fn merge_entity<T>(&self, entity: &T, if_match: IfMatch) -> Result<()>
    where
        T: TableEntity,
    {
        let body = typed_entity_to_body(entity)?;
        self.send_entity_write(
            merge_method(),
            self.entity_url(entity.partition_key(), entity.row_key())?,
            body,
            Some(if_match),
            false,
        )
        .await
    }

    pub async fn merge_dynamic_entity(
        &self,
        entity: &DynamicEntity,
        if_match: IfMatch,
    ) -> Result<()> {
        let body = dynamic_entity_to_body(entity)?;
        self.send_entity_write(
            merge_method(),
            self.entity_url(&entity.partition_key, &entity.row_key)?,
            body,
            Some(if_match),
            false,
        )
        .await
    }

    pub async fn upsert_replace<T>(&self, entity: &T) -> Result<()>
    where
        T: TableEntity,
    {
        let body = typed_entity_to_body(entity)?;
        self.send_entity_write(
            Method::PUT,
            self.entity_url(entity.partition_key(), entity.row_key())?,
            body,
            None,
            false,
        )
        .await
    }

    pub async fn upsert_replace_dynamic(&self, entity: &DynamicEntity) -> Result<()> {
        let body = dynamic_entity_to_body(entity)?;
        self.send_entity_write(
            Method::PUT,
            self.entity_url(&entity.partition_key, &entity.row_key)?,
            body,
            None,
            false,
        )
        .await
    }

    pub async fn upsert_merge<T>(&self, entity: &T) -> Result<()>
    where
        T: TableEntity,
    {
        let body = typed_entity_to_body(entity)?;
        self.send_entity_write(
            merge_method(),
            self.entity_url(entity.partition_key(), entity.row_key())?,
            body,
            None,
            false,
        )
        .await
    }

    pub async fn upsert_merge_dynamic(&self, entity: &DynamicEntity) -> Result<()> {
        let body = dynamic_entity_to_body(entity)?;
        self.send_entity_write(
            merge_method(),
            self.entity_url(&entity.partition_key, &entity.row_key)?,
            body,
            None,
            false,
        )
        .await
    }

    pub async fn delete_entity(
        &self,
        partition_key: &str,
        row_key: &str,
        if_match: IfMatch,
    ) -> Result<()> {
        let mut headers = HeaderMap::new();
        headers.insert(IF_MATCH, if_match.into_header_value()?);
        let url = self.entity_url(partition_key, row_key)?;
        let prepared =
            self.service
                .prepare_request(Method::DELETE, url, Bytes::new(), None, headers)?;
        let response = self.service.send(prepared).await?;
        ensure_status(response, &[StatusCode::NO_CONTENT], "delete entity")?;
        Ok(())
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
        let mut url = self
            .service
            .join_relative(&format!("{}()", self.table_name))?;
        Query {
            original: original_query.clone(),
        }
        .append_to_url(&mut url);

        if let Some(continuation) = continuation {
            if let Some(next_partition_key) = &continuation.next_partition_key {
                url.query_pairs_mut()
                    .append_pair("NextPartitionKey", next_partition_key);
            }
            if let Some(next_row_key) = &continuation.next_row_key {
                url.query_pairs_mut()
                    .append_pair("NextRowKey", next_row_key);
            }
        }

        let prepared =
            self.service
                .prepare_request(Method::GET, url, Bytes::new(), None, HeaderMap::new())?;
        let response = self.service.send(prepared).await?;
        let response = ensure_status(response, &[StatusCode::OK], "query entities")?;
        let request_id = response
            .headers
            .get("x-ms-request-id")
            .and_then(|value| value.to_str().ok())
            .map(ToOwned::to_owned);
        let continuation =
            ContinuationToken::from_headers(&response.headers, original_query.clone());
        let items = extract_query_values(&response.body)?
            .into_iter()
            .map(|value| dynamic_entity_from_value(value, None))
            .collect::<Result<Vec<_>>>()?;

        Ok(QueryPage {
            items,
            continuation,
            request_id,
            raw_headers: response.headers,
        })
    }

    async fn send_entity_write(
        &self,
        method: Method,
        url: url::Url,
        body: Bytes,
        if_match: Option<IfMatch>,
        allow_created: bool,
    ) -> Result<()> {
        let mut headers = HeaderMap::new();
        if let Some(if_match) = if_match {
            headers.insert(IF_MATCH, if_match.into_header_value()?);
        }
        let prepared =
            self.service
                .prepare_request(method, url, body, Some("application/json"), headers)?;
        let response = self.service.send(prepared).await?;
        let mut expected = vec![StatusCode::NO_CONTENT];
        if allow_created {
            expected.push(StatusCode::CREATED);
        } else {
            expected.push(StatusCode::CREATED);
        }
        ensure_status(response, &expected, "write entity")?;
        Ok(())
    }

    fn table_collection_url(&self) -> Result<url::Url> {
        validate_table_name(&self.table_name)?;
        self.service.join_relative(&self.table_name)
    }

    fn entity_url(&self, partition_key: &str, row_key: &str) -> Result<url::Url> {
        validate_table_name(&self.table_name)?;
        validate_partition_key(partition_key)?;
        validate_row_key(row_key)?;

        self.service.join_relative(&format!(
            "{}(PartitionKey='{}',RowKey='{}')",
            self.table_name,
            escape_odata_string(partition_key),
            escape_odata_string(row_key)
        ))
    }
}

impl IfMatch {
    fn into_header_value(self) -> Result<HeaderValue> {
        let value = match self {
            Self::Any => "*".to_owned(),
            Self::Etag(value) => value,
        };

        HeaderValue::from_str(&value)
            .map_err(|error| crate::error::ValidationError::InvalidKey(error.to_string()).into())
    }
}

fn merge_method() -> Method {
    Method::from_bytes(b"MERGE").expect("MERGE is a valid HTTP method")
}

fn escape_odata_string(value: &str) -> String {
    value.replace('\'', "''")
}
