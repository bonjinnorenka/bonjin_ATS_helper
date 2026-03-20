# ATS mock backend 実装指示書（改訂版）

## 1. 文書の位置づけ

この文書は、Rust 製 Azure Table Storage (ATS) クライアントライブラリに対して、**永続化可能な mock backend** を追加実装するための指示書である。

目的は、単なる一時的なテストダブルではなく、**プロセス再起動後もデータを保持できる ATS 互換のローカル実行基盤**を用意することにある。
加えて、**query（特に `$filter`）は既存の ATS 向け parser / AST / evaluator 実装をそのまま流用**し、mock 専用の別系統実装を作らないことを必須要件とする。

この文書は **ATS の実仕様に近い検証基盤を作る**ためのものであり、単なる「都合のよい in-memory DB」を作る指示ではない。
ATS には、結果順序、ページング、`$top`、`$select`、`$filter`、ETag/If-Match、`Timestamp`、null、名前制約、サイズ制約、batch 制約といった明確な仕様があるため、mock 側でもそれらを意識した挙動にする。 ([Microsoft Learn][2])

---

## 2. 事実 / 方針

### 2.1 事実として固定する ATS 仕様

以下は設計判断ではなく、ATS 側の仕様として固定である。

* Query 結果は **`PartitionKey` 昇順 → `RowKey` 昇順** で返る。別順序での並び替えはサポートされない。 ([Microsoft Learn][2])
* Query は 1 回で最大 1,000 件、実行時間は最大 5 秒で、継続トークンは **結果 0 件でも返り得る**。また `$top` は「全体件数」ではなく **ページあたり件数** である。 ([Microsoft Learn][3])
* `$filter` / `$top` / `$select` はサポート対象だが、追加の OData query option はサポートされない。`$filter` では 15 個を超える離散比較は不可である。 ([Microsoft Learn][2])
* テーブル名、プロパティ名、`PartitionKey` / `RowKey` には明確な制約がある。エンティティは最大 255 プロパティ（うち 3 つは system property）、総サイズは 1 MiB 上限である。 ([Microsoft Learn][4])
* `Timestamp` はサーバー管理であり、クライアントからの変更は無視される。ETag は opaque であり、`If-Match: *` で無条件更新できる。 ([Microsoft Learn][5])
* `null` プロパティは永続化されない。`Update/Replace` では `null` または未指定でプロパティを落とせるが、`Merge` では `null` は永続化されず、削除にもならない。 ([Microsoft Learn][6])
* Entity Group Transaction は **同一 `PartitionKey`**、**最大 100 entity**、**合計 4 MiB 以下**、**同一 entity への複数操作禁止** という制約を持つ。 ([Microsoft Learn][7])
* Table Delete は即物理削除ではなく、削除マーク後しばらく `TableBeingDeleted` になり得る。少なくとも 40 秒程度かかることがある。 ([Microsoft Learn][8])

### 2.2 この mock で採用する方針

* ATS の **wire 互換 HTTP サーバー**は作らない。
* ただし、**公開クライアント API の意味論**はできるだけ ATS に寄せる。
* `$filter` だけ共通化して、CRUD やページングや ETag が適当、という状態は不可。
* v1 では性能よりも **正しさ・再現性・永続化安全性・本番差分の明示** を優先する。
* ATS の完全再現が難しい点は、**再現しないことを仕様として明記**する。

---

## 3. 必須要件

### 3.1 公開 API を変えない

利用者コードから見て、real ATS backend と mock backend は**同じ client API で差し替え可能**であること。
mock のためだけに別 client、別 entity 型、別 query API を増やしてはならない。

### 3.2 async/await の扱いを明記する

既存ライブラリが async ベースなら、mock backend も **同じく async** にすること。
同期版 mock を別に作るのは禁止。

backend 抽象については、次のどちらかを明示的に選ぶこと。

* **静的ディスパッチ前提**
  `Client<B: Backend>` のような generic backend を使う。
  この場合、native の async trait 設計を採用してよい。

