# `create_compression_session` で VTSessionSetProperties 失敗時にセッションがリークする

- Created: 2026-07-30
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-encoder-session-leak
- Polished: {YYYY-MM-DD}

## 目的

`Encoder::new` 内で `VTCompressionSessionCreate` が成功した後に `add_common_properties` または `VTSessionSetProperties` が失敗すると、生成済みの `VTCompressionSessionRef` が解放されずにリークする。`Encoder` 構造体が構築されないため `Drop` も走らない。

## 現状

`src/encoder.rs` の `create_compression_session` 関数で、`VTCompressionSessionCreate` の成功後に `session` ポインタをガードせずにプロパティ設定を進めている。`add_common_properties` の `?` や `VTSessionSetProperties` の `Error::check` で早期リターンすると、`session` が誰にも解放されない。

## 設計方針

`VTCompressionSessionCreate` 成功直後に `session` を `CfPtrMut` でガードし、関数成功時に `std::mem::forget` でガードを解除するパターンを使う。

## 完了条件

`create_compression_session` 内のどのエラーパスでも `VTCompressionSessionRef` がリークしないこと。
