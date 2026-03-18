use bonjin_ATS_helper::{
    ClientOptions, DynamicEntity, EntityProperty, Query, SasCredential, TableServiceClient,
};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{body_string_contains, header, method, path, query_param},
};

#[tokio::test]
async fn insert_dynamic_entity_sends_expected_request_shape() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/Users"))
        .and(query_param("sv", "2025-01-01"))
        .and(query_param("sig", "abc+123"))
        .and(header("accept", "application/json;odata=nometadata"))
        .and(header("dataserviceversion", "3.0;NetFx"))
        .and(header("maxdataserviceversion", "3.0;NetFx"))
        .and(header("x-ms-version", "2026-02-06"))
        .and(header("content-type", "application/json"))
        .and(body_string_contains("\"PartitionKey\":\"user\""))
        .and(body_string_contains("\"RowKey\":\"123\""))
        .and(body_string_contains("\"score\":\"9001\""))
        .and(body_string_contains("\"score@odata.type\":\"Edm.Int64\""))
        .respond_with(ResponseTemplate::new(201))
        .mount(&server)
        .await;

    let service = TableServiceClient::new(
        server.uri(),
        SasCredential::new("sv=2025-01-01&sig=abc%2B123").unwrap(),
        ClientOptions::default(),
    )
    .unwrap();
    let table = service.table_client("Users").unwrap();

    let mut entity = DynamicEntity::new("user", "123");
    entity.insert_property("name", EntityProperty::String("Ryuhei".to_owned()));
    entity.insert_property("score", EntityProperty::Int64(9001));

    table.insert_dynamic_entity(&entity).await.unwrap();
}

#[tokio::test]
async fn query_dynamic_entities_preserves_query_and_continuation() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/Users()"))
        .and(query_param("$filter", "PartitionKey eq 'user'"))
        .and(query_param("$top", "10"))
        .and(query_param("$select", "name,score"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("x-ms-request-id", "req-123")
                .insert_header("x-ms-continuation-NextPartitionKey", "user")
                .insert_header("x-ms-continuation-NextRowKey", "124")
                .set_body_json(serde_json::json!({
                    "value": []
                })),
        )
        .mount(&server)
        .await;

    let service = TableServiceClient::new(
        server.uri(),
        SasCredential::new("sv=2025-01-01&sig=abc").unwrap(),
        ClientOptions::default(),
    )
    .unwrap();
    let table = service.table_client("Users").unwrap();
    let query = Query::new()
        .with_filter("PartitionKey eq 'user'")
        .unwrap()
        .with_top(10)
        .unwrap()
        .with_select(["name", "score"]);

    let page = table.query_dynamic_entities(query).await.unwrap();

    assert!(page.items.is_empty());
    assert_eq!(page.request_id.as_deref(), Some("req-123"));
    let continuation = page.continuation.expect("continuation token");
    assert_eq!(continuation.next_partition_key.as_deref(), Some("user"));
    assert_eq!(continuation.next_row_key.as_deref(), Some("124"));
    assert_eq!(
        continuation.original_query.filter.as_deref(),
        Some("PartitionKey eq 'user'")
    );
    assert_eq!(continuation.original_query.top, Some(10));
    assert_eq!(
        continuation.original_query.select,
        vec!["name".to_owned(), "score".to_owned()]
    );
}
