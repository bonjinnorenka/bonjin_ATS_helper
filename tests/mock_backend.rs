use std::fs;

use bonjin_ATS_helper::{
    DynamicEntity, EntityProperty, FlushPolicy, IfMatch, MockOptions, Query, TableEntity,
    TableServiceClient,
};
use serde::{Deserialize, Serialize};
use tempfile::TempDir;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct ReplaceEntity {
    #[serde(rename = "PartitionKey")]
    partition_key: String,
    #[serde(rename = "RowKey")]
    row_key: String,
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "Tag")]
    tag: Option<String>,
}

impl TableEntity for ReplaceEntity {
    fn partition_key(&self) -> &str {
        &self.partition_key
    }

    fn row_key(&self) -> &str {
        &self.row_key
    }
}

fn open_mock_service(tempdir: &TempDir) -> TableServiceClient {
    TableServiceClient::new_mock(MockOptions::new(tempdir.path())).unwrap()
}

fn rewrite_persisted_property(
    tempdir: &TempDir,
    table_name: &str,
    property_name: &str,
    persisted_value: serde_json::Value,
) {
    let table_path = tempdir
        .path()
        .join("tables")
        .join(format!("{table_name}.json"));
    let mut table: serde_json::Value =
        serde_json::from_slice(&fs::read(&table_path).unwrap()).unwrap();
    table["entities"][0]["properties"][property_name]["value"] = persisted_value;
    fs::write(&table_path, serde_json::to_vec_pretty(&table).unwrap()).unwrap();
}

#[tokio::test]
async fn mock_persists_across_restart() {
    let tempdir = TempDir::new().unwrap();
    let service = open_mock_service(&tempdir);
    let table = service.table_client("Users").unwrap();

    table.create_if_not_exists().await.unwrap();
    let mut entity = DynamicEntity::new("user", "1");
    entity.insert_property("name", EntityProperty::String("alice".to_owned()));
    table.insert_dynamic_entity(&entity).await.unwrap();

    drop(table);
    drop(service);

    let reopened = open_mock_service(&tempdir);
    let table = reopened.table_client("Users").unwrap();
    let fetched = table.get_dynamic_entity("user", "1").await.unwrap();

    assert_eq!(
        fetched.properties.get("name"),
        Some(&EntityProperty::String("alice".to_owned()))
    );
    assert!(fetched.etag.is_some());
    assert!(fetched.timestamp.is_some());
}

#[tokio::test]
async fn manual_flush_defers_persistence_until_flush() {
    let tempdir = TempDir::new().unwrap();
    let options = MockOptions::new(tempdir.path()).with_flush_policy(FlushPolicy::Manual);
    let service = TableServiceClient::new_mock(options).unwrap();
    let table = service.table_client("Users").unwrap();

    table.create_if_not_exists().await.unwrap();
    let mut entity = DynamicEntity::new("user", "1");
    entity.insert_property("name", EntityProperty::String("alice".to_owned()));
    table.insert_dynamic_entity(&entity).await.unwrap();

    let reopened = open_mock_service(&tempdir);
    let reopened_table = reopened.table_client("Users").unwrap();
    assert!(!reopened_table.exists().await.unwrap());

    service.flush().await.unwrap();

    let reopened = open_mock_service(&tempdir);
    let reopened_table = reopened.table_client("Users").unwrap();
    assert!(reopened_table.exists().await.unwrap());
    let fetched = reopened_table
        .get_dynamic_entity("user", "1")
        .await
        .unwrap();
    assert_eq!(
        fetched.properties.get("name"),
        Some(&EntityProperty::String("alice".to_owned()))
    );
}