* **動的ディスパッチ前提**
  `Arc<dyn Backend>` を使う。
  この場合、backend trait は object-safe でなければならないため、`async fn` をそのまま trait object に流す設計にはしない。boxed future などを使って object-safe にすること。

`#[async_trait]` を必須依存にしてはならない。
必要なら使ってよいが、**それが唯一の設計前提であるかのように文書化してはならない**。

### 3.3 query 実装を共通化する

`$filter` の lexer / parser / AST / evaluator は **real/mock 完全共通** とする。
mock 専用の簡易 evaluator、あるいは CRUD 層の if 文で query を模倣する実装は禁止。

---

## 4. スコープ

### 4.1 本実装に含めるもの

* テーブルの作成 / 削除 / existence 確認
* エンティティの insert / get / replace / merge / upsert / delete
* 永続化
* 再起動後の復元
* query 実行

  * `$filter`
  * `$top`
  * `$select`
  * 既存公開 API がページング情報を持つなら continuation も対応
* ETag / If-Match / Timestamp の基本意味論
* 妥当なエラー返却
* 単体テスト / mock-only 結合テスト / real-ATS 互換比較テスト

### 4.2 本実装に含めないもの

初期実装では以下は不要、または明示的な非対応でよい。

* HTTP server としての ATS エミュレーション
* Azure 認証や署名の再現
* サービス側の 5 秒 timeout 自体の再現
* delete table の 40 秒 tombstone 状態の再現
* 複数プロセスから同一データディレクトリを同時共有する保証
* ATS 未対応機能の独自 query 拡張

### 4.3 batch / Entity Group Transaction の扱い

* **既存公開 API に batch がある場合**
  mock backend でも **ATS 制約付きで実装すること**。同一 `PartitionKey`、最大 100 entity、合計 4 MiB、同一 entity への複数操作禁止を必ず検証する。 ([Microsoft Learn][7])
* **既存公開 API に batch がない場合**
  v1 では新規公開 API を追加しなくてよい。ただし backend 内部設計は将来追加しやすい形にすること。

---

## 5. backend 抽象

backend 層は少なくとも以下の責務を持つこと。

* create_table
* delete_table
* table_exists
* list_tables
* get_entity
* insert_entity
* replace_entity
* merge_entity
* insert_or_replace
* insert_or_merge
* delete_entity
* query_entities
* 必要なら batch

real backend は HTTP / SDK 呼び出し、mock backend はローカル store 実装とする。

---

## 6. 互換性モード

mock backend には `strict_mode: bool` を持たせること。**デフォルトは true** とする。

### strict_mode = true で必須とするもの

* テーブル名制約の検証
* `PartitionKey` / `RowKey` 禁止文字・長さ制約の検証
* 予約 system property 名の禁止
* property 名長さ・文字制約の検証
* property 数上限・entity 総サイズ上限の検証
* ATS 未対応 query option / operator の拒否
* `$top <= 1000` の検証
* `$filter` 離散比較数上限の検証
* ETag 条件チェック
* 永続化データ破損時の fail-fast

### strict_mode = false で緩和してよいもの

* 正確な entity サイズ計算の一部簡略化
* 一部の高コストな検証
* delete table tombstone 非再現を許容

ただし、`strict_mode = false` でも **主キー一意性・型復元・永続化整合性** は必須である。

---

## 7. データモデル

### 7.1 テーブル

各テーブルは少なくとも以下を持つこと。

* `table_name`
* `entities`
* `table_revision`
* 必要なら `deletion_state`（ただし v1 では未使用可）

### 7.2 エンティティ

各エンティティは少なくとも以下を保持すること。

* `PartitionKey`
* `RowKey`
* `Timestamp`
* `ETag`
* `properties: BTreeMap<String, TypedValue>`

### 7.3 system property の扱い

