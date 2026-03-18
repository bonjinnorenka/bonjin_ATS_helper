use crate::{
    client::ClientOptions, error::TransportError, http::response::Response,
    request::prepared_request::PreparedRequest,
};

#[derive(Clone)]
pub(crate) struct ReqwestTransport {
    client: reqwest::Client,
}

impl ReqwestTransport {
    pub(crate) fn new(options: &ClientOptions) -> Result<Self, TransportError> {
        let mut builder = reqwest::Client::builder();

        if let Some(timeout) = options.timeout {
            builder = builder.timeout(timeout);
        }
        if let Some(timeout) = options.connect_timeout {
            builder = builder.connect_timeout(timeout);
        }
        if let Some(user_agent) = &options.user_agent {
            builder = builder.user_agent(user_agent.clone());
        }

        let client = builder.build().map_err(TransportError::from)?;
        Ok(Self { client })
    }

    pub(crate) async fn execute(
        &self,
        prepared: PreparedRequest,
    ) -> Result<Response, TransportError> {
        let response = self
            .client
            .request(prepared.method, prepared.url)
            .headers(prepared.headers)
            .body(prepared.body)
            .send()
            .await
            .map_err(TransportError::from)?;

        let status = response.status();
        let headers = response.headers().clone();
        let body = response.bytes().await.map_err(TransportError::from)?;

        Ok(Response {
            status,
            headers,
            body,
        })
    }
}
