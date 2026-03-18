use bonjin_ATS_helper::{
    ClientOptions, Credential, DynamicEntity, EntityProperty, IfMatch, Query, SasCredential,
    SharedKeyCredential, TableServiceClient,
};

#[tokio::test]
#[ignore = "requires a real Azure Table Storage account"]
async fn cloud_conformance_smoke_test() {
    let endpoint = std::env::var("ATS_CLOUD_ENDPOINT").expect("ATS_CLOUD_ENDPOINT is required");
    let table_name = std::env::var("ATS_CLOUD_TABLE")
        .unwrap_or_else(|_| format!("CloudTest{}", std::process::id()));

    let credential: Credential = if let Ok(sas) = std::env::var("ATS_CLOUD_SAS") {
        Credential::from(SasCredential::new(sas).unwrap())
    } else {
        let account_name = std::env::var("ATS_ACCOUNT_NAME")
            .expect("ATS_ACCOUNT_NAME is required when SAS is absent");
        let account_key = std::env::var("ATS_ACCOUNT_KEY")
            .expect("ATS_ACCOUNT_KEY is required when SAS is absent");
        Credential::from(SharedKeyCredential::new(account_name, account_key).unwrap())
    };

    let service = TableServiceClient::new(endpoint, credential, ClientOptions::default()).unwrap();
    let table = service.table_client(&table_name).unwrap();
    table.create_if_not_exists().await.unwrap();

    let mut entity = DynamicEntity::new("conformance", "row-1");
    entity.insert_property("name", EntityProperty::String("cloud".to_owned()));
    table.upsert_replace_dynamic(&entity).await.unwrap();

    let fetched = table
        .get_dynamic_entity("conformance", "row-1")
        .await
        .unwrap();
    let etag = fetched.etag.clone().expect("etag must exist");
    table
        .delete_entity("conformance", "row-1", IfMatch::Etag(etag))
        .await
        .unwrap();

    let page = table
        .query_dynamic_entities(
            Query::new()
                .with_filter("PartitionKey eq 'conformance'")
                .unwrap()
                .with_top(10)
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(
        page.items.is_empty()
            || page
                .items
                .iter()
                .all(|item| item.partition_key == "conformance")
    );

    table.delete().await.unwrap();
}