`PartitionKey` / `RowKey` / `Timestamp` は **properties map に入れてはならない**。
内部モデル上は明示フィールドとして持つこと。
CRUD 入力時に properties 側へこれらと同名のキーが来たらエラーにすること。system property 名は予約語として扱う。 ([Microsoft Learn][4])

### 7.4 値型

既存 query evaluator が比較できる型はすべて mock 側も保持できること。少なくとも以下を想定する。

* String
* Bool
* Int32
* Int64
* Double
* DateTime
* Guid
* Binary

**Null は永続化対象の型として持たない。**
ATS は null を保存しないため、mock 側でも永続層には保存しない。 ([Microsoft Learn][6])

---

## 8. 永続化方式

### 8.1 採用方式

v1 は **ファイルベース snapshot** とする。DB 依存は入れない。

推奨構成:

```text
<root>/
  manifest.json
  tables/
    Users.json
    Orders.json
```

### 8.2 `auto_create_if_missing` の定義

`auto_create_if_missing` は **root directory と manifest を自動作成するか** を意味する。
**存在しないテーブルを自動作成する意味ではない。**
テーブル自動作成は ATS 互換性を壊すため禁止。

### 8.3 store format version

`manifest.json` に少なくとも以下を持つこと。

* `store_format_version: 1`
* `created_at`
* `tables`
* `library_semver`（任意）
* `strict_mode`

方針は以下とする。

* 同じ `store_format_version` 内でのみ読み込み互換を保証
* バージョン不一致時は原則 fail-fast
* 自動マイグレーションは v1 では不要
* 将来マイグレーションを書く場合は `migrate_vN_to_vNplus1` を明示関数で持つ

### 8.4 永続化 JSON フォーマットを固定する

永続化 JSON は **ATS の REST payload の再現ではなく、内部永続化フォーマット**である。
実装者ごとにフォーマットを変えてはならない。タグ付き型表現を必須とする。

#### 例

```json
{
  "table_name": "Users",
  "table_revision": 42,
  "entities": [
    {
      "partition_key": "p1",
      "row_key": "r1",
      "timestamp": "2026-03-19T10:00:00.0000000Z",
      "etag": "W/\"mock-42\"",
      "properties": {
        "Name": { "type": "String", "value": "Alice" },
        "IsActive": { "type": "Bool", "value": true },
        "Age": { "type": "Int32", "value": 23 },
        "Count": { "type": "Int64", "value": "9223372036854775807" },
        "AmountDue": { "type": "Double", "value": "200.23" },
        "When": { "type": "DateTime", "value": "2026-03-19T10:00:00Z" },
        "Id": { "type": "Guid", "value": "c9da6455-213d-42c9-9a79-3e9149a57833" },
        "Blob": { "type": "Binary", "value": "BASE64..." }
      }
    }
  ]
}
```

### 8.5 型ごとの永続化規則

* String: JSON string
* Bool: JSON boolean
* Int32: JSON integer
* Int64: **decimal string**
* Double: **decimal string**
* DateTime: UTC RFC3339 / ISO 8601 文字列、末尾 `Z`
* Guid: lowercase hyphenated canonical string
* Binary: base64 string

`Int64` と `Double` を文字列にする理由は、JSON 数値処理系依存で精度や表現がぶれるのを避けるためである。
ATS の wire でも型付き JSON 表現が使われるため、mock の内部永続化でも型タグを落としてはならない。 ([Microsoft Learn][6])

復元時は `Int64` / `Double` の文字列表現を明示的に検証すること。

* `Int64` は 64-bit signed integer として parse できなければならず、非数値・空文字・範囲外は `CorruptedStore` として fail-fast する
* `Double` は有限の数値文字列として parse できなければならず、非数値・空文字・不正形式は `CorruptedStore` として fail-fast する
* `Double` の `NaN` / `Infinity` / `-Infinity` は `strict_mode = true` では `CorruptedStore` として拒否する
* `strict_mode = false` では restore 時に限り `NaN` / `Infinity` / `-Infinity` を受理してよいが、通常の API 書き込みは canonical な数値文字列のみを永続化すること

