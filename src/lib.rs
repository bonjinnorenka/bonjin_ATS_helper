#![allow(non_snake_case)] // Cargo package name is intentionally mixed-case

pub mod mock;

pub mod auth;
pub(crate) mod backend;
pub mod client;
pub mod entity;
pub mod error;
pub mod query;

pub(crate) mod codec;
pub(crate) mod http;
pub(crate) mod request;
pub(crate) mod validation;

pub use auth::{Credential, SasCredential, SharedKeyCredential};
pub use client::{
    ClientOptions, DEFAULT_CONNECT_TIMEOUT, DEFAULT_REQUEST_TIMEOUT, DEFAULT_STORAGE_API_VERSION,
    IfMatch, MetadataLevel, TableClient, TableServiceClient,
};
pub use entity::{DynamicEntity, EntityProperty, EntitySystemProperties, TableEntity};
pub use error::{
    AuthError, Error, Result, SerializationError, ServiceError, ServiceErrorKind, TransportError,
    UnexpectedResponseError, ValidationError,
};
pub use mock::{DurabilityMode, FlushPolicy, MockOptions};
pub use query::{ContinuationToken, OriginalQuery, Query, QueryBuilder, QueryPage};
