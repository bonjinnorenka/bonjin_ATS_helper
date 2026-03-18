# Rust 製 Azure Table Storage クライアントライブラリ 実装指示書（改訂版）

## 文書の位置づけ
この文書は、これまでの会話で確定した要件と、2026-03-18 時点で確認した一次情報をもとに、**Rust で Azure Table Storage (ATS) 向けの client 方式ライブラリを自作するための実装指示**をまとめたものです。

ここでいう **client 方式** とは、都度生の HTTP を各所で組み立てるのではなく、以下のようなクライアントオブジェクト経由で操作する方式を指します。

- `TableServiceClient`
- `TableClient`
- 必要に応じて将来 `BatchClient` 相当

この文書は **実装担当者へ直接渡す前提**で書いているため、着手方針ではなく、**API 境界・責務分割・禁止事項・受け入れ条件**まで明示します。

---

## 1. 前提として確定している要件

### 1.1 過去会話から確定しているもの
1. 対象は **Azure Table Storage** であり、Rust で使うための **自作ライブラリ / 自作 SDK もどき** を作る。
2. 設計は **client 方式** を採用する。認証情報はクライアントに保持し、各リクエストで参照する。
3. v1 で最低限扱う認証方式は **Shared Key** と **SAS**。
4. ATS は DynamoDB のように主キー名を自由設計するものではなく、`PartitionKey` と `RowKey` が固定のシステム項目である。
5. 利用者は比較的 **型が定まった使い方** を想定しているが、ATS のスキーマレス性は捨てない。
6. テストしやすい設計は必要だが、テーブル名以外の大量なスキーマ事前登録は望まれていない。

### 1.2 仕様上の重要事実
1. ATS の各 entity は `PartitionKey` と `RowKey` を持ち、この組で一意になる。`Timestamp` はサーバー管理で、更新のたびに増加する。書き込み側で設定しても無視される。  
2. ATS はスキーマレスで、同一テーブル内でも entity ごとに異なるプロパティ集合を持てる。  
3. Table service の Shared Key 署名は Blob/Queue/File と完全には同じではない。**Table service の Shared Key 署名文字列には `CanonicalizedHeaders` を含めない**。また `x-ms-date` を使う場合でも、署名上の `Date` は空にせずその値を使う。  
4. Table service の OData 対応操作では、JSON が推奨であり、**2015-12-11 以降は JSON が必須**。`Accept` を省略すると既定は `application/atom+xml` 側に倒れる。  
5. OData 互換操作では `DataServiceVersion` / `MaxDataServiceVersion` と `x-ms-version` の整合を取る必要がある。  
6. Query は **1 回あたり最大 1000 件、実行最大 5 秒、要求全体最大 30 秒**。continuation token は件数超過時だけでなく、**5 秒制限到達時や partition boundary をまたいだ場合にも返る**。次ページ要求では元の URI 条件を維持する必要がある。`$top` は全体上限ではなく **1 ページあたり上限**。  
7. `$filter` では **15 個を超える離散比較は不可**。  
8. Entity Group Transactions (EGT) は **同一 `PartitionKey` の entity に対してのみ** 使え、**最大 100 件・総ペイロード 4 MiB**。OData batch (`multipart/mixed`) を使う。  
9. entity 制約として、**最大 255 プロパティ（`PartitionKey` / `RowKey` / `Timestamp` 含む）**、**entity 全体 1 MiB**、`PartitionKey` / `RowKey` は **最大 1024 文字**。`null` プロパティは永続化されず、送らなかったのと同じ扱いになる。  
10. `x-ms-version` は最新追従ではなく、**ライブラリ内で試験済み定数として固定**すべき。地域未展開のバージョンでは mismatch が起こりうる。  
11. ローカル試験用には Azurite が使えるが、クラウドと機能差がある。旧 Azure Storage Emulator は非推奨。  

---

## 2. このライブラリで解決すべきこと

### 必須
- Rust から ATS を使うための安定した **client 方式 API** を提供する。
- Shared Key / SAS を用いた認証付きリクエストを正しく送れる。
- テーブル操作と entity 操作を明確に分離する。
- **型付き API** と **動的 property bag API** の両方を持つ。
- 将来 EGT を載せても壊れない内部設計にする。
- テスト容易性を確保し、署名・URI・ペイロード・ページングを分離検証できるようにする。

