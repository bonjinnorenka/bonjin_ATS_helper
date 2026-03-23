use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::Write,
    ops::Bound,
    path::{Path, PathBuf},
    sync::Arc,
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use http::{HeaderMap, HeaderValue, StatusCode};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::{
    backend::{Backend, BackendFuture, MatchCondition, UpdateMode},
    codec::serialize::{dynamic_entity_to_body, dynamic_entity_to_value},
    entity::{DynamicEntity, EntityProperty},
    error::{Result, UnexpectedResponseError, service_error},
    mock::{DurabilityMode, FlushPolicy, MockOptions},
    query::{
        ContinuationToken, EntityView, OriginalQuery, QueryPage, apply_select, evaluate_filter,
        parse_filter,
    },
    validation::{
        key::{validate_partition_key, validate_row_key},
        limits::validate_entity_limit_constraints,
        property::validate_property_name,
        table_name::validate_table_name,
    },
};

const STORE_FORMAT_VERSION: u32 = 1;
const QUERY_SCAN_BUDGET_MULTIPLIER: usize = 16;

#[derive(Clone)]
pub(crate) struct MockBackend {
    inner: Arc<MockBackendInner>,
}

struct MockBackendInner {
    root_path: PathBuf,
    strict_mode: bool,
    flush_policy: FlushPolicy,
    durability_mode: DurabilityMode,
    state: Mutex<StoreState>,
}

#[derive(Debug, Clone)]
struct StoreState {
    created_at: OffsetDateTime,
    next_revision: u64,
    tables: BTreeMap<String, TableState>,
    dirty: bool,
}

#[derive(Debug, Clone)]
struct TableState {
    table_name: String,
    table_revision: u64,
    entities: BTreeMap<(String, String), StoredEntity>,
}

#[derive(Debug, Clone)]
struct StoredEntity {
    partition_key: String,
    row_key: String,
    timestamp: OffsetDateTime,
    etag: String,
    properties: BTreeMap<String, EntityProperty>,
}

impl EntityView for StoredEntity {
    fn partition_key(&self) -> &str {
        &self.partition_key
    }

    fn row_key(&self) -> &str {
        &self.row_key
    }

    fn timestamp(&self) -> Option<OffsetDateTime> {
        Some(self.timestamp)
    }

    fn property(&self, name: &str) -> Option<&EntityProperty> {
        self.properties.get(name)
    }
}

impl MockBackend {
    pub(crate) fn new(options: MockOptions) -> Result<Self> {
        let state = load_store(&options)?;
        Ok(Self {
            inner: Arc::new(MockBackendInner {
                root_path: options.root_path,
                strict_mode: options.strict_mode,
                flush_policy: options.flush_policy,
                durability_mode: options.durability_mode,
                state: Mutex::new(state),
            }),
        })
    }

    async fn mutate<T>(
        &self,
        operation: impl FnOnce(&mut StoreState, bool) -> Result<T>,
    ) -> Result<T> {
        let mut state = self.inner.state.lock().await;
        let mut candidate = state.clone();
        let result = operation(&mut candidate, self.inner.strict_mode)?;

        if self.inner.flush_policy == FlushPolicy::WriteThrough {
            persist_store(
                &self.inner.root_path,
                self.inner.strict_mode,
                self.inner.durability_mode,
                &candidate,
            )?;
            candidate.dirty = false;
        } else {
            candidate.dirty = true;
        }

        *state = candidate;
        Ok(result)
    }

    fn request_id() -> String {
        Uuid::new_v4().to_string()
    }
}

