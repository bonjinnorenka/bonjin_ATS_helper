use url::Url;

pub(crate) fn canonicalized_resource(account_name: &str, url: &Url) -> String {
    format!("/{account_name}{}", url.path())
}
