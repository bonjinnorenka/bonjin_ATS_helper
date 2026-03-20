#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IfMatch {
    Any,
    Etag(String),
}