### v1 で最低限必要
- テーブル CRUD
- entity CRUD
- upsert
- query + paging
- ETag 条件付き更新/削除
- 基本的なエラー分類
- ローカル/クラウド両方の結合テスト導線

### 後回しでよい
- Entra ID 完全対応
- 高水準の query DSL 完成版
- 高水準の batch DSL
- Cosmos DB Table API 差異吸収
- derive macro による自動 entity 実装

---

## 3. 非目標

以下は v1 の目標に含めない。

1. ATS 上に RDB 的スキーマ管理を持ち込むこと。
2. DynamoDB 風に主キー名自体を自由定義すること。
3. コンパイル時に ATS のクエリ制約を完全表現すること。
4. Azure Portal / Pulumi / ARM の代替になること。
5. EGT の高水準 DSL を最初から公開すること。
6. 最新 `x-ms-version` へ自動追従すること。

---

## 4. 公開 API の基本方針

## 4.1 推奨 API 形

```rust
let service = TableServiceClient::new(endpoint, credential, options)?;
let table = service.table_client("MyTable")?;

table.create_if_not_exists().await?;

let entity = UserEntity {
    partition_key: "user".into(),
    row_key: "123".into(),
    name: "Ryuhei".into(),
    etag: None,
};

table.insert_entity(&entity).await?;
let fetched: UserEntity = table.get_entity("user", "123").await?;
```

## 4.2 クライアント階層

### `TableServiceClient`
責務:
- endpoint 保持
- 認証保持
- 共通 options 保持
- テーブル作成/削除/一覧
- `table_client(name)` の生成

### `TableClient`
責務:
- 特定テーブルに束縛された entity 操作
- query / paging
- upsert
- existence check
- 条件付き更新/削除

### `RawTableClient` または `TableClient` 内低水準メソッド
責務:
- `DynamicEntity` / property bag ベースの入出力
- 型付き API で覆い切れない機能の退避口

### 指示
- `table_client(name)` は **ローカルオブジェクト生成のみ** とし、その場で存在確認しないこと。
- クライアント型は `Clone` 可能にすること。内部は `Arc` ベースでよい。
- 公開 API は `reqwest` 型を一切露出しないこと。

---

## 5. 認証設計

## 5.1 v1 で必須
- `SharedKeyCredential`
- `SasCredential`

## 5.2 v2 以降候補
- `BearerTokenCredential`（Entra ID 用）

## 5.3 認証抽象の方針
公開 API として「何でも差し替え可能な認証 trait」をむやみに外へ出さないこと。

- v1 では `Credential` enum か sealed trait でよい。
- 外部拡張ポイントが必要になった時点で、公開拡張性を再設計する。
- 署名処理そのものは `auth` モジュールに隔離し、クライアント本体に埋め込まない。

## 5.4 Table service 固有の Shared Key 実装指示
Blob/Queue/File の Shared Key 実装と **共通化しすぎないこと**。

Table service の Shared Key 署名文字列は次を基本とする。

```text
StringToSign = VERB + "\n" +
               Content-MD5 + "\n" +
               Content-Type + "\n" +
               Date + "\n" +
               CanonicalizedResource
```

### 指示
- Table service では **`CanonicalizedHeaders` を署名文字列に入れない**こと。
- `x-ms-date` を送る場合でも、署名上の `Date` は空にしないこと。`x-ms-date` の値を `Date` として使うこと。
- `Authorization` の構築は **Table 専用実装** として分離すること。
- `SharedKeyLite` は将来用余地として残してよいが、v1 の主対象は通常の Shared Key とする。

## 5.5 OData / バージョン系ヘッダの既定値
OData 互換操作では、ヘッダを明示的に固定運用する。

### v1 の既定
- `x-ms-version`: crate 内の **試験済み固定定数** を使う。
- `DataServiceVersion: 3.0;NetFx`
- `MaxDataServiceVersion: 3.0;NetFx`

