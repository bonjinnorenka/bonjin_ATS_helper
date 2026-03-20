use std::sync::Arc;

use bytes::Bytes;
use http::{HeaderMap, Method, StatusCode};
use url::{Host, Url};

use crate::{
    auth::Credential,
    backend::{Backend, BackendFuture, MatchCondition, UpdateMode},
    client::ClientOptions,
    codec::{
        deserialize::{
            dynamic_entity_from_body, dynamic_entity_from_value, extract_query_values,
            table_names_from_body,
        },
        serialize::dynamic_entity_to_body,
    },
    entity::DynamicEntity,
    error::{Result, ValidationError, ensure_status},
    query::{ContinuationToken, OriginalQuery, Query, QueryPage},
    request::{headers::IF_MATCH, pipeline::RequestPipeline},
};

#[derive(Clone)]
pub(crate) struct HttpBackend {
    inner: Arc<HttpBackendInner>,
}

struct HttpBackendInner {
    endpoint: Url,
    pipeline: RequestPipeline,
}

impl HttpBackend {
    pub(crate) fn new(
        endpoint: &str,
        credential: Credential,
        options: ClientOptions,
    ) -> Result<Self> {
        let endpoint = normalize_endpoint(endpoint, options.allow_insecure_http)?;
        let pipeline = RequestPipeline::new(credential, options)?;

        Ok(Self {
            inner: Arc::new(HttpBackendInner { endpoint, pipeline }),
        })
    }

    fn join_relative(&self, relative: &str) -> Result<Url> {
        self.inner
            .endpoint
            .join(relative)
            .map_err(|error| ValidationError::InvalidEndpoint(error.to_string()).into())
    }

    async fn send(
        &self,
        prepared: crate::request::prepared_request::PreparedRequest,
    ) -> Result<crate::http::response::Response> {
        self.inner.pipeline.send(prepared).await
    }

    fn prepare_request(
        &self,
        method: Method,
        url: Url,
        body: Bytes,
        content_type: Option<&str>,
        extra_headers: HeaderMap,
    ) -> Result<crate::request::prepared_request::PreparedRequest> {
        self.inner
            .pipeline
            .prepare_request(method, url, body, content_type, extra_headers)
    }

    async fn send_entity_write(
        &self,
        method: Method,
        url: Url,
        body: Bytes,
        if_match: Option<MatchCondition>,
        allow_created: bool,
    ) -> Result<()> {
        let mut headers = HeaderMap::new();
        if let Some(if_match) = if_match {
            headers.insert(IF_MATCH, match_condition_to_header(if_match)?);
        }
        let prepared =
            self.prepare_request(method, url, body, Some("application/json"), headers)?;
        let response = self.send(prepared).await?;
        let mut expected = vec![StatusCode::NO_CONTENT];
        if allow_created {
            expected.push(StatusCode::CREATED);
        }
        ensure_status(response, &expected, "write entity")?;
        Ok(())
    }

    fn entity_url(&self, table_name: &str, partition_key: &str, row_key: &str) -> Result<Url> {
        self.join_relative(&format!(
            "{}(PartitionKey='{}',RowKey='{}')",
            table_name,
            escape_odata_string(partition_key),
            escape_odata_string(row_key)
        ))
    }
}