impl Backend for MockBackend {
    fn create_table(&self, table_name: &str) -> BackendFuture<'_, ()> {
        let table_name = table_name.to_owned();
        Box::pin(async move {
            self.mutate(move |state, _strict_mode| {
                validate_table_name(&table_name)?;
                let normalized = normalize_table_name(&table_name);
                if state.tables.contains_key(&normalized) {
                    return Err(service_error(
                        StatusCode::CONFLICT,
                        Some("TableAlreadyExists"),
                        Some("table already exists"),
                    ));
                }

                let revision = next_revision(state);
                state.tables.insert(
                    normalized,
                    TableState {
                        table_name,
                        table_revision: revision,
                        entities: BTreeMap::new(),
                    },
                );
                Ok(())
            })
            .await
        })
    }

    fn delete_table(&self, table_name: &str) -> BackendFuture<'_, ()> {
        let table_name = table_name.to_owned();
        Box::pin(async move {
            self.mutate(move |state, _strict_mode| {
                let normalized = normalize_table_name(&table_name);
                if state.tables.remove(&normalized).is_none() {
                    return Err(service_error(
                        StatusCode::NOT_FOUND,
                        Some("TableNotFound"),
                        Some("table not found"),
                    ));
                }
                Ok(())
            })
            .await
        })
    }

    fn list_tables(&self) -> BackendFuture<'_, Vec<String>> {
        Box::pin(async move {
            let state = self.inner.state.lock().await;
            Ok(state
                .tables
                .values()
                .map(|table| table.table_name.clone())
                .collect())
        })
    }

    fn table_exists(&self, table_name: &str) -> BackendFuture<'_, bool> {
        let table_name = table_name.to_owned();
        Box::pin(async move {
            let state = self.inner.state.lock().await;
            Ok(state
                .tables
                .contains_key(&normalize_table_name(&table_name)))
        })
    }

    fn insert_entity(&self, table_name: &str, entity: DynamicEntity) -> BackendFuture<'_, ()> {
        let table_name = table_name.to_owned();
        Box::pin(async move {
            self.mutate(move |state, strict_mode| {
                validate_dynamic_entity(&entity, strict_mode)?;
                let entity_revision = next_revision(state);
                let table_revision = next_revision(state);
                let table = table_mut(state, &table_name)?;
                let key = (entity.partition_key.clone(), entity.row_key.clone());
                if table.entities.contains_key(&key) {
                    return Err(service_error(
                        StatusCode::CONFLICT,
                        Some("EntityAlreadyExists"),
                        Some("entity already exists"),
                    ));
                }

                let stored = stored_entity_from_dynamic(entity, entity_revision, None, false);
                table.entities.insert(key, stored);
                table.table_revision = table_revision;
                Ok(())
            })
            .await
        })
    }

    fn get_entity(
        &self,
        table_name: &str,
        partition_key: &str,
        row_key: &str,
    ) -> BackendFuture<'_, DynamicEntity> {
        let table_name = table_name.to_owned();
        let partition_key = partition_key.to_owned();
        let row_key = row_key.to_owned();
        Box::pin(async move {
            let state = self.inner.state.lock().await;
            let table = table_ref(&state, &table_name)?;
            let entity = table
                .entities
                .get(&(partition_key, row_key))
                .ok_or_else(|| {
                    service_error(
                        StatusCode::NOT_FOUND,
                        Some("EntityNotFound"),
                        Some("entity not found"),
                    )
                })?;
            Ok(dynamic_from_stored(entity))
        })
    }

    fn update_entity(
        &self,
        table_name: &str,
        entity: DynamicEntity,
        if_match: MatchCondition,
        mode: UpdateMode,
    ) -> BackendFuture<'_, ()> {
        let table_name = table_name.to_owned();
        Box::pin(async move {
            self.mutate(move |state, strict_mode| {
                validate_dynamic_entity(&entity, strict_mode)?;
                let key = (entity.partition_key.clone(), entity.row_key.clone());
                let current = table_ref(state, &table_name)?
                    .entities
                    .get(&key)
                    .cloned()
                    .ok_or_else(|| {
                        service_error(
                            StatusCode::NOT_FOUND,
                            Some("EntityNotFound"),
                            Some("entity not found"),
                        )
                    })?;
                ensure_match(&current, if_match)?;
                let entity_revision = next_revision(state);
                let table_revision = next_revision(state);
                let updated = match mode {
                    UpdateMode::Replace => {
                        stored_entity_from_dynamic(entity, entity_revision, None, false)
                    }
                    UpdateMode::Merge => {
                        stored_entity_from_dynamic(entity, entity_revision, Some(&current), true)
                    }
                };
                let table = table_mut(state, &table_name)?;
                table.entities.insert(key, updated);
                table.table_revision = table_revision;
                Ok(())
            })
            .await
        })
    }

    fn upsert_entity(
        &self,
        table_name: &str,
        entity: DynamicEntity,
        mode: UpdateMode,
    ) -> BackendFuture<'_, ()> {
        let table_name = table_name.to_owned();
        Box::pin(async move {
            self.mutate(move |state, strict_mode| {
                validate_dynamic_entity(&entity, strict_mode)?;
                let key = (entity.partition_key.clone(), entity.row_key.clone());
                let current = table_ref(state, &table_name)?.entities.get(&key).cloned();
                let entity_revision = next_revision(state);
                let table_revision = next_revision(state);
                let merged = match (mode, current.as_ref()) {
                    (UpdateMode::Replace, _) => {
                        stored_entity_from_dynamic(entity, entity_revision, None, false)
                    }
                    (UpdateMode::Merge, Some(current)) => {
                        stored_entity_from_dynamic(entity, entity_revision, Some(current), true)
                    }
                    (UpdateMode::Merge, None) => {
                        stored_entity_from_dynamic(entity, entity_revision, None, false)
                    }
                };
                let table = table_mut(state, &table_name)?;
                table.entities.insert(key, merged);
                table.table_revision = table_revision;
                Ok(())
            })
            .await
        })
    }

    fn delete_entity(
        &self,
        table_name: &str,
        partition_key: &str,
        row_key: &str,
        if_match: MatchCondition,
    ) -> BackendFuture<'_, ()> {
        let table_name = table_name.to_owned();
        let partition_key = partition_key.to_owned();
        let row_key = row_key.to_owned();
        Box::pin(async move {
            self.mutate(move |state, _strict_mode| {
                let key = (partition_key.clone(), row_key.clone());
                let current = table_ref(state, &table_name)?
                    .entities
                    .get(&key)
                    .cloned()
                    .ok_or_else(|| {
                        service_error(
                            StatusCode::NOT_FOUND,
                            Some("EntityNotFound"),
                            Some("entity not found"),
                        )
                    })?;
                ensure_match(&current, if_match)?;
                let table_revision = next_revision(state);
                let table = table_mut(state, &table_name)?;
                table.entities.remove(&key);
                table.table_revision = table_revision;
                Ok(())
            })
            .await
        })
    }

    fn query_entities(
        &self,
        table_name: &str,
        query: OriginalQuery,
        continuation: Option<ContinuationToken>,
    ) -> BackendFuture<'_, QueryPage<DynamicEntity>> {
        let table_name = table_name.to_owned();
        Box::pin(async move {
            let filter = query.filter.as_deref().map(parse_filter).transpose()?;
            let page_size = usize::from(query.top.unwrap_or(1000));
            let scan_limit = page_size
                .max(1)
                .saturating_mul(QUERY_SCAN_BUDGET_MULTIPLIER);
            let state = self.inner.state.lock().await;
            let table = table_ref(&state, &table_name)?;

            let start = continuation
                .as_ref()
                .and_then(cursor_from_continuation)
                .map(Bound::Excluded)
                .unwrap_or(Bound::Unbounded);
            let mut iter = table
                .entities
                .range::<(String, String), _>((start, Bound::Unbounded))
                .peekable();
            let mut scanned = 0usize;
            let mut items = Vec::new();
            let mut last_scanned = None;

            for ((partition_key, row_key), entity) in iter.by_ref() {
                last_scanned = Some((partition_key.clone(), row_key.clone()));
                scanned += 1;
                let matches = match &filter {
                    Some(filter) => evaluate_filter(filter, entity)?,
                    None => true,
                };
                if matches {
                    let dynamic = dynamic_from_stored(entity);
                    items.push(apply_select(&dynamic, &query.select));
                    if items.len() >= page_size {
                        break;
                    }
                }

                if scanned >= scan_limit {
                    break;
                }
            }

            let continuation = if iter.peek().is_some() {
                last_scanned.map(|(next_partition_key, next_row_key)| ContinuationToken {
                    next_partition_key: Some(next_partition_key),
                    next_row_key: Some(next_row_key),
                    original_query: query.clone(),
                })
            } else {
                None
            };

            let request_id = Self::request_id();
            let mut headers = HeaderMap::new();
            headers.insert(
                "x-ms-request-id",
                HeaderValue::from_str(&request_id).expect("uuid is a valid header"),
            );
            if let Some(continuation) = &continuation {
                if let Some(next_partition_key) = &continuation.next_partition_key {
                    headers.insert(
                        "x-ms-continuation-NextPartitionKey",
                        HeaderValue::from_str(next_partition_key)
                            .expect("partition key is already validated"),
                    );
                }
                if let Some(next_row_key) = &continuation.next_row_key {
                    headers.insert(
                        "x-ms-continuation-NextRowKey",
                        HeaderValue::from_str(next_row_key).expect("row key is already validated"),
                    );
                }
            }

            Ok(QueryPage {
                items,
                continuation,
                request_id: Some(request_id),
                raw_headers: headers,
            })
        })
    }

    fn flush(&self) -> BackendFuture<'_, ()> {
        Box::pin(async move {
            let mut state = self.inner.state.lock().await;
            if !state.dirty {
                return Ok(());
            }
            persist_store(
                &self.inner.root_path,
                self.inner.strict_mode,
                self.inner.durability_mode,
                &state,
            )?;
            state.dirty = false;
            Ok(())
        })
    }
}

