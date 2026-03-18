use std::fmt;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use zeroize::Zeroizing;

use crate::{
    error::{AuthError, Result, ValidationError},
    request::prepared_request::PreparedRequest,
};

use super::{apply_sas_credential, apply_shared_key_credential};

#[derive(Clone)]
pub enum Credential {
    SharedKey(SharedKeyCredential),
    Sas(SasCredential),
}

impl fmt::Debug for Credential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SharedKey(credential) => f.debug_tuple("SharedKey").field(credential).finish(),
            Self::Sas(credential) => f.debug_tuple("Sas").field(credential).finish(),
        }
    }
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

#[derive(Clone)]
pub struct SharedKeyCredential {
    account_name: String,
    account_key: Zeroizing<Vec<u8>>,
}

impl fmt::Debug for SharedKeyCredential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SharedKeyCredential")
            .field("account_name", &self.account_name)
            .field("account_key", &"[REDACTED]")
            .finish()
    }
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
            account_key: Zeroizing::new(account_key),
        })
    }

    pub fn account_name(&self) -> &str {
        &self.account_name
    }

    pub(crate) fn account_key(&self) -> &[u8] {
        self.account_key.as_slice()
    }
}

#[derive(Clone)]
pub struct SasCredential {
    raw_query: Zeroizing<String>,
}

impl fmt::Debug for SasCredential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SasCredential")
            .field("raw_query", &"[REDACTED]")
            .finish()
    }
}

impl SasCredential {
    pub fn new(raw_query: impl Into<String>) -> Result<Self> {
        let raw_query = Zeroizing::new(raw_query.into());
        let trimmed = raw_query.trim().trim_start_matches('?');
        if trimmed.is_empty() {
            return Err(ValidationError::InvalidSas("sas token cannot be empty".to_owned()).into());
        }

        if !trimmed.contains('=') {
            return Err(ValidationError::InvalidSas(
                "sas token must contain key=value pairs".to_owned(),
            )
            .into());
        }

        Ok(Self {
            raw_query: Zeroizing::new(trimmed.to_owned()),
        })
    }

    pub(crate) fn raw_query(&self) -> &str {
        self.raw_query.as_str()
    }
}

#[cfg(test)]
mod tests {
    use super::{Credential, SasCredential, SharedKeyCredential};

    #[test]
    fn shared_key_debug_redacts_account_key() {
        let credential = SharedKeyCredential::new("account", "AQIDBA==").unwrap();
        let debug = format!("{credential:?}");

        assert!(debug.contains("account"));
        assert!(debug.contains("[REDACTED]"));
        assert!(!debug.contains("AQIDBA=="));
        assert!(!debug.contains("1, 2, 3, 4"));
    }

    #[test]
    fn sas_debug_redacts_raw_query_from_wrapped_credential() {
        let credential = Credential::from(SasCredential::new("sv=1&sig=secret").unwrap());
        let debug = format!("{credential:?}");

        assert!(debug.contains("[REDACTED]"));
        assert!(!debug.contains("sig=secret"));
    }
}