impl Backend for HttpBackend {
    fn create_table(&self, table_name: &str) -> BackendFuture<'_, ()> {
        let table_name = table_name.to_owned();
        Box::pin(async move {
            let url = self.join_relative("Tables")?;
            let body = Bytes::from(
                serde_json::to_vec(&serde_json::json!({
                    "TableName": table_name
                }))
                .map_err(crate::error::SerializationError::from)?,
            );
            let prepared = self.prepare_request(
                Method::POST,
                url,
                body,
                Some("application/json"),
                HeaderMap::new(),
            )?;
            let response = self.send(prepared).await?;
            ensure_status(response, &[StatusCode::CREATED], "create table")?;
            Ok(())
        })
    }

    fn delete_table(&self, table_name: &str) -> BackendFuture<'_, ()> {
        let table_name = table_name.to_owned();
        Box::pin(async move {
            let url = self.join_relative(&format!("Tables('{table_name}')"))?;
            let prepared =
                self.prepare_request(Method::DELETE, url, Bytes::new(), None, HeaderMap::new())?;
            let response = self.send(prepared).await?;
            ensure_status(response, &[StatusCode::NO_CONTENT], "delete table")?;
            Ok(())
        })
    }

    fn list_tables(&self) -> BackendFuture<'_, Vec<String>> {
        Box::pin(async move {
            let url = self.join_relative("Tables")?;
            let prepared =
                self.prepare_request(Method::GET, url, Bytes::new(), None, HeaderMap::new())?;
            let response = self.send(prepared).await?;
            let response = ensure_status(response, &[StatusCode::OK], "list tables")?;
            table_names_from_body(&response.body)
        })
    }

    fn table_exists(&self, table_name: &str) -> BackendFuture<'_, bool> {
        let table_name = table_name.to_owned();
        Box::pin(async move {
            let url = self.join_relative(&format!("Tables('{}')", table_name))?;
            let prepared =
                self.prepare_request(Method::GET, url, Bytes::new(), None, HeaderMap::new())?;
            let response = self.send(prepared).await?;
            match response.status {
                StatusCode::OK => Ok(true),
                StatusCode::NOT_FOUND => Ok(false),
                _ => {
                    ensure_status(response, &[StatusCode::OK], "check table existence")?;
                    Ok(true)
                }
            }
        })
    }

    fn insert_entity(&self, table_name: &str, entity: DynamicEntity) -> BackendFuture<'_, ()> {
        let table_name = table_name.to_owned();
        Box::pin(async move {
            let body = dynamic_entity_to_body(&entity)?;
            let url = self.join_relative(&table_name)?;
            self.send_entity_write(Method::POST, url, body, None, true)
                .await
        })
    }

    fn get_entity(
        &self,
        table_name: &str,
        partition_key: &str,
        row_key: &str,
    ) -> BackendFuture<'_, DynamicEntity> {
        let table_name = table_name.to_owned();
        let partition_key = partition_key.to_owned();
        let row_key = row_key.to_owned();
        Box::pin(async move {
            let url = self.entity_url(&table_name, &partition_key, &row_key)?;
            let prepared =
                self.prepare_request(Method::GET, url, Bytes::new(), None, HeaderMap::new())?;
            let response = self.send(prepared).await?;
            let response = ensure_status(response, &[StatusCode::OK], "get entity")?;
            let etag = response
                .headers
                .get("etag")
                .and_then(|value| value.to_str().ok())
                .map(ToOwned::to_owned);
            dynamic_entity_from_body(&response.body, etag)
        })
    }

    fn update_entity(
        &self,
        table_name: &str,
        entity: DynamicEntity,
        if_match: MatchCondition,
        mode: UpdateMode,
    ) -> BackendFuture<'_, ()> {
        let table_name = table_name.to_owned();
        Box::pin(async move {
            let body = dynamic_entity_to_body(&entity)?;
            let url = self.entity_url(&table_name, &entity.partition_key, &entity.row_key)?;
            let method = match mode {
                UpdateMode::Replace => Method::PUT,
                UpdateMode::Merge => merge_method(),
            };
            self.send_entity_write(method, url, body, Some(if_match), false)
                .await
        })
    }

    fn upsert_entity(
        &self,
        table_name: &str,
        entity: DynamicEntity,
        mode: UpdateMode,
    ) -> BackendFuture<'_, ()> {
        let table_name = table_name.to_owned();
        Box::pin(async move {
            let body = dynamic_entity_to_body(&entity)?;
            let url = self.entity_url(&table_name, &entity.partition_key, &entity.row_key)?;
            let method = match mode {
                UpdateMode::Replace => Method::PUT,
                UpdateMode::Merge => merge_method(),
            };
            self.send_entity_write(method, url, body, None, false).await
        })
    }

    fn delete_entity(
        &self,
        table_name: &str,
        partition_key: &str,
        row_key: &str,
        if_match: MatchCondition,
    ) -> BackendFuture<'_, ()> {
        let table_name = table_name.to_owned();
        let partition_key = partition_key.to_owned();
        let row_key = row_key.to_owned();
        Box::pin(async move {
            let mut headers = HeaderMap::new();
            headers.insert(IF_MATCH, match_condition_to_header(if_match)?);
            let url = self.entity_url(&table_name, &partition_key, &row_key)?;
            let prepared =
                self.prepare_request(Method::DELETE, url, Bytes::new(), None, headers)?;
            let response = self.send(prepared).await?;
            ensure_status(response, &[StatusCode::NO_CONTENT], "delete entity")?;
            Ok(())
        })
    }

    fn query_entities(
        &self,
        table_name: &str,
        query: OriginalQuery,
        continuation: Option<ContinuationToken>,
    ) -> BackendFuture<'_, QueryPage<DynamicEntity>> {
        let table_name = table_name.to_owned();
        Box::pin(async move {
            let mut url = self.join_relative(&format!("{}()", table_name))?;
            Query {
                original: query.clone(),
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
                self.prepare_request(Method::GET, url, Bytes::new(), None, HeaderMap::new())?;
            let response = self.send(prepared).await?;
            let response = ensure_status(response, &[StatusCode::OK], "query entities")?;
            let request_id = response
                .headers
                .get("x-ms-request-id")
                .and_then(|value| value.to_str().ok())
                .map(ToOwned::to_owned);
            let continuation = ContinuationToken::from_headers(&response.headers, query.clone());
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
        })
    }

    fn flush(&self) -> BackendFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }
}