### 指示
- `x-ms-version` は「その時点の最新」ではなく、**crate が試験した固定値**を使うこと。
- 初期既定値は `2026-02-06` を採用してよい。override は builder option でのみ許可する。
- `x-ms-version` の値は CI の integration test で実際に検証すること。

## 5.6 `PreparedRequest` を先に定義すること
認証・送信・試験の共通単位として、HTTP ライブラリ依存の手前に `PreparedRequest` を置く。

```rust
pub struct PreparedRequest {
    pub method: http::Method,
    pub url: url::Url,
    pub headers: http::HeaderMap,
    pub body: bytes::Bytes,
    pub content_md5: Option<String>,
    pub content_type: Option<String>,
    pub signing_date: String,
    pub canonicalized_resource: String,
}
```

### 指示
- 認証器は `PreparedRequest` を受け取り、ヘッダを付与すること。
- リクエスト生成・署名・送信を同じ関数に押し込まないこと。
- 将来 `multipart/mixed` を送るため、body は `Vec<u8>` 固定ではなく `Bytes` 等のバイト列抽象で持てるようにしてよい。

---

## 6. HTTP / Transport 層の設計

## 6.1 基本方針
公開 API として **`HttpTransport` trait や `MockTransport` を外へ出さないこと**。

v1 は以下を基本とする。
- 実運用の HTTP 実装は `reqwest::Client` を内部ラップする。
- テストは主に `wiremock` による **外部 HTTP mock server** を使う。
- middleware が必要なら `reqwest-middleware` 系を検討する。

## 6.2 `async fn in traits` への方針
Rust 1.75 以降では `async fn in traits` と RPITIT が安定化している。したがって、**内部実装だけなら `#[async_trait]` へ無条件依存する必要はない**。

ただし、**公開 trait に `async fn` をそのまま置く設計は避ける**こと。公開 trait では auto trait 境界（特に `Send`）を後から緩められず、API を固定しやすい。

### 指示
- crate 内部の trait なら、ネイティブの `async fn` または `fn -> impl Future + Send` を使ってよい。
- 公開拡張ポイントとして trait を出す必要が生じた場合は、
  - crate-private trait に留める、または
  - `Pin<Box<dyn Future<Output = _> + Send + '_>>` などで戻り値境界を明示する、
  のどちらかを採ること。
- `async-trait` クレートは **必須依存にしない**こと。

## 6.3 公開 API から隠すもの
- `reqwest::Client`
- `reqwest::Request`
- `reqwest::Response`
- `reqwest_middleware::ClientWithMiddleware`

これらは内部実装詳細であり、公開型に入れないこと。

## 6.4 middleware の扱い
- retry / logging / tracing / metrics は transport 直下または middleware 層で扱う。
- ただし v1 では retry を大きく作り込まず、**差し込み位置だけ確保**する。
- 503 / timeout 系に対して exponential backoff を載せられるようにする。

---

## 7. エンティティ表現

## 7.1 方針
ATS はスキーマレスなので、ライブラリは以下の両方を持つこと。

1. **型付きエンティティ API**
2. **動的 property bag API**

## 7.2 型付き API

```rust
pub trait TableEntity: serde::Serialize + for<'de> serde::Deserialize<'de> {
    fn partition_key(&self) -> &str;
    fn row_key(&self) -> &str;
    fn etag(&self) -> Option<&str> { None }
}
```

### 指示
- `etag()` を持てるようにすること。条件付き更新/削除で必要になる。
- `Timestamp` は型付き entity に持たせてもよいが、**送信時は codec 側で除外**すること。
- entity 本体に `PartitionKey` / `RowKey` / `Timestamp` / `etag` を混ぜるか専用フィールドで分けるかは自由だが、**codec で一貫変換**できる形に統一すること。

## 7.3 動的 API

```rust
pub struct DynamicEntity {
    pub partition_key: String,
    pub row_key: String,
    pub properties: indexmap::IndexMap<String, EntityProperty>,
    pub etag: Option<String>,
    pub timestamp: Option<time::OffsetDateTime>,
}
```

