use base64::{Engine as _, engine::general_purpose::STANDARD};
use hmac::{Hmac, Mac};
use http::HeaderValue;
use sha2::Sha256;

use crate::{
    auth::credential::SharedKeyCredential,
    error::{AuthError, Result},
    request::{headers::AUTHORIZATION, prepared_request::PreparedRequest},
};

type HmacSha256 = Hmac<Sha256>;

pub(crate) fn apply_shared_key_credential(
    credential: &SharedKeyCredential,
    prepared: &mut PreparedRequest,
) -> Result<()> {
    if prepared.signing_date.is_empty() {
        return Err(AuthError::MissingSigningMetadata("signing_date").into());
    }
    if prepared.canonicalized_resource.is_empty() {
        return Err(AuthError::MissingSigningMetadata("canonicalized_resource").into());
    }

    let string_to_sign = string_to_sign(prepared);
    let mut hmac = HmacSha256::new_from_slice(credential.account_key())
        .map_err(|_| AuthError::InvalidAccountKey)?;
    hmac.update(string_to_sign.as_bytes());
    let signature = STANDARD.encode(hmac.finalize().into_bytes());
    let value = format!("SharedKey {}:{}", credential.account_name(), signature);

    prepared.headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&value)
            .map_err(|_| AuthError::MissingSigningMetadata("authorization"))?,
    );

    Ok(())
}

pub(crate) fn string_to_sign(prepared: &PreparedRequest) -> String {
    format!(
        "{}\n{}\n{}\n{}\n{}",
        prepared.method.as_str(),
        prepared.content_md5.as_deref().unwrap_or(""),
        prepared.content_type.as_deref().unwrap_or(""),
        prepared.signing_date,
        prepared.canonicalized_resource
    )
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;
    use http::{HeaderMap, Method};
    use url::Url;

    use crate::request::prepared_request::PreparedRequest;

    use super::string_to_sign;

    #[test]
    fn table_service_signature_does_not_include_canonicalized_headers() {
        let prepared = PreparedRequest {
            method: Method::GET,
            url: Url::parse("https://example.table.core.windows.net/Tables").unwrap(),
            headers: HeaderMap::new(),
            body: Bytes::new(),
            content_md5: None,
            content_type: None,
            signing_date: "Thu, 18 Mar 2026 03:04:05 GMT".to_owned(),
            canonicalized_resource: "/account/Tables".to_owned(),
        };

        assert_eq!(
            string_to_sign(&prepared),
            "GET\n\n\nThu, 18 Mar 2026 03:04:05 GMT\n/account/Tables"
        );
    }
}