fn normalize_table_name(table_name: &str) -> String {
    table_name.to_ascii_lowercase()
}

fn table_ref<'a>(state: &'a StoreState, table_name: &str) -> Result<&'a TableState> {
    state
        .tables
        .get(&normalize_table_name(table_name))
        .ok_or_else(|| {
            service_error(
                StatusCode::NOT_FOUND,
                Some("TableNotFound"),
                Some("table not found"),
            )
        })
}

fn table_mut<'a>(state: &'a mut StoreState, table_name: &str) -> Result<&'a mut TableState> {
    state
        .tables
        .get_mut(&normalize_table_name(table_name))
        .ok_or_else(|| {
            service_error(
                StatusCode::NOT_FOUND,
                Some("TableNotFound"),
                Some("table not found"),
            )
        })
}

fn ensure_match(entity: &StoredEntity, if_match: MatchCondition) -> Result<()> {
    match if_match {
        MatchCondition::Any => Ok(()),
        MatchCondition::Etag(value) if value == entity.etag => Ok(()),
        MatchCondition::Etag(_) => Err(service_error(
            StatusCode::PRECONDITION_FAILED,
            Some("UpdateConditionNotSatisfied"),
            Some("etag condition not satisfied"),
        )),
    }
}

fn next_revision(state: &mut StoreState) -> u64 {
    let revision = state.next_revision;
    state.next_revision += 1;
    revision
}

