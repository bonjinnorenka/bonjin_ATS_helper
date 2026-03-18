use std::sync::Arc;

use bytes::Bytes;
use http::{HeaderMap, Method, StatusCode};
use url::Url;

use crate::{
    auth::Credential,
    client::{ClientOptions, TableClient},
    codec::deserialize::table_names_from_body,
    error::{Result, SerializationError, ServiceErrorKind, ValidationError, ensure_status},
    request::pipeline::RequestPipeline,
    validation::table_name::validate_table_name,
};

#[derive(Clone)]
pub struct TableServiceClient {
    inner: Arc<ServiceClientInner>,
}

struct ServiceClientInner {
    endpoint: Url,
    pipeline: RequestPipeline,
}

impl TableServiceClient {
    pub fn new(
        endpoint: impl AsRef<str>,
        credential: impl Into<Credential>,
        options: ClientOptions,
    ) -> Result<Self> {
        let endpoint = normalize_endpoint(endpoint.as_ref())?;
        let pipeline = RequestPipeline::new(credential.into(), options)?;

        Ok(Self {
            inner: Arc::new(ServiceClientInner { endpoint, pipeline }),
        })
    }

    pub fn table_client(&self, table_name: impl Into<String>) -> Result<TableClient> {
        let table_name = table_name.into();
        validate_table_name(&table_name)?;
        Ok(TableClient::new(self.clone(), table_name))
    }

    pub async fn create_table(&self, table_name: &str) -> Result<()> {
        validate_table_name(table_name)?;
        let url = self.join_relative("Tables")?;
        let body = Bytes::from(
            serde_json::to_vec(&serde_json::json!({
                "TableName": table_name
            }))
            .map_err(SerializationError::from)?,
        );
        let prepared = self.inner.pipeline.prepare_request(
            Method::POST,
            url,
            body,
            Some("application/json"),
            HeaderMap::new(),
        )?;
        let response = self.inner.pipeline.send(prepared).await?;
        ensure_status(response, &[StatusCode::CREATED], "create table")?;
        Ok(())
    }

    pub async fn create_table_if_not_exists(&self, table_name: &str) -> Result<bool> {
        match self.create_table(table_name).await {
            Ok(()) => Ok(true),
            Err(crate::error::Error::Service(error))
                if error.kind == ServiceErrorKind::TableAlreadyExists
                    || error.status == StatusCode::CONFLICT =>
            {
                Ok(false)
            }
            Err(error) => Err(error),
        }
    }

    pub async fn delete_table(&self, table_name: &str) -> Result<()> {
        validate_table_name(table_name)?;
        let url = self.join_relative(&format!("Tables('{table_name}')"))?;
        let prepared = self.inner.pipeline.prepare_request(
            Method::DELETE,
            url,
            Bytes::new(),
            None,
            HeaderMap::new(),
        )?;
        let response = self.inner.pipeline.send(prepared).await?;
        ensure_status(response, &[StatusCode::NO_CONTENT], "delete table")?;
        Ok(())
    }

    pub async fn list_tables(&self) -> Result<Vec<String>> {
        let url = self.join_relative("Tables")?;
        let prepared = self.inner.pipeline.prepare_request(
            Method::GET,
            url,
            Bytes::new(),
            None,
            HeaderMap::new(),
        )?;
        let response = self.inner.pipeline.send(prepared).await?;
        let response = ensure_status(response, &[StatusCode::OK], "list tables")?;
        table_names_from_body(&response.body)
    }

    pub(crate) fn join_relative(&self, relative: &str) -> Result<Url> {
        self.inner
            .endpoint
            .join(relative)
            .map_err(|error| ValidationError::InvalidEndpoint(error.to_string()).into())
    }

    pub(crate) async fn send(
        &self,
        prepared: crate::request::prepared_request::PreparedRequest,
    ) -> Result<crate::http::response::Response> {
        self.inner.pipeline.send(prepared).await
    }

    pub(crate) fn prepare_request(
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
}

fn normalize_endpoint(raw: &str) -> Result<Url> {
    let mut endpoint =
        Url::parse(raw).map_err(|error| ValidationError::InvalidEndpoint(error.to_string()))?;

    if !matches!(endpoint.scheme(), "http" | "https") {
        return Err(
            ValidationError::InvalidEndpoint("endpoint must use http or https".to_owned()).into(),
        );
    }

    let normalized_path = if endpoint.path().is_empty() || endpoint.path() == "/" {
        "/".to_owned()
    } else {
        format!("{}/", endpoint.path().trim_end_matches('/'))
    };
    endpoint.set_path(&normalized_path);

    Ok(endpoint)
}

#[cfg(test)]
mod tests {
    use crate::{auth::SasCredential, client::ClientOptions};

    use super::TableServiceClient;

    #[test]
    fn normalizes_endpoints_with_trailing_slash() {
        let client = TableServiceClient::new(
            "https://example.table.core.windows.net",
            SasCredential::new("sv=1&sig=abc").unwrap(),
            ClientOptions::default(),
        )
        .unwrap();

        assert_eq!(
            client.join_relative("Tables").unwrap().as_str(),
            "https://example.table.core.windows.net/Tables"
        );
    }
}
