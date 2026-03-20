# Contributing

## 基本方針

- バグ修正、テスト追加、ドキュメント改善、API の使い勝手改善を歓迎します
- 大きな仕様変更や公開 API の変更は、先に Issue か Discussion 相当の場で方向性を合わせてください
- 既存の命名、エラーハンドリング、非同期 API の設計方針に合わせてください

## 開発環境

- Rust stable を使ってください
- 依存関係を変更した場合は `Cargo.lock` も更新してください
- TLS feature を触る変更では `rustls-tls` と `native-tls` の両方への影響を確認してください

## ローカル確認

変更前後で最低限次を実行してください。

```bash
cargo fmt --all
cargo test
```

必要に応じて追加で以下も確認してください。

```bash
cargo test --all-features
cargo deny check licenses --all-features
```

## Pull Request

- 変更の目的と背景を書く
- 互換性に影響がある場合は明記する
- 振る舞いを変えた場合はテストを追加する
- ドキュメントや README の更新が必要なら同じ PR に含める