fn stored_entity_from_dynamic(
    entity: DynamicEntity,
    entity_revision: u64,
    merge_with: Option<&StoredEntity>,
    merge: bool,
) -> StoredEntity {
    let timestamp = OffsetDateTime::now_utc();
    let etag = format!("W/\"mock-{entity_revision}\"");
    let properties = match (merge_with, merge) {
        (Some(current), true) => {
            let mut properties = current.properties.clone();
            for (name, value) in entity.properties {
                properties.insert(name, value);
            }
            properties
        }
        _ => entity.properties.into_iter().collect(),
    };

    StoredEntity {
        partition_key: entity.partition_key,
        row_key: entity.row_key,
        timestamp,
        etag,
        properties,
    }
}

fn dynamic_from_stored(entity: &StoredEntity) -> DynamicEntity {
    DynamicEntity {
        partition_key: entity.partition_key.clone(),
        row_key: entity.row_key.clone(),
        properties: entity
            .properties
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect::<IndexMap<_, _>>(),
        etag: Some(entity.etag.clone()),
        timestamp: Some(entity.timestamp),
    }
}

fn validate_dynamic_entity(entity: &DynamicEntity, strict_mode: bool) -> Result<()> {
    validate_partition_key(&entity.partition_key)?;
    validate_row_key(&entity.row_key)?;
    for name in entity.properties.keys() {
        validate_property_name(name)?;
    }

    if strict_mode {
        let body = dynamic_entity_to_body(entity)?;
        validate_entity_limit_constraints(entity, body.len())?;
    } else {
        let estimated_payload_size = serde_json::to_vec(&dynamic_entity_to_value(entity)?)
            .map_err(crate::error::SerializationError::from)?
            .len();
        validate_entity_limit_constraints(entity, estimated_payload_size)?;
    }

    Ok(())
}

