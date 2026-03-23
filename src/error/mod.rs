use http::{HeaderMap, StatusCode};
use thiserror::Error;

use crate::http::response::Response;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error(transparent)]
    Auth(#[from] AuthError),
    #[error(transparent)]
    Transport(#[from] TransportError),
    #[error(transparent)]
    Serialization(#[from] SerializationError),
    #[error(transparent)]
    Service(#[from] ServiceError),
    #[error(transparent)]
    Validation(#[from] ValidationError),
    #[error(transparent)]
    UnexpectedResponse(#[from] UnexpectedResponseError),
}

#[derive(Debug, Error)]
pub enum AuthError {
    #[error("invalid shared key account key")]
    InvalidAccountKey,
    #[error("missing signing metadata: {0}")]
    MissingSigningMetadata(&'static str),
}

#[derive(Debug, Error)]
pub enum TransportError {
    #[error("{message}")]
    RequestFailed { message: String },
}

impl From<reqwest::Error> for TransportError {
    fn from(mut value: reqwest::Error) -> Self {
        sanitize_reqwest_error_url(&mut value);
        Self::RequestFailed {
            message: value.to_string(),
        }
    }
}

#[derive(Debug, Error)]
pub enum SerializationError {
    #[error("json serialization failed: {0}")]
    Json(String),
    #[error("unsupported entity shape: {0}")]
    UnsupportedShape(String),
    #[error("date/time conversion failed: {0}")]
    DateTime(String),
}

impl From<serde_json::Error> for SerializationError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value.to_string())
    }
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum ValidationError {
    #[error("invalid endpoint: {0}")]
    InvalidEndpoint(String),
    #[error("invalid table name: {0}")]
    InvalidTableName(String),
    #[error("invalid partition or row key: {0}")]
    InvalidKey(String),
    #[error("invalid query: {0}")]
    InvalidQuery(String),
    #[error("invalid sas token: {0}")]
    InvalidSas(String),
    #[error("invalid client option: {0}")]
    InvalidClientOption(String),
    #[error("entity validation failed: {0}")]
    EntityLimit(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceErrorKind {
    BadRequest,
    Forbidden,
    NotFound,
    TableNotFound,
    EntityNotFound,
    Conflict,
    TableAlreadyExists,
    PreconditionFailed,
    Unauthorized,
    Unknown,
}

#[derive(Debug, Error)]
#[error(
    "service request failed with status {status}{code_suffix}{message_suffix}",
    code_suffix = format_code_suffix(code.as_deref()),
    message_suffix = format_message_suffix(message.as_deref())
)]
pub struct ServiceError {
    pub status: StatusCode,
    pub code: Option<String>,
    pub request_id: Option<String>,
    pub message: Option<String>,
    pub body_snippet: Option<String>,
    pub kind: ServiceErrorKind,
}

#[derive(Debug, Error)]
#[error("unexpected response status {status:?}: {message}")]
pub struct UnexpectedResponseError {
    pub status: Option<StatusCode>,
    pub message: String,
    pub body_snippet: Option<String>,
}

pub(crate) fn ensure_status(
    response: Response,
    expected: &[StatusCode],
    operation: &'static str,
) -> Result<Response> {
    if expected.contains(&response.status) {
        return Ok(response);
    }

    if response.status.is_client_error() || response.status.is_server_error() {
        return Err(ServiceError::from_response(response).into());
    }

    Err(UnexpectedResponseError {
        status: Some(response.status),
        message: format!("unexpected status for {operation}"),
        body_snippet: Some(truncate_body(&response.body)),
    }
    .into())
}

pub(crate) fn service_error(
    status: StatusCode,
    code: Option<&str>,
    message: Option<&str>,
) -> Error {
    let code = code.map(ToOwned::to_owned);
    let message = message.map(ToOwned::to_owned);
    ServiceError {
        status,
        kind: classify_service_error(status, code.as_deref()),
        code,
        request_id: None,
        message,
        body_snippet: None,
    }
    .into()
}

impl ServiceError {
    pub(crate) fn from_response(response: Response) -> Self {
        let request_id = header_string(&response.headers, "x-ms-request-id");
        let body_snippet = Some(truncate_body(&response.body));
        let (code, message) = parse_service_error_body(&response.body);
        let kind = classify_service_error(response.status, code.as_deref());

        Self {
            status: response.status,
            code,
            request_id,
            message,
            body_snippet,
            kind,
        }
    }
}

fn parse_service_error_body(body: &[u8]) -> (Option<String>, Option<String>) {
    let value = match serde_json::from_slice::<serde_json::Value>(body) {
        Ok(value) => value,
        Err(_) => return (None, None),
    };

    if let Some(odata_error) = value.get("odata.error") {
        let code = odata_error
            .get("code")
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned);
        let message = odata_error
            .get("message")
            .and_then(|message| message.get("value"))
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned);
        return (code, message);
    }

    let code = value
        .get("error")
        .and_then(|error| error.get("code"))
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned);
    let message = value
        .get("error")
        .and_then(|error| error.get("message"))
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned);

    (code, message)
}

fn classify_service_error(status: StatusCode, code: Option<&str>) -> ServiceErrorKind {
    match (status, code) {
        (StatusCode::BAD_REQUEST, _) => ServiceErrorKind::BadRequest,
        (StatusCode::UNAUTHORIZED, _) => ServiceErrorKind::Unauthorized,
        (StatusCode::FORBIDDEN, _) => ServiceErrorKind::Forbidden,
        (StatusCode::NOT_FOUND, Some("ResourceNotFound" | "EntityNotFound")) => {
            ServiceErrorKind::EntityNotFound
        }
        (StatusCode::NOT_FOUND, Some("TableNotFound")) => ServiceErrorKind::TableNotFound,
        (StatusCode::NOT_FOUND, _) => ServiceErrorKind::NotFound,
        (StatusCode::CONFLICT, Some("TableAlreadyExists")) => ServiceErrorKind::TableAlreadyExists,
        (StatusCode::CONFLICT, _) => ServiceErrorKind::Conflict,
        (StatusCode::PRECONDITION_FAILED, _) => ServiceErrorKind::PreconditionFailed,
        _ => ServiceErrorKind::Unknown,
    }
}

fn header_string(headers: &HeaderMap, name: &'static str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(ToOwned::to_owned)
}

fn truncate_body(body: &[u8]) -> String {
    let text = String::from_utf8_lossy(body);
    let mut snippet = text.chars().take(512).collect::<String>();
    if text.chars().count() > 512 {
        snippet.push_str("...");
    }
    snippet
}

fn format_code_suffix(code: Option<&str>) -> String {
    code.map(|code| format!(" ({code})")).unwrap_or_default()
}

fn format_message_suffix(message: Option<&str>) -> String {
    message
        .map(|message| format!(": {message}"))
        .unwrap_or_default()
}

fn sanitize_reqwest_error_url(error: &mut reqwest::Error) {
    if let Some(url) = error.url_mut() {
        url.set_query(None);
        url.set_fragment(None);
        let _ = url.set_password(None);
        let _ = url.set_username("");
    }
}

#[cfg(test)]
mod tests {
    use super::TransportError;

    #[tokio::test]
    async fn reqwest_errors_redact_query_parameters() {
        let error = reqwest::Client::new()
            .get("blob://example.table.core.windows.net/Tables?sv=1&sig=secret")
            .send()
            .await
            .unwrap_err();

        let message = TransportError::from(error).to_string();

        assert!(message.contains("blob://example.table.core.windows.net/Tables"));
        assert!(!message.contains("sig=secret"));
        assert!(!message.contains("sv=1"));
    }
}