fn normalize_endpoint(raw: &str, allow_insecure_http: bool) -> Result<Url> {
    let mut endpoint =
        Url::parse(raw).map_err(|error| ValidationError::InvalidEndpoint(error.to_string()))?;

    match endpoint.scheme() {
        "https" => {}
        "http" if allow_insecure_http || is_loopback_host(&endpoint) => {}
        "http" => {
            return Err(ValidationError::InvalidEndpoint(
                "http endpoints are only allowed for loopback hosts or when ClientOptions::with_insecure_http_allowed(true) is set".to_owned(),
            )
            .into());
        }
        _ => {
            return Err(ValidationError::InvalidEndpoint(
                "endpoint must use http or https".to_owned(),
            )
            .into());
        }
    }

    let normalized_path = if endpoint.path().is_empty() || endpoint.path() == "/" {
        "/".to_owned()
    } else {
        format!("{}/", endpoint.path().trim_end_matches('/'))
    };
    endpoint.set_path(&normalized_path);

    Ok(endpoint)
}

fn is_loopback_host(endpoint: &Url) -> bool {
    match endpoint.host() {
        Some(Host::Domain(domain)) => domain.eq_ignore_ascii_case("localhost"),
        Some(Host::Ipv4(address)) => address.is_loopback(),
        Some(Host::Ipv6(address)) => address.is_loopback(),
        None => false,
    }
}

fn match_condition_to_header(condition: MatchCondition) -> Result<http::HeaderValue> {
    let value = match condition {
        MatchCondition::Any => "*".to_owned(),
        MatchCondition::Etag(value) => value,
    };

    http::HeaderValue::from_str(&value)
        .map_err(|error| crate::error::ValidationError::InvalidKey(error.to_string()).into())
}

fn merge_method() -> Method {
    Method::from_bytes(b"MERGE").expect("MERGE is a valid HTTP method")
}

fn escape_odata_string(value: &str) -> String {
    value.replace('\'', "''")
}

#[cfg(test)]
mod tests {
    use crate::{auth::SasCredential, client::ClientOptions, error::ValidationError};

    use super::HttpBackend;

    #[test]
    fn normalizes_endpoints_with_trailing_slash() {
        let client = HttpBackend::new(
            "https://example.table.core.windows.net",
            SasCredential::new("sv=1&sig=abc").unwrap().into(),
            ClientOptions::default(),
        )
        .unwrap();

        assert_eq!(
            client.join_relative("Tables").unwrap().as_str(),
            "https://example.table.core.windows.net/Tables"
        );
    }

    #[test]
    fn rejects_non_loopback_http_by_default() {
        let result = HttpBackend::new(
            "http://example.table.core.windows.net",
            SasCredential::new("sv=1&sig=abc").unwrap().into(),
            ClientOptions::default(),
        );

        assert!(matches!(
            result,
            Err(crate::error::Error::Validation(ValidationError::InvalidEndpoint(message)))
                if message.contains("with_insecure_http_allowed")
        ));
    }

    #[test]
    fn allows_non_loopback_http_when_explicitly_enabled() {
        let client = HttpBackend::new(
            "http://example.table.core.windows.net",
            SasCredential::new("sv=1&sig=abc").unwrap().into(),
            ClientOptions::default().with_insecure_http_allowed(true),
        )
        .unwrap();

        assert_eq!(
            client.join_relative("Tables").unwrap().as_str(),
            "http://example.table.core.windows.net/Tables"
        );
    }

    #[test]
    fn allows_loopback_http_for_local_emulators() {
        let client = HttpBackend::new(
            "http://127.0.0.1:10002/devstoreaccount1",
            SasCredential::new("sv=1&sig=abc").unwrap().into(),
            ClientOptions::default(),
        )
        .unwrap();

        assert_eq!(
            client.join_relative("Tables").unwrap().as_str(),
            "http://127.0.0.1:10002/devstoreaccount1/Tables"
        );
    }
}