fn cursor_from_continuation(token: &ContinuationToken) -> Option<(String, String)> {
    let partition_key = token.next_partition_key.clone()?;
    let row_key = token.next_row_key.clone().unwrap_or_default();
    Some((partition_key, row_key))
}

fn load_store(options: &MockOptions) -> Result<StoreState> {
    if !options.root_path.exists() {
        if !options.auto_create_if_missing {
            return Err(persistence_error("mock store root does not exist"));
        }
        fs::create_dir_all(options.root_path.join("tables"))
            .map_err(|error| persistence_error(&format!("failed to create mock store: {error}")))?;
        let state = StoreState {
            created_at: OffsetDateTime::now_utc(),
            next_revision: 1,
            tables: BTreeMap::new(),
            dirty: false,
        };
        persist_store(
            &options.root_path,
            options.strict_mode,
            options.durability_mode,
            &state,
        )?;
        return Ok(state);
    }

    let manifest_path = options.root_path.join("manifest.json");
    if !manifest_path.exists() {
        if !options.auto_create_if_missing {
            return Err(persistence_error("manifest.json does not exist"));
        }
        fs::create_dir_all(options.root_path.join("tables")).map_err(|error| {
            persistence_error(&format!("failed to create table directory: {error}"))
        })?;
        let state = StoreState {
            created_at: OffsetDateTime::now_utc(),
            next_revision: 1,
            tables: BTreeMap::new(),
            dirty: false,
        };
        persist_store(
            &options.root_path,
            options.strict_mode,
            options.durability_mode,
            &state,
        )?;
        return Ok(state);
    }

    let manifest: PersistedManifest = read_json(&manifest_path)?;
    if manifest.store_format_version != STORE_FORMAT_VERSION {
        return Err(corrupted_store_error(
            "store_format_version is not supported",
        ));
    }
    if manifest.strict_mode != options.strict_mode {
        return Err(corrupted_store_error(
            "strict_mode does not match the existing manifest",
        ));
    }

    let mut tables = BTreeMap::new();
    let mut next_revision = 1u64;
    for table_name in manifest.tables {
        let table_path = table_path(&options.root_path, &table_name);
        let table: PersistedTable = read_json(&table_path)?;
        if table.table_name != table_name {
            return Err(corrupted_store_error(
                "table snapshot name does not match manifest",
            ));
        }
        let normalized = normalize_table_name(&table_name);
        if tables.contains_key(&normalized) {
            return Err(corrupted_store_error(
                "manifest contains duplicate table names ignoring case",
            ));
        }

        let mut entities = BTreeMap::new();
        for entity in table.entities {
            validate_partition_key(&entity.partition_key).map_err(|error| {
                corrupted_store_error(&format!(
                    "invalid PartitionKey for entity ({}, {}): {error}",
                    entity.partition_key, entity.row_key
                ))
            })?;
            validate_row_key(&entity.row_key).map_err(|error| {
                corrupted_store_error(&format!(
                    "invalid RowKey for entity ({}, {}): {error}",
                    entity.partition_key, entity.row_key
                ))
            })?;
            let properties = entity
                .properties
                .into_iter()
                .map(|(name, value)| {
                    validate_property_name(&name).map_err(|error| {
                        corrupted_store_error(&format!(
                            "invalid property name `{name}` for entity ({}, {}): {error}",
                            entity.partition_key, entity.row_key
                        ))
                    })?;
                    let property =
                        value
                            .into_entity_property(options.strict_mode)
                            .map_err(|message| {
                                corrupted_store_error(&format!(
                                    "invalid property `{name}` for entity ({}, {}): {message}",
                                    entity.partition_key, entity.row_key
                                ))
                            })?;
                    Ok((name, property))
                })
                .collect::<Result<BTreeMap<_, _>>>()?;
            let stored = StoredEntity {
                partition_key: entity.partition_key.clone(),
                row_key: entity.row_key.clone(),
                timestamp: entity.timestamp,
                etag: entity.etag.clone(),
                properties,
            };
            if let Some(revision) = parse_etag_revision(&stored.etag) {
                next_revision = next_revision.max(revision + 1);
            }
            entities.insert(
                (stored.partition_key.clone(), stored.row_key.clone()),
                stored,
            );
        }

        next_revision = next_revision.max(table.table_revision + 1);
        tables.insert(
            normalized,
            TableState {
                table_name,
                table_revision: table.table_revision,
                entities,
            },
        );
    }

    Ok(StoreState {
        created_at: manifest.created_at,
        next_revision,
        tables,
        dirty: false,
    })
}

