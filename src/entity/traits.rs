use serde::{Serialize, de::DeserializeOwned};

pub trait TableEntity: Serialize + DeserializeOwned {
    fn partition_key(&self) -> &str;
    fn row_key(&self) -> &str;
    fn etag(&self) -> Option<&str> {
        None
    }
}