### 指示
- property bag の意味論は **順序非依存** とすること。
- ただしテスト容易性のため、実装で `IndexMap` など安定順序の map を使ってもよい。
- **署名用の辞書順整列** と **entity property bag** は別問題なので、同じ map 型に責務を背負わせないこと。

## 7.4 `null` と `Option<T>` の扱い
重要。ATS は `null` プロパティを永続化しない。

### 指示
- `Option<T>::None` は「null を保存する」ではなく、**プロパティ欠落として送る**ものとして扱うこと。
- 読み戻し時、`None` は「元から無かった」と「null を送った」を区別できない前提で扱うこと。
- round-trip の完全性を前提に API を設計しないこと。

---

## 8. シリアライズ / codec 設計

## 8.1 既定ペイロード方針
ATS は OData ベースだが、v1 では **JSON / no metadata 固定運用** を基本にする。

### v1 の既定
- `Content-Type: application/json`
- `Accept: application/json;odata=nometadata`

### 指示
- `Accept` 未指定時の既定が `application/atom+xml` なので、省略しないこと。
- `odata=minimalmetadata` / `odata=fullmetadata` は debug 用または将来用として扱い、v1 の通常パスでは標準にしないこと。

## 8.2 `codec` モジュールの責務
`codec` は以下を担当する。

- entity → JSON body
- JSON body → entity
- ATS の型情報と Rust 型の変換
- システムプロパティの分離/再結合
- OData metadata level 差異の吸収
- 日付時刻のパース/フォーマット

## 8.3 Rust の時刻型
内部標準は **`time::OffsetDateTime`** を推奨する。

### 指示
- `Timestamp` や Edm.DateTime 系は、JSON 上では文字列として来る前提で `codec` が吸収すること。
- 文字列→時刻型変換失敗は `SerializationError` に分類すること。
- `chrono` 互換が必要なら将来 feature flag で追加し、v1 の内部標準は 1 つに固定すること。

## 8.4 ATS 型表現
`codec` 内部に ATS の型表現を持つこと。

例:
- `Edm.String`
- `Edm.Boolean`
- `Edm.Int32`
- `Edm.Int64`
- `Edm.Double`
- `Edm.Guid`
- `Edm.Binary`
- `Edm.DateTime`

### 指示
- 利用者向け API では自然な Rust 型を見せる。
- OData の詳細注釈は内部に閉じ込める。
- `odata=nometadata` 前提でも型が曖昧になる値については codec 側で安全に扱うこと。

---

## 9. テーブル操作

## 9.1 必須メソッド
`TableServiceClient`:
- `create_table(name)`
- `create_table_if_not_exists(name)`
- `delete_table(name)`
- `list_tables()`
- `table_client(name)`

`TableClient`:
- `create_if_not_exists()`
- `delete()`
- `exists()`

### 指示
- Create Table API を持つため、**「事前に Pulumi 等で作成済み前提」にはしない**こと。
- テーブル名 validation はクライアント生成時か送信前に早期に行うこと。

---

## 10. entity 操作

## 10.1 必須メソッド
- `insert_entity`
- `get_entity`
- `update_entity`
- `merge_entity`
- `upsert_replace`
- `upsert_merge`
- `delete_entity`
- `query_entities`

## 10.2 ETag 条件付き操作

```rust
pub enum IfMatch {
    Any,
    Etag(String),
}
```

### 指示
- `insert` と `upsert` は明確に分けること。
- `get_entity(partition_key, row_key)` は型付き版と dynamic 版の両方を用意すること。
- `delete_entity` / `update_entity` / `merge_entity` は `IfMatch` を受けられるようにすること。
- v1 で optimistic concurrency を捨てないこと。

---

## 11. Query / paging 設計

## 11.1 v1 の最低ライン
- filter 文字列をそのまま渡せる API
- `$top`
- `$select`
- continuation token
- page API
- 低水準レスポンス情報の一部保持

## 11.2 `QueryPage<T>` を必ず作ること
高水準 stream 的 API だけで済ませない。

```rust
pub struct QueryPage<T> {
    pub items: Vec<T>,
    pub continuation: Option<ContinuationToken>,
    pub request_id: Option<String>,
    pub raw_headers: http::HeaderMap,
}

pub struct ContinuationToken {
    pub next_partition_key: Option<String>,
    pub next_row_key: Option<String>,
    pub original_query: OriginalQuery,
}
```