#[tokio::test]
async fn replace_removes_missing_properties_and_merge_keeps_them() {
    let tempdir = TempDir::new().unwrap();
    let service = open_mock_service(&tempdir);
    let table = service.table_client("Users").unwrap();
    table.create_if_not_exists().await.unwrap();

    let original = ReplaceEntity {
        partition_key: "user".to_owned(),
        row_key: "1".to_owned(),
        name: "alice".to_owned(),
        tag: Some("blue".to_owned()),
    };
    table.insert_entity(&original).await.unwrap();

    let replaced = ReplaceEntity {
        partition_key: "user".to_owned(),
        row_key: "1".to_owned(),
        name: "alice-v2".to_owned(),
        tag: None,
    };
    table.update_entity(&replaced, IfMatch::Any).await.unwrap();

    let after_replace = table.get_dynamic_entity("user", "1").await.unwrap();
    assert_eq!(
        after_replace.properties.get("Name"),
        Some(&EntityProperty::String("alice-v2".to_owned()))
    );
    assert!(!after_replace.properties.contains_key("Tag"));

    let mut merge = DynamicEntity::new("user", "1");
    merge.insert_property("Name", EntityProperty::String("alice-v3".to_owned()));
    table
        .merge_dynamic_entity(&merge, IfMatch::Any)
        .await
        .unwrap();

    let after_merge = table.get_dynamic_entity("user", "1").await.unwrap();
    assert_eq!(
        after_merge.properties.get("Name"),
        Some(&EntityProperty::String("alice-v3".to_owned()))
    );
    assert!(!after_merge.properties.contains_key("Tag"));

    let mut seed = DynamicEntity::new("user", "2");
    seed.insert_property("Name", EntityProperty::String("bob".to_owned()));
    seed.insert_property("Tag", EntityProperty::String("green".to_owned()));
    table.insert_dynamic_entity(&seed).await.unwrap();

    let mut merge = DynamicEntity::new("user", "2");
    merge.insert_property("Name", EntityProperty::String("bob-v2".to_owned()));
    table
        .merge_dynamic_entity(&merge, IfMatch::Any)
        .await
        .unwrap();

    let merged = table.get_dynamic_entity("user", "2").await.unwrap();
    assert_eq!(
        merged.properties.get("Tag"),
        Some(&EntityProperty::String("green".to_owned()))
    );
}

#[tokio::test]
async fn etag_conditions_are_enforced() {
    let tempdir = TempDir::new().unwrap();
    let service = open_mock_service(&tempdir);
    let table = service.table_client("Users").unwrap();
    table.create_if_not_exists().await.unwrap();

    let mut entity = DynamicEntity::new("user", "1");
    entity.insert_property("name", EntityProperty::String("alice".to_owned()));
    table.insert_dynamic_entity(&entity).await.unwrap();

    let fetched = table.get_dynamic_entity("user", "1").await.unwrap();
    let etag = fetched.etag.clone().unwrap();

    let mut replacement = DynamicEntity::new("user", "1");
    replacement.insert_property("name", EntityProperty::String("alice-v2".to_owned()));
    table
        .update_dynamic_entity(&replacement, IfMatch::Etag(etag.clone()))
        .await
        .unwrap();

    let error = table
        .delete_entity("user", "1", IfMatch::Etag(etag))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("412"));

    table
        .delete_entity("user", "1", IfMatch::Any)
        .await
        .unwrap();
}

#[tokio::test]
async fn query_supports_filter_select_and_zero_item_continuation() {
    let tempdir = TempDir::new().unwrap();
    let service = open_mock_service(&tempdir);
    let table = service.table_client("Users").unwrap();
    table.create_if_not_exists().await.unwrap();

    for index in 0..20 {
        let mut entity = DynamicEntity::new("other", format!("{index:02}"));
        entity.insert_property("score", EntityProperty::Int32(index));
        entity.insert_property("name", EntityProperty::String(format!("other-{index:02}")));
        table.insert_dynamic_entity(&entity).await.unwrap();
    }

    for (partition_key, row_key, score) in [("user", "1", 10), ("user", "2", 20)] {
        let mut entity = DynamicEntity::new(partition_key, row_key);
        entity.insert_property("score", EntityProperty::Int32(score));
        entity.insert_property(
            "name",
            EntityProperty::String(format!("{partition_key}-{row_key}")),
        );
        table.insert_dynamic_entity(&entity).await.unwrap();
    }

    let first_page = table
        .query_dynamic_entities(
            Query::new()
                .with_filter("PartitionKey eq 'user'")
                .unwrap()
                .with_top(1)
                .unwrap()
                .with_select(["name"]),
        )
        .await
        .unwrap();
    assert!(first_page.items.is_empty());
    let continuation = first_page.continuation.expect("continuation");

    let second_page = table
        .query_dynamic_entities_next(&continuation)
        .await
        .unwrap();
    assert_eq!(second_page.items.len(), 1);
    assert_eq!(
        second_page.items[0].properties.get("name"),
        Some(&EntityProperty::String("user-1".to_owned()))
    );
    assert!(!second_page.items[0].properties.contains_key("score"));
}