fn persist_store(
    root_path: &Path,
    strict_mode: bool,
    durability_mode: DurabilityMode,
    state: &StoreState,
) -> Result<()> {
    fs::create_dir_all(root_path.join("tables")).map_err(|error| {
        persistence_error(&format!("failed to create store directory: {error}"))
    })?;

    let manifest = PersistedManifest {
        store_format_version: STORE_FORMAT_VERSION,
        created_at: state.created_at,
        tables: state
            .tables
            .values()
            .map(|table| table.table_name.clone())
            .collect(),
        library_semver: Some(env!("CARGO_PKG_VERSION").to_owned()),
        strict_mode,
    };

    for table in state.tables.values() {
        let snapshot = PersistedTable {
            table_name: table.table_name.clone(),
            table_revision: table.table_revision,
            entities: table
                .entities
                .values()
                .map(PersistedEntity::from_stored)
                .collect(),
        };
        write_json_atomically(
            &table_path(root_path, &table.table_name),
            &snapshot,
            durability_mode,
        )?;
    }

    write_json_atomically(&root_path.join("manifest.json"), &manifest, durability_mode)?;

    let existing_tables = fs::read_dir(root_path.join("tables"))
        .map_err(|error| persistence_error(&format!("failed to read tables directory: {error}")))?;
    for entry in existing_tables {
        let entry = entry.map_err(|error| {
            persistence_error(&format!("failed to inspect tables directory: {error}"))
        })?;
        let path = entry.path();
        let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let Some(table_name) = file_name.strip_suffix(".json") else {
            continue;
        };
        if !state.tables.contains_key(&normalize_table_name(table_name)) {
            fs::remove_file(&path).map_err(|error| {
                persistence_error(&format!("failed to remove stale table snapshot: {error}"))
            })?;
        }
    }

    Ok(())
}

fn read_json<T>(path: &Path) -> Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    let bytes = fs::read(path).map_err(|error| {
        persistence_error(&format!("failed to read {}: {error}", path.display()))
    })?;
    serde_json::from_slice(&bytes).map_err(|error| {
        corrupted_store_error(&format!("failed to parse {}: {error}", path.display()))
    })
}

