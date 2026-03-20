use bonjin_ATS_helper::{
    ClientOptions, Credential, DynamicEntity, EntityProperty, IfMatch, MockOptions, Query,
    SasCredential, SharedKeyCredential, TableServiceClient,
};
use tempfile::TempDir;

fn cloud_service() -> Option<TableServiceClient> {
    let endpoint = std::env::var("ATS_CLOUD_ENDPOINT").ok()?;
    let credential: Credential = if let Ok(sas) = std::env::var("ATS_CLOUD_SAS") {
        SasCredential::new(sas).ok()?.into()
    } else {
        let account_name = std::env::var("ATS_ACCOUNT_NAME").ok()?;
        let account_key = std::env::var("ATS_ACCOUNT_KEY").ok()?;
        SharedKeyCredential::new(account_name, account_key)
            .ok()?
            .into()
    };

    TableServiceClient::new(endpoint, credential, ClientOptions::default()).ok()
}

fn seed_entity(partition_key: &str, row_key: &str, name: &str, score: i32) -> DynamicEntity {
    let mut entity = DynamicEntity::new(partition_key, row_key);
    entity.insert_property("Name", EntityProperty::String(name.to_owned()));
    entity.insert_property("Score", EntityProperty::Int32(score));
    entity
}

#[tokio::test]
#[ignore = "requires a real Azure Table Storage account"]
async fn mock_and_cloud_match_crud_and_query_semantics() {
    let Some(cloud) = cloud_service() else {
        return;
    };

    let tempdir = TempDir::new().unwrap();
    let mock = TableServiceClient::new_mock(MockOptions::new(tempdir.path())).unwrap();

    let cloud_table_name = std::env::var("ATS_CLOUD_TABLE")
        .unwrap_or_else(|_| format!("Compare{}", std::process::id()));
    let mock_table_name = "CompareUsers";

    let cloud_table = cloud.table_client(&cloud_table_name).unwrap();
    let mock_table = mock.table_client(mock_table_name).unwrap();

    cloud_table.create_if_not_exists().await.unwrap();
    mock_table.create_if_not_exists().await.unwrap();

    for entity in [
        seed_entity("user", "1", "alice", 10),
        seed_entity("user", "2", "bob", 20),
    ] {
        cloud_table.upsert_replace_dynamic(&entity).await.unwrap();
        mock_table.upsert_replace_dynamic(&entity).await.unwrap();
    }

    let mut merge = DynamicEntity::new("user", "1");
    merge.insert_property("Name", EntityProperty::String("alice-v2".to_owned()));
    cloud_table
        .merge_dynamic_entity(&merge, IfMatch::Any)
        .await
        .unwrap();
    mock_table
        .merge_dynamic_entity(&merge, IfMatch::Any)
        .await
        .unwrap();

    let cloud_entity = cloud_table.get_dynamic_entity("user", "1").await.unwrap();
    let mock_entity = mock_table.get_dynamic_entity("user", "1").await.unwrap();
    assert_eq!(cloud_entity.partition_key, mock_entity.partition_key);
    assert_eq!(cloud_entity.row_key, mock_entity.row_key);
    assert_eq!(cloud_entity.properties, mock_entity.properties);

    let query = Query::new()
        .with_filter("PartitionKey eq 'user'")
        .unwrap()
        .with_top(10)
        .unwrap()
        .with_select(["Name"]);
    let cloud_page = cloud_table
        .query_dynamic_entities(query.clone())
        .await
        .unwrap();
    let mock_page = mock_table.query_dynamic_entities(query).await.unwrap();

    let cloud_keys = cloud_page
        .items
        .iter()
        .map(|item| (item.partition_key.clone(), item.row_key.clone()))
        .collect::<Vec<_>>();
    let mock_keys = mock_page
        .items
        .iter()
        .map(|item| (item.partition_key.clone(), item.row_key.clone()))
        .collect::<Vec<_>>();
    assert_eq!(cloud_keys, mock_keys);
    assert_eq!(
        cloud_page
            .items
            .iter()
            .map(|item| item.properties.clone())
            .collect::<Vec<_>>(),
        mock_page
            .items
            .iter()
            .map(|item| item.properties.clone())
            .collect::<Vec<_>>()
    );

    let cloud_etag = cloud_entity.etag.clone().expect("cloud etag");
    let mock_etag = mock_entity.etag.clone().expect("mock etag");
    cloud_table
        .delete_entity("user", "1", IfMatch::Etag(cloud_etag))
        .await
        .unwrap();
    mock_table
        .delete_entity("user", "1", IfMatch::Etag(mock_etag))
        .await
        .unwrap();

    cloud_table.delete().await.unwrap();
    mock_table.delete().await.unwrap();
}
