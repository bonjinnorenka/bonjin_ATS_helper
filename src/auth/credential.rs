use base64::{Engine as _, engine::general_purpose::STANDARD};

use crate::{
    error::{AuthError, Result, ValidationError},
    request::prepared_request::PreparedRequest,
};

use super::{apply_sas_credential, apply_shared_key_credential};

#[derive(Debug, Clone)]
pub enum Credential {
    SharedKey(SharedKeyCredential),
    Sas(SasCredential),
}

impl From<SharedKeyCredential> for Credential {
    fn from(value: SharedKeyCredential) -> Self {
        Self::SharedKey(value)
    }
}

impl From<SasCredential> for Credential {
    fn from(value: SasCredential) -> Self {
        Self::Sas(value)
    }
}

impl Credential {
    pub(crate) fn apply(&self, prepared: &mut PreparedRequest) -> Result<()> {
        match self {
            Self::SharedKey(credential) => apply_shared_key_credential(credential, prepared),
            Self::Sas(credential) => {
                apply_sas_credential(credential, &mut prepared.url);
                Ok(())
            }
        }
    }

    pub(crate) fn account_name(&self) -> Option<&str> {
        match self {
            Self::SharedKey(credential) => Some(credential.account_name()),
            Self::Sas(_) => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SharedKeyCredential {
    account_name: String,
    account_key: Vec<u8>,
}

impl SharedKeyCredential {
    pub fn new(account_name: impl Into<String>, account_key: impl AsRef<str>) -> Result<Self> {
        let account_name = account_name.into();
        if account_name.trim().is_empty() {
            return Err(ValidationError::InvalidEndpoint(
                "account name cannot be empty".to_owned(),
            )
            .into());
        }

        let account_key = STANDARD
            .decode(account_key.as_ref())
            .map_err(|_| AuthError::InvalidAccountKey)?;

        Ok(Self {
            account_name,
            account_key,
        })
    }

    pub fn account_name(&self) -> &str {
        &self.account_name
    }

    pub(crate) fn account_key(&self) -> &[u8] {
        &self.account_key
    }
}

#[derive(Debug, Clone)]
pub struct SasCredential {
    raw_query: String,
}

impl SasCredential {
    pub fn new(raw_query: impl Into<String>) -> Result<Self> {
        let raw_query = raw_query.into();
        let raw_query = raw_query.trim().trim_start_matches('?').to_owned();
        if raw_query.is_empty() {
            return Err(ValidationError::InvalidSas("sas token cannot be empty".to_owned()).into());
        }

        if !raw_query.contains('=') {
            return Err(ValidationError::InvalidSas(
                "sas token must contain key=value pairs".to_owned(),
            )
            .into());
        }

        Ok(Self { raw_query })
    }

    pub(crate) fn raw_query(&self) -> &str {
        &self.raw_query
    }
}