fn write_json_atomically<T>(path: &Path, value: &T, durability_mode: DurabilityMode) -> Result<()>
where
    T: Serialize,
{
    let bytes = serde_json::to_vec_pretty(value).map_err(|error| {
        persistence_error(&format!("failed to serialize {}: {error}", path.display()))
    })?;
    let parent = path
        .parent()
        .ok_or_else(|| persistence_error("path has no parent directory"))?;
    fs::create_dir_all(parent).map_err(|error| {
        persistence_error(&format!(
            "failed to create parent directory {}: {error}",
            parent.display()
        ))
    })?;

    let tmp_path = path.with_extension("tmp");
    let mut file = File::create(&tmp_path).map_err(|error| {
        persistence_error(&format!(
            "failed to create temp file {}: {error}",
            tmp_path.display()
        ))
    })?;
    file.write_all(&bytes).map_err(|error| {
        persistence_error(&format!(
            "failed to write temp file {}: {error}",
            tmp_path.display()
        ))
    })?;
    file.flush().map_err(|error| {
        persistence_error(&format!(
            "failed to flush temp file {}: {error}",
            tmp_path.display()
        ))
    })?;
    if durability_mode == DurabilityMode::Durable {
        file.sync_all().map_err(|error| {
            persistence_error(&format!(
                "failed to fsync temp file {}: {error}",
                tmp_path.display()
            ))
        })?;
    }
    drop(file);

    fs::rename(&tmp_path, path).map_err(|error| {
        persistence_error(&format!(
            "failed to replace {} with temp file {}: {error}",
            path.display(),
            tmp_path.display()
        ))
    })?;

    if durability_mode == DurabilityMode::Durable {
        #[cfg(not(windows))]
        {
            let directory = fs::OpenOptions::new()
                .read(true)
                .open(parent)
                .map_err(|error| {
                    persistence_error(&format!(
                        "failed to open directory {}: {error}",
                        parent.display()
                    ))
                })?;
            directory.sync_all().map_err(|error| {
                persistence_error(&format!(
                    "failed to fsync directory {}: {error}",
                    parent.display()
                ))
            })?;
        }
    }

    Ok(())
}

fn table_path(root_path: &Path, table_name: &str) -> PathBuf {
    root_path.join("tables").join(format!("{table_name}.json"))
}

fn parse_etag_revision(etag: &str) -> Option<u64> {
    etag.strip_prefix("W/\"mock-")
        .and_then(|value| value.strip_suffix('"'))
        .and_then(|value| value.parse::<u64>().ok())
}

fn persistence_error(message: &str) -> crate::error::Error {
    UnexpectedResponseError {
        status: None,
        message: message.to_owned(),
        body_snippet: None,
    }
    .into()
}

fn corrupted_store_error(message: &str) -> crate::error::Error {
    UnexpectedResponseError {
        status: None,
        message: format!("corrupted mock store: {message}"),
        body_snippet: None,
    }
    .into()
}

