use http::HeaderMap;
use serde::{Deserialize, Serialize};

use super::builder::OriginalQuery;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContinuationToken {
    pub next_partition_key: Option<String>,
    pub next_row_key: Option<String>,
    pub original_query: OriginalQuery,
}

impl ContinuationToken {
    pub(crate) fn from_headers(headers: &HeaderMap, original_query: OriginalQuery) -> Option<Self> {
        let next_partition_key = headers
            .get("x-ms-continuation-NextPartitionKey")
            .and_then(|value| value.to_str().ok())
            .map(ToOwned::to_owned);
        let next_row_key = headers
            .get("x-ms-continuation-NextRowKey")
            .and_then(|value| value.to_str().ok())
            .map(ToOwned::to_owned);

        if next_partition_key.is_none() && next_row_key.is_none() {
            return None;
        }

        Some(Self {
            next_partition_key,
            next_row_key,
            original_query,
        })
    }
}
