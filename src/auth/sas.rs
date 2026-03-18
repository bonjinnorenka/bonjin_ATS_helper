use url::{Url, form_urlencoded};

use crate::auth::credential::SasCredential;

pub(crate) fn apply_sas_credential(credential: &SasCredential, url: &mut Url) {
    let pairs = form_urlencoded::parse(credential.raw_query().as_bytes())
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect::<Vec<_>>();

    let mut query_pairs = url.query_pairs_mut();
    for (key, value) in pairs {
        query_pairs.append_pair(&key, &value);
    }
}

#[cfg(test)]
mod tests {
    use url::Url;

    use crate::auth::credential::SasCredential;

    use super::apply_sas_credential;

    #[test]
    fn appends_sas_parameters_to_existing_query() {
        let credential = SasCredential::new("sv=2025-01-01&sig=abc%2B123").unwrap();
        let mut url = Url::parse("https://example.table.core.windows.net/Tables?$top=10").unwrap();

        apply_sas_credential(&credential, &mut url);

        let query = url.query().unwrap();
        assert!(query.contains("$top=10"));
        assert!(query.contains("sv=2025-01-01"));
        assert!(query.contains("sig=abc%2B123"));
    }
}