### 8.6 null の永続化禁止

`null` 値の property は保存してはならない。
復元後も property 自体が存在しない状態でなければならない。
これは ATS の保存意味論に合わせる。 ([Microsoft Learn][6])

### 8.7 書き込み方式

永続化時は必ず以下の手順を使うこと。

1. 新しい snapshot をメモリ上で生成
2. 一時ファイルへ書く
3. `flush`
4. durability mode が `Durable` の場合は file `fsync`
5. rename で本ファイルへ置換
6. durability mode が `Durable` の場合は parent directory も `fsync`

---

## 9. flush policy と durability mode

`flush_policy` と `durability_mode` を分離すること。

### 9.1 flush policy

* `WriteThrough`
  成功した mutating operation ごとに persist する。**デフォルト**
* `Manual`
  明示的 `flush()` まで persist しない。テスト高速化用途のみ

v1 では `Interval` などのバックグラウンド flush は不要。

### 9.2 durability mode

* `Fast`
  flush + rename まで
* `Durable`
  file fsync + rename + parent dir fsync

デフォルトは `Fast` でよい。
ただし `strict_mode = true` かつ永続化破損耐性を重視するテストでは `Durable` を使えるようにする。

---

## 10. 同時実行制御

v1 は **単一プロセス専用** とし、複数プロセス共有は非対応とする。

ロック戦略は次のどちらかに限定する。

* store 全体を 1 つの async `Mutex` で直列化する
* または metadata と table 単位の lock に分ける

ただし v1 では、**単純さと正しさを優先して store 全体直列化を推奨**する。
中途半端な細粒度 `RwLock` による deadlock / stale overwrite は避ける。

### 10.1 mutating operation の実行順序

mutating operation は次の順序で行うこと。

1. lock 取得
2. 現在状態を clone
3. clone に対して検証と変更適用
4. clone から snapshot JSON を生成
5. persist 実施
6. persist 成功後に live state を差し替え
7. lock 解放

これにより、persist 失敗時に「メモリだけ更新済み」という状態を作らない。

---

## 11. 入力制約の検証

### 11.1 テーブル名

テーブル名は以下を満たすこと。strict では必須、relaxed でも原則維持すること。

* 英数字のみ
* 先頭数字禁止
* 長さ 3〜63
* case-insensitive
* `tables` は予約名として拒否 ([Microsoft Learn][4])

### 11.2 `PartitionKey` / `RowKey`

* 長さは各 1024 文字以下
* `/`, `\`, `#`, `?` を禁止
* 制御文字を禁止
* null 禁止、空文字は許容 ([Microsoft Learn][4])

### 11.3 property 名

* case-sensitive
* 最大 255 文字
* XML / URL 的に不正な文字を禁止
* `-` を禁止
* `PartitionKey` / `RowKey` / `Timestamp` と同名禁止 ([Microsoft Learn][4])

### 11.4 entity 制約

* 最大 255 property（system property を含む）
* custom property は最大 252
* entity 合計サイズは 1 MiB 以下 ([Microsoft Learn][4])

---

## 12. CRUD 意味論

### 12.1 insert

* 同一 `(PartitionKey, RowKey)` があれば `EntityAlreadyExists`
* `Timestamp` は backend が現在時刻で設定
* `ETag` は backend が新規発行
* property に `null` が来ても保存しない

### 12.2 get

* なければ `EntityNotFound`

### 12.3 replace (`Update Entity` 相当)

* 対象がなければ `EntityNotFound`
* entity 全面置換
* 未指定 property は削除
* `null` property も保存しないため、結果的に削除扱い
* `Timestamp` は入力値を無視して更新
* ETag 条件が不一致なら `UpdateConditionNotSatisfied` / 412 相当
* `If-Match: *` 相当の無条件上書きをサポートすること ([Microsoft Learn][6])

### 12.4 merge (`Merge Entity` 相当)

