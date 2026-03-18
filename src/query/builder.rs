use serde::{Deserialize, Serialize};
use url::Url;

use crate::error::ValidationError;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OriginalQuery {
    pub filter: Option<String>,
    pub top: Option<u16>,
    pub select: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Query {
    pub(crate) original: OriginalQuery,
}

#[derive(Debug, Clone, Default)]
pub struct QueryBuilder {
    query: Query,
}

impl Query {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn builder() -> QueryBuilder {
        QueryBuilder::new()
    }

    pub fn with_filter(mut self, filter: impl Into<String>) -> Result<Self, ValidationError> {
        let filter = validate_filter(filter.into())?;
        self.original.filter = Some(filter);
        Ok(self)
    }

    pub fn with_top(mut self, top: u16) -> Result<Self, ValidationError> {
        validate_top(top)?;
        self.original.top = Some(top);
        Ok(self)
    }

    pub fn with_select<I, S>(mut self, select: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.original.select = select.into_iter().map(Into::into).collect();
        self
    }

    pub fn original_query(&self) -> &OriginalQuery {
        &self.original
    }

    pub(crate) fn append_to_url(&self, url: &mut Url) {
        if let Some(filter) = &self.original.filter {
            url.query_pairs_mut().append_pair("$filter", filter);
        }
        if let Some(top) = self.original.top {
            url.query_pairs_mut().append_pair("$top", &top.to_string());
        }
        if !self.original.select.is_empty() {
            url.query_pairs_mut()
                .append_pair("$select", &self.original.select.join(","));
        }
    }
}

impl QueryBuilder {
    pub fn new() -> Self {
        Self {
            query: Query::new(),
        }
    }

    pub fn filter(mut self, filter: impl Into<String>) -> Result<Self, ValidationError> {
        self.query = self.query.with_filter(filter)?;
        Ok(self)
    }

    pub fn top(mut self, top: u16) -> Result<Self, ValidationError> {
        self.query = self.query.with_top(top)?;
        Ok(self)
    }

    pub fn select<I, S>(mut self, select: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.query = self.query.with_select(select);
        self
    }

    pub fn build(self) -> Query {
        self.query
    }
}

pub(crate) fn validate_top(top: u16) -> Result<(), ValidationError> {
    if top == 0 {
        return Err(ValidationError::InvalidQuery(
            "$top must be greater than zero".to_owned(),
        ));
    }
    if top > 1000 {
        return Err(ValidationError::InvalidQuery(
            "$top cannot exceed 1000".to_owned(),
        ));
    }
    Ok(())
}

pub(crate) fn validate_filter(filter: String) -> Result<String, ValidationError> {
    if filter.trim().is_empty() {
        return Err(ValidationError::InvalidQuery(
            "$filter cannot be empty".to_owned(),
        ));
    }

    let comparisons = filter.to_ascii_lowercase().match_indices(" eq ").count()
        + filter.to_ascii_lowercase().match_indices(" ne ").count()
        + filter.to_ascii_lowercase().match_indices(" gt ").count()
        + filter.to_ascii_lowercase().match_indices(" ge ").count()
        + filter.to_ascii_lowercase().match_indices(" lt ").count()
        + filter.to_ascii_lowercase().match_indices(" le ").count();

    if comparisons > 15 {
        return Err(ValidationError::InvalidQuery(
            "$filter cannot contain more than 15 discrete comparisons".to_owned(),
        ));
    }

    Ok(filter)
}

#[cfg(test)]
mod tests {
    use super::Query;

    #[test]
    fn validates_filter_comparison_count() {
        let filter = (0..16)
            .map(|index| format!("RowKey eq '{index}'"))
            .collect::<Vec<_>>()
            .join(" or ");

        let error = Query::new().with_filter(filter).unwrap_err();
        assert_eq!(
            error.to_string(),
            "invalid query: $filter cannot contain more than 15 discrete comparisons"
        );
    }
}