## 11.3 continuation の扱い
### 指示
- continuation token は `NextPartitionKey` / `NextRowKey` だけでなく、**元の query 条件一式**と結びつけて管理すること。
- 次ページ要求では元の `$filter` / `$select` / `$top` を維持すること。
- **空配列でも continuation が返りうる**前提で実装すること。
- page API を残した上で、必要なら stream helper を後から生やすこと。

## 11.4 query builder の方針
- v1 は **string ベース + 最低限 builder** で十分。
- 複雑式を compile-time で保証しようとしないこと。
- builder は安全な文字列組み立て補助に留めること。

### validation
- `$filter` 内 15 比較超過は、builder が把握できる範囲では validation error を返してよい。
- `$top > 1000` は validation error でよい。

---

## 12. EGT / batch への将来拡張性

## 12.1 v1 の方針
v1 では **高水準 batch DSL は作らない**。

ただし、内部設計は将来の EGT を前提にする。

## 12.2 将来のための制約
### 指示
- リクエスト生成層・署名層・送信層を、**単一 entity 操作専用に密結合させない**こと。
- `PreparedRequest` は任意の `Content-Type` と任意バイト列 body を扱えること。
- `multipart/mixed` + boundary を持つ batch request を送れる前提で設計すること。
- 単一 entity 用の URL builder / body builder を batch 実装で流用できるようにすること。
- batch の署名・ヘッダ付与は「単一 entity API の特殊分岐」ではなく、**共通 request pipeline** 上に載せること。

## 12.3 EGT 仕様メモ（将来用）
- 同一 `PartitionKey` の entity のみ対象
- 最大 100 entity
- 総ペイロード 4 MiB 以下
- 1 batch 内 1 changeset のみ
- 同一 entity に複数操作は不可

---

## 13. エラー設計

## 13.1 必須分類
- `AuthError`
- `TransportError`
- `SerializationError`
- `ServiceError`
- `ValidationError`
- `UnexpectedResponseError`

## 13.2 `ServiceError` に最低限入れるもの
- HTTP status
- Azure error code
- request id
- message
- 可能なら応答本文断片

### 指示
- 400 / 403 / 404 / 409 / 412 は必ず識別しやすくすること。
- `table already exists`、`entity not found`、`precondition failed` は専用 variant を許容する。
- URL に SAS が乗る場合、エラーログへそのまま出さないこと。

---

## 14. バリデーション

## 14.1 即時チェックすべきもの
- テーブル名規則違反
- `PartitionKey` / `RowKey` の禁止文字
- `PartitionKey` / `RowKey` の長さ超過（1024 文字）
- 空文字や不正形式
- `$top > 1000`
- SAS 文字列の明らかな壊れ

## 14.2 可能なら行う best-effort 検査
- プロパティ数超過（255、うち custom は実質 252）
- entity サイズ概算 1 MiB 超過
- EGT 制約違反（同一 PK / 100 件 / 4 MiB）

### 指示
- サーバーだけが知る制約を全再現しようとして複雑化しないこと。
- ただし、**利用者が高確率で踏む制約**はクライアント側で早期に弾くこと。
- validation ルールは `validation/` モジュールに隔離すること。

---

## 15. テスト戦略

## 15.1 テストレーンを 4 つに分けること

### 1. Unit test
対象:
- Shared Key 署名
- SAS 付与
- URI 構築
- table/key validation
- codec
- continuation token 復元
- query builder

ここでは純粋関数テストを優先する。

### 2. HTTP black-box test
対象:
- 実際の HTTP request の method / path / query / header / body 確認
- エラー応答のパース
- retry / middleware の挙動

手段:
- `wiremock` を使う

### 3. Local integration test
対象:
- CRUD
- query
- paging
- create/delete table

手段:
- Azurite を使う

### 4. Cloud conformance test
対象:
- 409 / 404 / 412
- continuation の実挙動
- 認証
- Azurite との差分確認

手段:
- 実 Azure Table Storage を使う