* 対象がなければ `EntityNotFound`
* 指定された **非 null property のみ** を更新
* 未指定 property は保持
* `null` property は永続化されず、削除にもならない
* `Timestamp` は入力値を無視して更新
* ETag 条件が不一致なら `UpdateConditionNotSatisfied` / 412 相当
* `If-Match: *` 相当の無条件 merge をサポートすること ([Microsoft Learn][1])

### 12.5 upsert

* `insert_or_replace` は replace 意味論
* `insert_or_merge` は merge 意味論
* `Timestamp` は常に backend 管理
* `ETag` は更新ごとに変わる

### 12.6 delete

* 対象がなければ `EntityNotFound`
* ETag 条件付き delete を公開 API が持つなら mock でも検証する
* `If-Match: *` 相当の無条件 delete をサポートする

---

## 13. Timestamp / ETag 仕様

### 13.1 Timestamp

* 外部入力の `Timestamp` は無視
* insert / replace / merge / upsert で必ず backend が更新
* 値は UTC で管理する ([Microsoft Learn][5])

### 13.2 ETag

* ETag は **opaque** として扱う
* mock では `W/"mock-<global_revision>"` のような内部形式でよい
* 同一 entity で変更が入れば必ず新値に変わること
* 同一更新で衝突しないこと
* ETag の exact string を real ATS と一致させることは要求しない。ATS 側も opaque と明記しているため、比較テストで exact match をしてはならない。 ([Microsoft Learn][6])

### 13.3 If-Match

* exact match なら成功
* 不一致なら 412 相当
* `*` なら無条件成功
* 公開 API が ETag 省略を許す場合、その意味は高レベル API 定義に合わせること。
  ただし REST 生互換層を持つなら、Update/Merge で `If-Match` 欠如時に upsert 側へ流れる挙動まで扱うかを別途仕様化すること。 ([Microsoft Learn][6])

---

## 14. query 仕様

### 14.1 サポート対象

v1 でサポートする query は **ATS サブセットに一致**させること。

* `$filter`
* `$top`
* `$select`

その他の OData query option は明示的に reject すること。
`orderby`, `skip`, `count`, `groupby`, `join`, `contains` などを独自拡張で受けてはならない。 ([Microsoft Learn][2])

### 14.2 `$filter`

* 前回定義済みの lexer / parser / AST / evaluator をそのまま使う
* ATS サブセットから外れる構文は reject
* 15 個を超える離散比較は reject
* 定数の型は対象 property 型と整合しなければならない ([Microsoft Learn][2])

### 14.3 結果順序

* 結果順序は **必須で `PartitionKey` 昇順 → `RowKey` 昇順**
* lexical 比較であること
* 他順序の並び替え機能は提供しない ([Microsoft Learn][2])

### 14.4 評価順序

mock backend は次の順序で query を評価すること。

1. query option の妥当性検証
2. 全 entity を ATS 順序で走査
3. **フル entity** に対して `$filter` 評価
4. ページ境界 / `$top` を適用
5. そのページに対して `$select` projection
6. continuation token を生成

`$select` を `$filter` より先に適用してはならない。
それをやると、select 対象外 property を使った filter が壊れる。

### 14.5 `$top`

* `1..=1000` の範囲のみ許可
* continuation を使う場合、**ページサイズ** として扱う
* 「全体件数上限」として実装してはならない ([Microsoft Learn][3])

### 14.6 `$select`

* projection は返却時にのみ適用
* internal store は常にフル entity を保持
* `PartitionKey`, `RowKey`, `Timestamp` を返すかどうかは既存 client API の方針に合わせること

### 14.7 continuation token

既存公開 API が continuation を公開しているなら、mock も対応すること。

方針は以下とする。

* offset ベースは禁止
* cursor ベースにする
* token は少なくとも「最後に返した `PartitionKey`,`RowKey`」と「元 query の整合確認情報」を保持する
* 0 件ページでも token を返し得る設計にする
* 次ページ要求時は original `$filter` / `$select` / `$top` を引き継ぐこと ([Microsoft Learn][3])