#[tokio::test]
async fn top_limits_returned_matches_not_scanned_rows() {
    let tempdir = TempDir::new().unwrap();
    let service = open_mock_service(&tempdir);
    let table = service.table_client("Users").unwrap();
    table.create_if_not_exists().await.unwrap();

    for (partition_key, row_key, score) in [
        ("other", "0", 1),
        ("user", "1", 10),
        ("user", "2", 20),
        ("user", "3", 30),
    ] {
        let mut entity = DynamicEntity::new(partition_key, row_key);
        entity.insert_property("score", EntityProperty::Int32(score));
        entity.insert_property(
            "name",
            EntityProperty::String(format!("{partition_key}-{row_key}")),
        );
        table.insert_dynamic_entity(&entity).await.unwrap();
    }

    let page = table
        .query_dynamic_entities(
            Query::new()
                .with_filter("PartitionKey eq 'user'")
                .unwrap()
                .with_top(2)
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(page.items.len(), 2);
    assert_eq!(page.items[0].row_key, "1");
    assert_eq!(page.items[1].row_key, "2");
}

#[test]
fn corrupted_store_fails_fast() {
    let tempdir = TempDir::new().unwrap();
    let _service = open_mock_service(&tempdir);

    fs::write(tempdir.path().join("manifest.json"), "{not valid json").unwrap();

    let error = match TableServiceClient::new_mock(MockOptions::new(tempdir.path())) {
        Ok(_) => panic!("expected corrupted store error"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("corrupted mock store"));
}

#[test]
fn store_format_mismatch_fails_fast() {
    let tempdir = TempDir::new().unwrap();
    let _service = open_mock_service(&tempdir);

    let manifest_path = tempdir.path().join("manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest["store_format_version"] = serde_json::json!(999);
    fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();

    let error = match TableServiceClient::new_mock(MockOptions::new(tempdir.path())) {
        Ok(_) => panic!("expected store format mismatch"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("store_format_version"));
}

#[test]
fn corrupted_store_rejects_invalid_persisted_int64() {
    let tempdir = TempDir::new().unwrap();
    let service = open_mock_service(&tempdir);
    let table = service.table_client("Users").unwrap();

    tokio::runtime::Runtime::new().unwrap().block_on(async {
        table.create_if_not_exists().await.unwrap();
        let mut entity = DynamicEntity::new("user", "1");
        entity.insert_property("Count", EntityProperty::Int64(42));
        table.insert_dynamic_entity(&entity).await.unwrap();
    });

    rewrite_persisted_property(
        &tempdir,
        "Users",
        "Count",
        serde_json::json!("not_a_number"),
    );

    let error = match TableServiceClient::new_mock(MockOptions::new(tempdir.path())) {
        Ok(_) => panic!("expected corrupted store error"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("corrupted mock store"));
    assert!(error.to_string().contains("Int64"));
}

#[test]
fn corrupted_store_rejects_special_double_in_strict_mode() {
    let tempdir = TempDir::new().unwrap();
    let service = open_mock_service(&tempdir);
    let table = service.table_client("Users").unwrap();

    tokio::runtime::Runtime::new().unwrap().block_on(async {
        table.create_if_not_exists().await.unwrap();
        let mut entity = DynamicEntity::new("user", "1");
        entity.insert_property("AmountDue", EntityProperty::Double(200.23));
        table.insert_dynamic_entity(&entity).await.unwrap();
    });

    rewrite_persisted_property(&tempdir, "Users", "AmountDue", serde_json::json!("NaN"));

    let error = match TableServiceClient::new_mock(MockOptions::new(tempdir.path())) {
        Ok(_) => panic!("expected corrupted store error"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("corrupted mock store"));
    assert!(error.to_string().contains("strict_mode"));
}

#[tokio::test]
async fn relaxed_mode_allows_special_double_on_restore() {
    let tempdir = TempDir::new().unwrap();
    let options = MockOptions::new(tempdir.path()).with_strict_mode(false);
    let service = TableServiceClient::new_mock(options.clone()).unwrap();
    let table = service.table_client("Users").unwrap();
    table.create_if_not_exists().await.unwrap();

    let mut entity = DynamicEntity::new("user", "1");
    entity.insert_property("AmountDue", EntityProperty::Double(200.23));
    table.insert_dynamic_entity(&entity).await.unwrap();

    drop(table);
    drop(service);

    rewrite_persisted_property(&tempdir, "Users", "AmountDue", serde_json::json!("NaN"));

    let reopened = TableServiceClient::new_mock(options).unwrap();
    let table = reopened.table_client("Users").unwrap();
    let fetched = table.get_dynamic_entity("user", "1").await.unwrap();
    let amount_due = fetched.properties.get("AmountDue").unwrap();
    match amount_due {
        EntityProperty::Double(value) => assert!(value.is_nan()),
        other => panic!("expected Double, got {other:?}"),
    }
}