## 15.2 指示
- `MockTransport` を public API の中心に据えないこと。
- request/response script 的な最小モックが内部で必要なら作ってもよいが、**本命の HTTP 試験は wiremock** で行うこと。
- Azurite はローカル反復に使うが、**クラウド互換の最終判定に使わない**こと。

---

## 16. 推奨モジュール構成

```text
src/
  lib.rs
  client/
    service_client.rs
    table_client.rs
    options.rs
  auth/
    mod.rs
    credential.rs
    shared_key.rs
    sas.rs
    token.rs              # 将来用 placeholder 可
  request/
    prepared_request.rs
    pipeline.rs
    headers.rs
    canonicalization.rs
  http/
    reqwest_client.rs
    response.rs
  entity/
    mod.rs
    traits.rs
    dynamic_entity.rs
    property.rs
    system_properties.rs
  codec/
    serialize.rs
    deserialize.rs
    edm.rs
    datetime.rs
  query/
    mod.rs
    builder.rs
    page.rs
    continuation.rs
  error/
    mod.rs
  validation/
    table_name.rs
    key.rs
    limits.rs
```

### 指示
- 認証・request 署名・entity codec を混ぜないこと。
- `client/` は入口に留め、実処理は下位モジュールへ落とすこと。
- `http/` は送信専用、`request/` は送信前構築専用と役割を分けること。

---

## 17. Cargo feature と依存方針

### v1 推奨依存
- `reqwest`
- `serde`
- `serde_json`
- `http`
- `url`
- `bytes`
- `time`
- `thiserror`
- `indexmap`

### dev-dependencies 推奨
- `wiremock`
- `tokio`
- `assert_matches`

### 任意
- `reqwest-middleware`
- `reqwest-retry`

### 指示
- TLS 実装は feature gate で切り替え可能にすること（`rustls-tls` / `native-tls` など）。
- `chrono` は最初から内部標準にしないこと。必要なら相互変換 feature を後で足す。

---

## 18. v1 実装順序

### Phase 1
- endpoint / credential / request pipeline の骨格
- `PreparedRequest`
- Shared Key / SAS
- テーブル名 / キー validation
- 固定 `x-ms-version` / OData header 付与

### Phase 2
- `TableServiceClient`
- `TableClient`
- create/delete/list tables
- dynamic entity の insert/get/delete

### Phase 3
- typed entity API
- update/merge/upsert
- `IfMatch`
- codec の時刻型対応

### Phase 4
- query + `QueryPage<T>`
- continuation token
- `$select` / `$top`
- validation 強化

### Phase 5
- retry 差し込み
- Azurite / cloud conformance test
- 将来 batch を足せる点の点検

---

## 19. 実装上の禁止事項

1. 認証情報を毎回 ad-hoc に文字列結合して各 API メソッドへ渡すこと。
2. `reqwest::Client` や `reqwest::Response` を公開 API に露出すること。
3. 公開 trait として安易に `async fn` を置き、後から `Send` 境界変更不能な API を固定すること。
4. Table service の Shared Key 実装を Blob/Queue 用と雑に共通化すること。
5. `Accept` を省略し、Atom/XML 既定へ落ちる実装にすること。
6. `DataServiceVersion` / `MaxDataServiceVersion` / `x-ms-version` を曖昧に扱うこと。
7. `PartitionKey` / `RowKey` を通常プロパティ扱いして codec で混線させること。
8. 型付き API しか用意せず、動的 property bag を捨てること。
9. continuation token を呼び出し側から見えない形で握り潰すこと。
10. query を stream API だけにして、page API を消すこと。
11. batch を v1 非対応にすることを理由に、`multipart/mixed` を載せられない request pipeline にすること。
12. Azurite だけで互換性確認を済ませること。
13. `x-ms-version` を自動で最新版へ追従させること。

---

## 20. 実装担当者への最終指示

