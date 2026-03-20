# bonjin_ATS_helper

Rust から Azure Table Storage を扱うためのクライアントライブラリです。
`TableServiceClient` と `TableClient` を中心に、テーブル操作、Entity 操作、型付き API、動的 Entity API を提供します。
real ATS backend と永続化 mock backend を同じ client API で切り替えられます。

## 特徴

- `TableServiceClient` / `TableClient` ベースの client 方式
- Shared Key 認証と SAS 認証に対応
- 型付き Entity と `DynamicEntity` の両方を扱える
- テーブル CRUD、Entity CRUD、upsert、query、paging をサポート
- 永続化 mock backend をサポート
- `reqwest` の型を公開 API に出さない設計
- `rustls-tls` を既定で有効化

## 対応環境

- Rust 2024 edition
- `reqwest 0.12`
- `tokio` を使った async 実行環境

## インストール

`Cargo.toml` に追加します。

```toml
[dependencies]
bonjin_ATS_helper = { path = "." }
```

TLS 実装を切り替えたい場合は feature を指定します。

```toml
[dependencies]
bonjin_ATS_helper = { path = ".", default-features = false, features = ["native-tls"] }
```

## 使い方

### 1. サービスクライアントを作成する

```rust
use bonjin_ATS_helper::{ClientOptions, SharedKeyCredential, TableServiceClient};

let credential = SharedKeyCredential::new(
    "myaccount",
    "base64-encoded-account-key",
)?;

let service = TableServiceClient::new(
    "https://myaccount.table.core.windows.net",
    credential,
    ClientOptions::default(),
)?;
```

SAS を使う場合は `SasCredential` を渡します。

```rust
use bonjin_ATS_helper::{ClientOptions, SasCredential, TableServiceClient};

let service = TableServiceClient::new(
    "https://myaccount.table.core.windows.net",
    SasCredential::new("sv=...&sig=...")?,
    ClientOptions::default(),
)?;
```

### 2. テーブルクライアントを取得する

```rust
let table = service.table_client("Users")?;
table.create_if_not_exists().await?;
```

### 2b. 永続化 mock backend を使う

```rust
use bonjin_ATS_helper::{MockOptions, TableServiceClient};

let service = TableServiceClient::new_mock(
    MockOptions::new("./.ats-mock")
)?;

let table = service.table_client("Users")?;
table.create_if_not_exists().await?;
```

`MockOptions` では以下を設定できます。

- `root_path`
- `auto_create_if_missing`。root directory と `manifest.json` を自動作成するかを制御します。存在しないテーブルは自動作成しません。
- `strict_mode`。既定は `true` です。
- `flush_policy`。既定は `FlushPolicy::WriteThrough` です。
- `durability_mode`。既定は `DurabilityMode::Fast` です。

`FlushPolicy::Manual` を使う場合は `service.flush().await?` で明示的に永続化します。

### 3. 型付き Entity を扱う

```rust
use bonjin_ATS_helper::TableEntity;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct UserEntity {
    #[serde(rename = "PartitionKey")]
    partition_key: String,
    #[serde(rename = "RowKey")]
    row_key: String,
    #[serde(rename = "Name")]
    name: String,
}

impl TableEntity for UserEntity {
    fn partition_key(&self) -> &str {
        &self.partition_key
    }

    fn row_key(&self) -> &str {
        &self.row_key
    }
}

let entity = UserEntity {
    partition_key: "user".into(),
    row_key: "123".into(),
    name: "Ryuhei".into(),
};

table.insert_entity(&entity).await?;
let fetched: UserEntity = table.get_entity("user", "123").await?;
```

### 4. 動的 Entity を扱う

```rust
use bonjin_ATS_helper::{DynamicEntity, EntityProperty};

let mut entity = DynamicEntity::new("user", "123");
entity.insert_property("Name", EntityProperty::String("Ryuhei".into()));

table.insert_dynamic_entity(&entity).await?;
let fetched = table.get_dynamic_entity("user", "123").await?;
```

## 主な API

### `TableServiceClient`

- `new(endpoint, credential, options)`
- `new_mock(options)`
- `table_client(name)`
- `create_table`
- `create_table_if_not_exists`
- `delete_table`
- `list_tables`
- `flush`

### `TableClient`

- `create_if_not_exists`
- `delete`
- `exists`
- `insert_entity`
- `insert_dynamic_entity`
- `get_entity`
- `get_dynamic_entity`
- `update_entity`
- `merge_entity`
- `upsert_replace`
- `upsert_merge`
- `delete_entity`
- `query_entities`

## 設定

`ClientOptions` で以下を設定できます。

- Storage API version
- metadata level
- user agent
- request timeout
- connect timeout
- insecure HTTP allowance

既定の Storage API version は `2026-02-06`、request timeout は 30 秒、connect timeout は 10 秒です。
`http://` エンドポイントはデフォルトでは loopback (`localhost`, `127.0.0.1`, `::1`) のみ許可されます。非 loopback な HTTP を使う場合は `ClientOptions::with_insecure_http_allowed(true)` を明示してください。

## 認証

以下の認証方式をサポートします。

- `SharedKeyCredential`
- `SasCredential`

`Credential` はこれらのラッパーとして使えます。

## テストと開発

このクレートは `wiremock` を使った HTTP レベルのテストを前提にしています。
mock backend 向けには `tempfile` を使った永続化テストと、環境変数がある場合のみ動く real/mock 比較テストを用意しています。

ローカルで確認する場合は通常の Azure Table Storage か Azurite を使えますが、機能差があるため、最終確認は実際の Azure 環境でも行うのが安全です。

## mock backend の差分

- query の `$filter` は real/mock 共通の parser / evaluator を使います。
- ETag は opaque です。real/mock 比較で exact string を比較してはいけません。
- delete table の tombstone 状態は v1 では再現しません。mock では即時削除です。
- 複数プロセスから同じ data directory を共有する用途はサポートしません。

## ライセンス

MIT License
