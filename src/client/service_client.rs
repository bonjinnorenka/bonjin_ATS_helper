use std::sync::Arc;

use crate::{
    auth::Credential,
    backend::{Backend, http::HttpBackend, mock::MockBackend},
    client::{ClientOptions, TableClient},
    error::Result,
    mock::MockOptions,
    validation::table_name::validate_table_name,
};

#[derive(Clone)]
pub struct TableServiceClient {
    inner: Arc<ServiceClientInner>,
}

struct ServiceClientInner {
    backend: Arc<dyn Backend>,
}

impl TableServiceClient {
    pub fn new(
        endpoint: impl AsRef<str>,
        credential: impl Into<Credential>,
        options: ClientOptions,
    ) -> Result<Self> {
        let backend = HttpBackend::new(endpoint.as_ref(), credential.into(), options)?;
        Ok(Self::from_backend(Arc::new(backend)))
    }

    pub fn new_mock(options: MockOptions) -> Result<Self> {
        let backend = MockBackend::new(options)?;
        Ok(Self::from_backend(Arc::new(backend)))
    }

    pub fn table_client(&self, table_name: impl Into<String>) -> Result<TableClient> {
        let table_name = table_name.into();
        validate_table_name(&table_name)?;
        Ok(TableClient::new(self.clone(), table_name))
    }

    pub async fn create_table(&self, table_name: &str) -> Result<()> {
        validate_table_name(table_name)?;
        self.inner.backend.create_table(table_name).await
    }

    pub async fn create_table_if_not_exists(&self, table_name: &str) -> Result<bool> {
        match self.create_table(table_name).await {
            Ok(()) => Ok(true),
            Err(crate::error::Error::Service(error))
                if error.kind == crate::error::ServiceErrorKind::TableAlreadyExists
                    || error.status == http::StatusCode::CONFLICT =>
            {
                Ok(false)
            }
            Err(error) => Err(error),
        }
    }

    pub async fn delete_table(&self, table_name: &str) -> Result<()> {
        validate_table_name(table_name)?;
        self.inner.backend.delete_table(table_name).await
    }

    pub async fn list_tables(&self) -> Result<Vec<String>> {
        self.inner.backend.list_tables().await
    }

    pub async fn flush(&self) -> Result<()> {
        self.inner.backend.flush().await
    }

    pub(crate) fn backend(&self) -> &Arc<dyn Backend> {
        &self.inner.backend
    }

    fn from_backend(backend: Arc<dyn Backend>) -> Self {
        Self {
            inner: Arc::new(ServiceClientInner { backend }),
        }
    }
}