既存公開 API が continuation を公開していない場合でも、内部 query engine は cursor ベースにしておくこと。将来 API 拡張時に offset 実装へ逃げないためである。

---

## 15. batch / Entity Group Transaction

### 15.1 適用条件

公開 API に batch がある場合、以下を必ず検証する。

* 全 entity が同一テーブル
* 全 entity が同一 `PartitionKey`
* 最大 100 entity
* 合計 4 MiB 以下
* 同一 entity は 1 回だけ出現
* query operation を change set に混ぜない ([Microsoft Learn][7])

### 15.2 原子性

batch は次のように実装すること。

1. 現在 table state を clone
2. clone に対して全操作を検証・適用
3. 一括 persist
4. 成功時のみ live state を差し替え

途中失敗時は **全件失敗** であり、部分成功は許されない。

---

## 16. table delete の差分定義

ATS 実サービスでは delete table 後に tombstone 状態が入り、少なくとも 40 秒程度 `TableBeingDeleted` が起こり得る。 ([Microsoft Learn][8])

v1 の mock では以下の方針とする。

* delete table は **即時削除**
* `TableBeingDeleted` の遅延状態は再現しない
* README と仕様にこの差分を明記する

retry ロジックや 409 `TableBeingDeleted` を検証したい場合は、将来 `compat_emulate_table_deletion_delay` のような別オプションで追加する。

---

## 17. エラー体系

エラー型は既存ライブラリの公開エラー体系へ統合すること。
ただし内部では ATS 相当コードを保持すること。最低でも以下を区別する。

* `TableAlreadyExists`
* `TableNotFound`
* `TableBeingDeleted`（v1 非使用可）
* `EntityAlreadyExists`
* `EntityNotFound`
* `UpdateConditionNotSatisfied`
* `TooManyProperties`
* `EntityTooLarge`
* `PropertyNameInvalid`
* `PropertyNameTooLong`
* `PropertyValueTooLarge`
* `InvalidDuplicateRow`
* `InvalidInput`
* `UnsupportedQueryOption`
* `InvalidFilterSyntax`
* `InvalidFilterSemantics`
* `PersistenceIo`
* `CorruptedStore`

代表的な ATS エラーコードと HTTP 状態は Microsoft Learn に定義があるため、mock 側もできるだけそれに寄せる。 ([Microsoft Learn][9])
永続化 JSON の型不整合や hand-edited な `Int64` / `Double` 文字列の parse 失敗は `CorruptedStore` に分類すること。

---

## 18. テスト要件

### 18.1 単体テスト

最低限、以下を用意すること。

#### 永続化

* テーブル作成後に再起動して残る
* insert 後に再起動して残る
* replace / merge / delete 後も再起動後に正しい
* 破損 JSON で fail-fast
* store format version 不一致で fail-fast

#### 入力制約

* 不正テーブル名
* 不正 property 名
* 不正 `PartitionKey` / `RowKey`
* property 数超過
* entity サイズ超過
* system property 名衝突

#### CRUD

* duplicate key insert
* missing key get/delete
* replace で未指定 property が消える
* merge で未指定 property が残る
* merge で `null` 指定しても削除されない
* replace で `null` / omission により削除される
* `Timestamp` 入力が無視される
* ETag 一致/不一致
* `If-Match: *`

#### query

* 単純比較
* 論理積 / 論理和 / 括弧
* 型ごとの比較
* missing property
* `PartitionKey` / `RowKey` 比較
* unsupported query option
* 15 比較超過
* `$top <= 1000`
* `$select` と `$filter` の組合せ
* continuation
* 0 件ページ + continuation

### 18.2 ファイルシステム独立性

すべての永続化テストは `tempfile::TempDir` 等を使い、**テストごとに専用ディレクトリ**を割り当てること。
共有ディレクトリを使うテストは禁止。

### 18.3 mock / real 比較テスト

real ATS 比較テストは以下の条件で行うこと。