- ATS の本質は **KVS + 固定主キー (`PartitionKey`, `RowKey`) + スキーマレス** である。RDB 的 abstraction を持ち込みすぎないこと。
- ただし利用者は型付き利用を望んでいるため、**高水準 API は型安全寄り**に作ること。
- 一方で ATS 固有の制約や将来の EGT を考えると、**低水準 API と request pipeline は必ず残す**こと。
- v1 では **Shared Key と SAS を確実に完成**させること。Entra ID は後続でよい。
- Transport のために Java 的な大抽象を公開しないこと。**公開 API は client、中では request pipeline** という分割にすること。
- 過剰な DSL や derive macro は後回しにし、まず **壊れない CRUD / query / paging / concurrency** を成立させること。

---

## 21. 実装受け入れ条件

以下を満たしたら v1 を受け入れてよい。

1. Shared Key と SAS の両方で認証できる。
2. `TableServiceClient` と `TableClient` が分離されている。
3. テーブル create/delete/list ができる。
4. 型付き / 動的の両方で entity CRUD ができる。
5. upsert ができる。
6. `IfMatch` による条件付き更新/削除ができる。
7. query と paging が最低限使え、`QueryPage<T>` がある。
8. continuation token が元 query 条件を保持できる。
9. `Accept: application/json;odata=nometadata` が既定で送られる。
10. `DataServiceVersion` / `MaxDataServiceVersion` / `x-ms-version` が一貫して付与される。
11. request/署名/codec が単体試験できる。
12. wiremock / Azurite / 実 ATS の 3 レーン以上で試験される。
13. 将来 EGT を載せられる request pipeline になっている。

---

## 付録 A. 仕様メモ（実装で踏み抜きやすい点）

- `Timestamp` はサーバー管理。送っても無視される。
- `null` は保存されない。`Option<T>::None` は「欠落」と同義。
- `Accept` 省略時の既定は Atom。
- JSON は 2015-12-11 以降必須。
- `x-ms-version` は固定値を使う。
- Query は 1000 件 / 5 秒 / 30 秒制約。
- continuation は件数超過時だけではない。
- `$top` は全件上限ではなく 1 ページ上限。
- `$filter` は 15 比較まで。
- EGT は同一 `PartitionKey`・100 件・4 MiB。

---

## 付録 B. 参考資料（2026-03-18 取得）

- Rust Blog: Announcing `async fn` and return-position `impl Trait` in traits  
  https://blog.rust-lang.org/2023/12/21/async-fn-rpit-in-traits/
- Microsoft Learn: Authorize with Shared Key (REST API)  
  https://learn.microsoft.com/en-us/rest/api/storageservices/authorize-with-shared-key
- Microsoft Learn: Payload format for Table service operations (REST API)  
  https://learn.microsoft.com/en-us/rest/api/storageservices/payload-format-for-table-service-operations
- Microsoft Learn: Setting the OData data service version headers  
  https://learn.microsoft.com/en-us/rest/api/storageservices/setting-the-odata-data-service-version-headers
- Microsoft Learn: Querying tables and entities (REST API)  
  https://learn.microsoft.com/en-us/rest/api/storageservices/querying-tables-and-entities
- Microsoft Learn: Summary of Table Storage functionality  
  https://learn.microsoft.com/en-us/rest/api/storageservices/summary-of-table-service-functionality
- Microsoft Learn: Performing entity group transactions (REST API)  
  https://learn.microsoft.com/en-us/rest/api/storageservices/performing-entity-group-transactions
- Microsoft Learn: Understanding the Table service data model (REST API)  
  https://learn.microsoft.com/en-us/rest/api/storageservices/understanding-the-table-service-data-model
- Microsoft Learn: Scalability and performance targets for Table storage  
  https://learn.microsoft.com/en-us/azure/storage/tables/scalability-targets
- Microsoft Learn: Versioning for Azure Storage  
  https://learn.microsoft.com/en-us/rest/api/storageservices/versioning-for-the-azure-storage-services
- Microsoft Learn: Use the Azurite emulator for local Azure Storage development  
  https://learn.microsoft.com/en-us/azure/storage/common/storage-use-azurite
- Microsoft Learn: Use the Azure Storage Emulator for development and testing  
  https://learn.microsoft.com/en-us/azure/storage/common/storage-use-emulator
- docs.rs: wiremock  
  https://docs.rs/wiremock/
- docs.rs: reqwest-middleware  
  https://docs.rs/reqwest-middleware/latest/reqwest_middleware/
