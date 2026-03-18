use bonjin_ATS_helper::{
    ClientOptions, DynamicEntity, EntityProperty, Query, SharedKeyCredential, TableServiceClient,
};

#[tokio::test]
#[ignore = "requires a running Azurite Table endpoint"]
async fn azurite_crud_roundtrip() {
    if std::env::var("ATS_CLOUD_ENDPOINT").is_ok()
        && std::env::var("AZURITE_TABLE_ENDPOINT").is_err()
    {
        return;
    }

    let endpoint = std::env::var("AZURITE_TABLE_ENDPOINT")
        .unwrap_or_else(|_| "http://127.0.0.1:10002/devstoreaccount1".to_owned());
    let account_name =
        std::env::var("AZURITE_ACCOUNT_NAME").unwrap_or_else(|_| "devstoreaccount1".to_owned());
    let account_key = std::env::var("AZURITE_ACCOUNT_KEY").unwrap_or_else(|_| {
        "Eby8vdM02xNOcqFlqUwJPLlmEtlCDXJ1OUzFT50uSRZ6IFsuFq2UVErCz4I6tq/K1SZFPTOtr/KBHBeksoGMGw==".to_owned()
    });
    let table_name = format!("AzuriteTest{}", std::process::id());

    let service = TableServiceClient::new(
        endpoint,
        SharedKeyCredential::new(account_name, account_key).unwrap(),
        ClientOptions::default(),
    )
    .unwrap();
    let table = service.table_client(&table_name).unwrap();

    table.create_if_not_exists().await.unwrap();

    let mut entity = DynamicEntity::new("pk", "rk");
    entity.insert_property("name", EntityProperty::String("azurite".to_owned()));
    entity.insert_property("count", EntityProperty::Int32(1));
    table.insert_dynamic_entity(&entity).await.unwrap();

    let fetched = table.get_dynamic_entity("pk", "rk").await.unwrap();
    assert_eq!(
        fetched.properties.get("name"),
        Some(&EntityProperty::String("azurite".to_owned()))
    );

    let page = table
        .query_dynamic_entities(Query::new().with_top(10).unwrap())
        .await
        .unwrap();
    assert!(!page.items.is_empty());

    table
        .delete_entity("pk", "rk", bonjin_ATS_helper::IfMatch::Any)
        .await
        .unwrap();
    table.delete().await.unwrap();
}