* `ATS_TEST_CONNECTION_STRING` 等の環境変数があるときのみ実行
* CI の通常ジョブでは skip 可
* nightly / manual job で実行可

比較対象は以下とする。

* CRUD 成否
* 主キー集合
* query 結果順
* continuation の意味論
* ETag 条件成否
* null / replace / merge の意味論
* エラーコード種別

**ETag の exact string 一致は比較対象にしてはならない。**
opaque だからである。 ([Microsoft Learn][6])

---

## 19. 実装順序

1. backend 抽象の固定（async 方針を先に決める）
2. typed entity model
3. in-memory store
4. 入力制約検証
5. CRUD
6. Timestamp / ETag / If-Match
7. 永続化フォーマット固定
8. snapshot persist / restore
9. 共通 evaluator を用いた query
10. continuation
11. batch
12. テスト
13. README

---

## 20. README に必ず書くこと

* mock backend の生成方法
* root path 指定方法
* `auto_create_if_missing`
* `flush_policy`
* `durability_mode`
* `strict_mode`
* query は real/mock 共通 evaluator を使うこと
* ETag は opaque であり exact string 比較してはいけないこと
* delete table tombstone は v1 で再現しないこと
* 複数プロセス共有非対応であること

---

## 21. 完了条件

以下をすべて満たしたら完了とする。

1. mock backend を指定して client が起動できる
2. CRUD が動く
3. 再起動後もデータが残る
4. `$filter` が共通 parser / AST / evaluator 経由で動く
5. `$top`, `$select`, continuation の意味論が固定されている
6. null / merge / replace / Timestamp / ETag が ATS に沿っている
7. strict mode で ATS 制約を検証できる
8. batch が公開 API にあるなら制約付きで動く
9. テストが通る
10. README に差分が明記されている

---

## 22. 最終判断基準

判断に迷った場合は、以下の順で優先すること。

1. ATS の実仕様との整合
2. 既存公開 API との整合
3. real/mock の比較可能性
4. 永続化の安全性
5. 実装単純性
6. 性能

mock backend は速さのために作るのではない。
**本番との差分を早い段階で炙り出すために作る**。その目的を壊す設計は採用しない。

---

必要なら次に、この改訂版をベースにして **Rust の trait / struct / module / error enum / persistence schema まで落とした実装タスク表** にする。

[1]: https://learn.microsoft.com/en-us/rest/api/storageservices/merge-entity "Merge Entity (REST API) - Azure Storage | Microsoft Learn"
[2]: https://learn.microsoft.com/en-us/rest/api/storageservices/querying-tables-and-entities "Querying tables and entities (REST API) - Azure Storage | Microsoft Learn"
[3]: https://learn.microsoft.com/en-us/rest/api/storageservices/query-timeout-and-pagination "Query timeout and pagination (REST API) - Azure Storage | Microsoft Learn"
[4]: https://learn.microsoft.com/en-us/rest/api/storageservices/understanding-the-table-service-data-model "Understanding the Table service data model (REST API) - Azure Storage | Microsoft Learn"
[5]: https://learn.microsoft.com/en-us/rest/api/storageservices/designing-a-scalable-partitioning-strategy-for-azure-table-storage "Design a scalable partitioning strategy for Azure Table storage (REST API) - Azure Storage | Microsoft Learn"
[6]: https://learn.microsoft.com/en-us/rest/api/storageservices/update-entity2 "Update Entity (REST API) - Azure Storage | Microsoft Learn"
[7]: https://learn.microsoft.com/en-us/rest/api/storageservices/performing-entity-group-transactions "Performing entity group transactions (REST API) - Azure Storage | Microsoft Learn"
[8]: https://learn.microsoft.com/en-us/rest/api/storageservices/delete-table "Delete Table (REST API) - Azure Storage | Microsoft Learn"
[9]: https://learn.microsoft.com/ja-jp/rest/api/storageservices/table-service-error-codes "Table Storage エラー コード (REST API) - Azure Storage | Microsoft Learn"
