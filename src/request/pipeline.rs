use bytes::Bytes;
use http::{HeaderMap, HeaderValue, Method};
use time::{OffsetDateTime, format_description::FormatItem, macros::format_description};
use url::Url;

use crate::{
    auth::Credential,
    client::ClientOptions,
    error::Result,
    http::{reqwest_client::ReqwestTransport, response::Response},
    request::{
        canonicalization::canonicalized_resource,
        headers::{
            ACCEPT, CONTENT_TYPE, DATA_SERVICE_VERSION, DATE, MAX_DATA_SERVICE_VERSION, X_MS_DATE,
            X_MS_VERSION,
        },
        prepared_request::PreparedRequest,
    },
};

const RFC1123_FORMAT: &[FormatItem<'static>] = format_description!(
    "[weekday repr:short], [day padding:zero] [month repr:short] [year] [hour]:[minute]:[second] GMT"
);

#[derive(Clone)]
pub(crate) struct RequestPipeline {
    credential: Credential,
    options: ClientOptions,
    transport: ReqwestTransport,
}

impl RequestPipeline {
    pub(crate) fn new(credential: Credential, options: ClientOptions) -> Result<Self> {
        let transport = ReqwestTransport::new(&options)?;
        Ok(Self {
            credential,
            options,
            transport,
        })
    }

    pub(crate) fn prepare_request(
        &self,
        method: Method,
        url: Url,
        body: Bytes,
        content_type: Option<&str>,
        extra_headers: HeaderMap,
    ) -> Result<PreparedRequest> {
        let signing_date = format_http_date(OffsetDateTime::now_utc())?;
        let mut headers = HeaderMap::new();
        headers.insert(
            ACCEPT,
            HeaderValue::from_static(self.options.metadata_level.accept_header()),
        );
        headers.insert(DATA_SERVICE_VERSION, HeaderValue::from_static("3.0;NetFx"));
        headers.insert(
            MAX_DATA_SERVICE_VERSION,
            HeaderValue::from_static("3.0;NetFx"),
        );
        headers.insert(
            X_MS_VERSION,
            HeaderValue::from_str(&self.options.storage_api_version)
                .expect("storage API version is always a valid header"),
        );
        headers.insert(
            X_MS_DATE,
            HeaderValue::from_str(&signing_date).expect("formatted date is always a valid header"),
        );
        headers.insert(
            DATE,
            HeaderValue::from_str(&signing_date).expect("formatted date is always a valid header"),
        );

        let content_type = content_type.map(ToOwned::to_owned);
        if let Some(content_type) = &content_type {
            headers.insert(
                CONTENT_TYPE,
                HeaderValue::from_str(content_type)
                    .expect("content type provided by library is always valid"),
            );
        }

        headers.extend(extra_headers);
        let canonicalized_resource = self
            .credential
            .account_name()
            .map(|account_name| canonicalized_resource(account_name, &url))
            .unwrap_or_default();

        Ok(PreparedRequest {
            method,
            url,
            headers,
            body,
            content_md5: None,
            content_type,
            signing_date,
            canonicalized_resource,
        })
    }

    pub(crate) async fn send(&self, mut prepared: PreparedRequest) -> Result<Response> {
        self.credential.apply(&mut prepared)?;
        self.transport.execute(prepared).await.map_err(Into::into)
    }
}

fn format_http_date(value: OffsetDateTime) -> Result<String> {
    value
        .to_offset(time::UtcOffset::UTC)
        .format(RFC1123_FORMAT)
        .map_err(|error| crate::error::SerializationError::DateTime(error.to_string()).into())
}