#[derive(Debug, Serialize, Deserialize)]
struct PersistedManifest {
    store_format_version: u32,
    #[serde(with = "time::serde::rfc3339")]
    created_at: OffsetDateTime,
    tables: Vec<String>,
    library_semver: Option<String>,
    strict_mode: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct PersistedTable {
    table_name: String,
    table_revision: u64,
    entities: Vec<PersistedEntity>,
}

#[derive(Debug, Serialize, Deserialize)]
struct PersistedEntity {
    partition_key: String,
    row_key: String,
    #[serde(with = "time::serde::rfc3339")]
    timestamp: OffsetDateTime,
    etag: String,
    properties: BTreeMap<String, PersistedValue>,
}

impl PersistedEntity {
    fn from_stored(entity: &StoredEntity) -> Self {
        Self {
            partition_key: entity.partition_key.clone(),
            row_key: entity.row_key.clone(),
            timestamp: entity.timestamp,
            etag: entity.etag.clone(),
            properties: entity
                .properties
                .iter()
                .map(|(name, value)| (name.clone(), PersistedValue::from_entity_property(value)))
                .collect(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", content = "value")]
enum PersistedValue {
    String(String),
    Bool(bool),
    Int32(i32),
    Int64(String),
    Double(String),
    DateTime(String),
    Guid(String),
    Binary(String),
}

impl PersistedValue {
    fn from_entity_property(property: &EntityProperty) -> Self {
        match property {
            EntityProperty::String(value) => Self::String(value.clone()),
            EntityProperty::Bool(value) => Self::Bool(*value),
            EntityProperty::Int32(value) => Self::Int32(*value),
            EntityProperty::Int64(value) => Self::Int64(value.to_string()),
            EntityProperty::Double(value) => Self::Double(value.to_string()),
            EntityProperty::DateTime(value) => Self::DateTime(
                crate::codec::datetime::format_datetime(value).expect("datetime is serializable"),
            ),
            EntityProperty::Guid(value) => {
                Self::Guid(value.as_hyphenated().to_string().to_lowercase())
            }
            EntityProperty::Binary(value) => Self::Binary(STANDARD.encode(value)),
        }
    }

    fn into_entity_property(
        self,
        strict_mode: bool,
    ) -> std::result::Result<EntityProperty, String> {
        Ok(match self {
            Self::String(value) => EntityProperty::String(value),
            Self::Bool(value) => EntityProperty::Bool(value),
            Self::Int32(value) => EntityProperty::Int32(value),
            Self::Int64(value) => EntityProperty::Int64(parse_persisted_int64(&value)?),
            Self::Double(value) => {
                EntityProperty::Double(parse_persisted_double(&value, strict_mode)?)
            }
            Self::DateTime(value) => EntityProperty::DateTime(
                crate::codec::datetime::parse_datetime(&value)
                    .map_err(|error| format!("invalid persisted DateTime value: {error}"))?,
            ),
            Self::Guid(value) => EntityProperty::Guid(
                uuid::Uuid::parse_str(&value)
                    .map_err(|_| "invalid persisted Guid value".to_owned())?,
            ),
            Self::Binary(value) => EntityProperty::Binary(
                STANDARD
                    .decode(value.as_bytes())
                    .map_err(|_| "invalid persisted Binary value".to_owned())?,
            ),
        })
    }
}

fn parse_persisted_int64(value: &str) -> std::result::Result<i64, String> {
    if value.is_empty() {
        return Err("invalid persisted Int64 value: value cannot be empty".to_owned());
    }
    if value.trim() != value {
        return Err(
            "invalid persisted Int64 value: value cannot contain leading or trailing whitespace"
                .to_owned(),
        );
    }

    value
        .parse::<i64>()
        .map_err(|error| format!("invalid persisted Int64 value: {error}"))
}

fn parse_persisted_double(value: &str, strict_mode: bool) -> std::result::Result<f64, String> {
    if value.is_empty() {
        return Err("invalid persisted Double value: value cannot be empty".to_owned());
    }
    if value.trim() != value {
        return Err(
            "invalid persisted Double value: value cannot contain leading or trailing whitespace"
                .to_owned(),
        );
    }

    match value {
        "NaN" => {
            if strict_mode {
                Err("invalid persisted Double value: NaN is not allowed in strict_mode".to_owned())
            } else {
                Ok(f64::NAN)
            }
        }
        "Infinity" | "inf" => {
            if strict_mode {
                Err(
                    "invalid persisted Double value: Infinity is not allowed in strict_mode"
                        .to_owned(),
                )
            } else {
                Ok(f64::INFINITY)
            }
        }
        "-Infinity" | "-inf" => {
            if strict_mode {
                Err(
                    "invalid persisted Double value: -Infinity is not allowed in strict_mode"
                        .to_owned(),
                )
            } else {
                Ok(f64::NEG_INFINITY)
            }
        }
        _ => {
            let parsed = value
                .parse::<f64>()
                .map_err(|error| format!("invalid persisted Double value: {error}"))?;
            if !parsed.is_finite() {
                return Err(
                    "invalid persisted Double value: non-finite values are not supported"
                        .to_owned(),
                );
            }
            Ok(parsed)
        }
    }
}
